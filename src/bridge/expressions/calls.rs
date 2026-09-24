use oxc_ast::ast::Expression;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::TypeId;
use crate::types::Type;

use super::super::context::CheckContext;
use super::infer_expression_type;
use super::{
    check_excess_properties, collect_generic_param_constraints, expected_param_type,
    infer_member_access_type, infer_type_param_bindings, ordered_generic_param_ids,
    resolve_identifier_type, substitute_type_params,
};

// Resolves an explicit call-site type argument list, e.g. the <string> in
// identity<string>(x), into concrete TypeIds. Returns an empty vec for a call
// with no such list (`call.type_arguments`/`new_expr.type_arguments` being the
// ordinary, common case: a bare `identity(x)`), so callers don't need to
// special-case "none given" separately from "given but unresolvable".
// An individual type argument this checker cannot resolve (e.g. it names
// something not in scope) is dropped rather than aborting the whole list, the
// same graceful-degradation stance taken everywhere else in this checker for a
// single unresolved piece of an otherwise-checkable construct.
fn resolve_explicit_type_arguments(
    type_arguments: Option<&oxc_ast::ast::TSTypeParameterInstantiation>,
    ctx: &mut CheckContext<'_, '_>,
) -> Vec<TypeId> {
    let Some(type_arguments) = type_arguments else {
        return Vec::new();
    };
    type_arguments
        .params
        .iter()
        .filter_map(|ty| {
            crate::type_annotation::resolve_ts_type(ty, &mut ctx.namespace, &mut ctx.arena)
        })
        .collect()
}

// Everything about one call or `new` site that check_callable needs besides the
// callee's type, the scope info, and the checking context. Grouped into one value
// so check_callable stays under clippy's too_many_arguments limit instead of
// growing another positional parameter every time a call feature is added.
struct CallSite<'s, 'ast> {
    arguments: &'s [oxc_ast::ast::Argument<'ast>],
    span: Span,
    callee_name: &'s str,
    is_new: bool,
    explicit_type_args: &'s [TypeId],
}

pub(super) fn infer_call_expression_type(
    call: &oxc_ast::ast::CallExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let (callee_type, callee_name) = match &call.callee {
        Expression::Identifier(ident) => (
            resolve_identifier_type(ident, scoping, ctx),
            ident.name.to_string(),
        ),
        Expression::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            let property_type =
                infer_member_access_type(object_type, &member.property.name, member.span(), ctx);
            (property_type, member.property.name.to_string())
        }
        _ => {
            ctx.warning(
                crate::diagnostic_messages::messages::unimplemented_call_expression_kind(),
                call.span(),
            );
            return ctx.arena.error();
        }
    };

    let explicit_type_args = resolve_explicit_type_arguments(call.type_arguments.as_deref(), ctx);

    check_callable(
        callee_type,
        CallSite {
            arguments: &call.arguments,
            span: call.span(),
            callee_name: &callee_name,
            is_new: false,
            explicit_type_args: &explicit_type_args,
        },
        scoping,
        ctx,
    )
}

pub(super) fn infer_new_expression_type(
    new_expr: &oxc_ast::ast::NewExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Expression::Identifier(callee_ident) = &new_expr.callee else {
        ctx.warning(
            crate::diagnostic_messages::messages::unimplemented_new_expression_target(),
            new_expr.span(),
        );
        return ctx.arena.error();
    };

    let callee_type = resolve_identifier_type(callee_ident, scoping, ctx);
    let explicit_type_args =
        resolve_explicit_type_arguments(new_expr.type_arguments.as_deref(), ctx);

    check_callable(
        callee_type,
        CallSite {
            arguments: &new_expr.arguments,
            span: new_expr.span(),
            callee_name: &callee_ident.name,
            is_new: true,
            explicit_type_args: &explicit_type_args,
        },
        scoping,
        ctx,
    )
}

