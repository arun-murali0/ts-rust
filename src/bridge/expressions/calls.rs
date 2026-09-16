use oxc_ast::ast::Expression;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::TypeId;
use crate::types::Type;

use super::super::context::CheckContext;
use super::infer_expression_type;
use super::{
    expected_param_type, infer_member_access_type, infer_type_param_bindings,
    resolve_identifier_type, substitute_type_params,
};

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
                "This kind of call expression is not yet checked by ts-rust.",
                call.span(),
            );
            return ctx.arena.error();
        }
    };

    let not_callable = format!("'{callee_name}' is not callable.");
    let untyped_message = format!(
        "'{callee_name}' has an untyped parameter, so ts-rust can't check this call's arity yet."
    );
    check_callable(
        callee_type,
        &call.arguments,
        call.span(),
        &not_callable,
        &untyped_message,
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
            "`new` on anything other than a plain name is not yet checked by ts-rust.",
            new_expr.span(),
        );
        return ctx.arena.error();
    };

    let callee_type = resolve_identifier_type(callee_ident, scoping, ctx);

    let not_callable = format!("'{}' is not a constructor.", callee_ident.name);
    let untyped_message = format!(
        "'{}' has a constructor with an untyped parameter, so ts-rust can't check arity for `new {}(...)` yet.",
        callee_ident.name, callee_ident.name
    );
    check_callable(
        callee_type,
        &new_expr.arguments,
        new_expr.span(),
        &not_callable,
        &untyped_message,
        scoping,
        ctx,
    )
}

fn check_callable(
    callee_type: TypeId,
    arguments: &[oxc_ast::ast::Argument],
    span: Span,
    not_callable_message: &str,
    untyped_message: &str,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Type::Function(function_type) = ctx.arena.get(callee_type).clone() else {
        // Any and Error both mean "do not report a second, likely-noisy error on
        // top of one already reported (or deliberately suppressed) elsewhere."
        // Arguments are still checked for their own independent problems even
        // though the call itself is not.
        if !matches!(ctx.arena.get(callee_type), Type::Any | Type::Error) {
            ctx.error(not_callable_message, span);
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
        ctx.warning(untyped_message, span);
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
    let mut bindings: Vec<(crate::types::TypeParameterId, TypeId)> = Vec::new();
    for (index, arg_type) in arg_types.iter().enumerate() {
        let Some(arg_type) = arg_type else { continue };
        if let Some(param_type) = expected_param_type(&ctx.arena, &function_type.params, index) {
            infer_type_param_bindings(&ctx.arena, param_type, *arg_type, &mut bindings);
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
                "Argument type is not assignable to parameter type.",
                arg_expr.span(),
            );
        }
    }

    substitute_type_params(&mut ctx.arena, function_type.return_type, &bindings)
}

fn arity_message(required: usize, max: Option<usize>, got: usize) -> String {
    match max {
        Some(max) if max == required => {
            format!("Expected {required} argument(s), but got {got}.")
        }
        Some(max) => format!("Expected {required}-{max} argument(s), but got {got}."),
        None => format!("Expected at least {required} argument(s), but got {got}."),
    }
}