fn check_callable(
    callee_type: TypeId,
    site: CallSite<'_, '_>,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let CallSite {
        arguments,
        span,
        callee_name,
        is_new,
        explicit_type_args,
    } = site;

    let Type::Function(function_type) = ctx.arena.get(callee_type).clone() else {
        // Any and Error both mean "do not report a second, likely-noisy error on
        // top of one already reported (or deliberately suppressed) elsewhere."
        // Arguments are still checked for their own independent problems even
        // though the call itself is not.
        if !matches!(ctx.arena.get(callee_type), Type::Any | Type::Error) {
            let message = if is_new {
                crate::diagnostic_messages::messages::not_a_constructor(callee_name)
            } else {
                crate::diagnostic_messages::messages::not_callable(callee_name)
            };
            ctx.error(message, span);
            return ctx.arena.error();
        }

        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return ctx.arena.error();
    };

    // A parameter this checker could not resolve a type for (declare_top_level
    // left it untyped) means arity cannot be verified honestly, so it is skipped
    // with a warning rather than silently allowed or wrongly flagged.
    if function_type.is_untyped {
        let message = if is_new {
            crate::diagnostic_messages::messages::untyped_constructor_parameter_skips_arity_check(
                callee_name,
            )
        } else {
            crate::diagnostic_messages::messages::untyped_parameter_skips_arity_check(callee_name)
        };
        ctx.warning(message, span);
        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return function_type.return_type;
    }

    let required = function_type
        .params
        .iter()
        .filter(|p| !p.optional && !p.rest)
        .count();
    let has_rest = function_type.params.last().is_some_and(|p| p.rest);
    let max = if has_rest {
        None
    } else {
        Some(function_type.params.len())
    };

    let arity_ok = arguments.len() >= required
        && match max {
            Some(max) => arguments.len() <= max,
            None => true,
        };
    if !arity_ok {
        ctx.error(arity_message(required, max, arguments.len()), span);

        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return function_type.return_type;
    }

    // Every argument's type is inferred once up front and reused below, rather
    // than inferred again inside the parameter-checking loop, since inferring an
    // argument's type can itself report diagnostics; inferring it twice would
    // report the same problem in that argument twice.
    let arg_types: Vec<Option<TypeId>> = arguments
        .iter()
        .map(|arg| {
            arg.as_expression()
                .map(|expr| infer_expression_type(expr, scoping, ctx))
        })
        .collect();

    // A first pass over every argument collects generic parameter bindings before
    // any argument is checked against its expected type, so a generic function's
    // return type can be substituted correctly even when the argument that fixes
    // a type parameter comes after other checked arguments.
    // An explicit call-site type argument list (identity<string>(x)) is
    // positional, with no declaration span of its own, so it is zipped against
    // the function's own type parameters in the order they were declared --
    // see ordered_generic_param_ids for how that order is recovered. Extra
    // type arguments beyond the function's own parameter count are ignored,
    // and a partial list (some but not all of the function's type parameters
    // given explicitly) leaves the rest to ordinary argument-driven inference,
    // matching this checker's general "leave the rest to be inferred/left
    // unresolved" stance rather than treating it as an arity error.
    let mut declared_param_ids = Vec::new();
    for param in &function_type.params {
        ordered_generic_param_ids(&ctx.arena, param.type_id, &mut declared_param_ids);
    }
    ordered_generic_param_ids(
        &ctx.arena,
        function_type.return_type,
        &mut declared_param_ids,
    );

    let mut bindings: Vec<(crate::types::TypeParameterId, TypeId)> = declared_param_ids
        .iter()
        .zip(explicit_type_args.iter())
        .map(|(&id, &explicit)| (id, explicit))
        .collect();
    let locked: Vec<crate::types::TypeParameterId> = bindings.iter().map(|(id, _)| *id).collect();

    for (index, arg_type) in arg_types.iter().enumerate() {
        let Some(arg_type) = arg_type else { continue };
        if let Some(param_type) = expected_param_type(&ctx.arena, &function_type.params, index) {
            infer_type_param_bindings(
                &mut ctx.arena,
                param_type,
                *arg_type,
                &mut bindings,
                &locked,
            );
        }
    }

    // Checked once per call, after every argument has had its chance to inform a
    // binding, and before the per-argument assignability loop below: a type
    // parameter with an `extends` bound (function f<T extends { length: number
    // }>(...)) must have its final, fully-inferred binding satisfy that bound.
    // A parameter nothing ever bound (bindings has no entry for it) is skipped
    // here entirely, matching the same graceful "left unresolved" treatment an
    // uninferred parameter already gets everywhere else.
    let mut constraints = Vec::new();
    for param in &function_type.params {
        collect_generic_param_constraints(&ctx.arena, param.type_id, &mut constraints);
    }
    collect_generic_param_constraints(&ctx.arena, function_type.return_type, &mut constraints);

    for (id, name, constraint) in &constraints {
        let Some((_, bound)) = bindings.iter().find(|(bound_id, _)| bound_id == id) else {
            continue;
        };
        if !ctx.semantic().is_assignable(*bound, *constraint) {
            ctx.error(
                crate::diagnostic_messages::messages::type_argument_constraint_violation(name),
                span,
            );
        }
    }

    for (index, arg) in arguments.iter().enumerate() {
        let Some(arg_expr) = arg.as_expression() else {
            continue;
        };
        let Some(arg_type) = arg_types[index] else {
            continue;
        };
        let Some(param_type) = expected_param_type(&ctx.arena, &function_type.params, index) else {
            continue;
        };
        let expected = substitute_type_params(&mut ctx.arena, param_type, &bindings);
        if !ctx.semantic().is_assignable(arg_type, expected) {
            ctx.error(
                crate::diagnostic_messages::messages::argument_not_assignable(),
                arg_expr.span(),
            );
        } else {
            check_excess_properties(arg_expr, expected, ctx);
        }
    }

    substitute_type_params(&mut ctx.arena, function_type.return_type, &bindings)
}

fn arity_message(
    required: usize,
    max: Option<usize>,
    got: usize,
) -> crate::diagnostic_messages::DiagnosticMessage {
    use crate::diagnostic_messages::messages;
    match max {
        Some(max) if max == required => messages::argument_arity_exact(required, got),
        Some(max) => messages::argument_arity_range(required, max, got),
        None => messages::argument_arity_at_least(required, got),
    }
}
