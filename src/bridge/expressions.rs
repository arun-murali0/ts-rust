//! Pass 2 (expression half): infers an expression's TypeId, pushing
//! diagnostics for any mismatch found along the way.

use oxc_ast::ast::{BinaryOperator, Expression, IdentifierReference, ObjectPropertyKind, PropertyKey};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::TypeId;
use crate::types::{ObjectType, PropertyEntry, Type};

use super::context::CheckContext;
use super::narrow::resolve_symbol_id;

/// Always returns a TypeId. On failure this is `ctx.arena.error()`, not
/// `ctx.arena.unknown()`, so a caller checking the result against a
/// declared type won't raise a second, misleading diagnostic on top of the
/// first.
#[tracing::instrument(skip_all)]
pub fn infer_expression_type(expr: &Expression, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) -> TypeId {
    match expr {
        Expression::NumericLiteral(n) => ctx.arena.alloc(Type::NumberLiteral(n.value)),
        Expression::StringLiteral(s) => ctx.arena.alloc(Type::StringLiteral(s.value.to_string())),
        Expression::BooleanLiteral(b) => ctx.arena.alloc(Type::BooleanLiteral(b.value)),
        Expression::NullLiteral(_) => ctx.arena.null(),

        Expression::Identifier(ident) => resolve_identifier_type(ident, scoping, ctx),

        // Resolves to the instance type of the class whose constructor or
        // method body is currently being checked (bridge/statements.rs's
        // ClassDeclaration handling sets this before walking a body).
        // Outside any class body there's nothing meaningful to type it
        // as yet, so it falls back to the error sentinel.
        Expression::ThisExpression(_) => ctx.current_class_instance.unwrap_or_else(|| ctx.arena.error()),

        Expression::BinaryExpression(bin) => {
            let left = infer_expression_type(&bin.left, scoping, ctx);
            let right = infer_expression_type(&bin.right, scoping, ctx);
            infer_binary_expression_type(bin.operator, left, right, bin.span(), ctx)
        }

        Expression::CallExpression(call) => infer_call_expression_type(call, scoping, ctx),

        Expression::NewExpression(new_expr) => infer_new_expression_type(new_expr, scoping, ctx),

        Expression::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_member_access_type(object_type, &member.property.name, member.span(), ctx)
        }

        Expression::ObjectExpression(object) => infer_object_expression_type(object, scoping, ctx),

        Expression::ArrayExpression(array) => infer_array_expression_type(array, scoping, ctx),

        // `typeof x` always evaluates to a string at runtime, independent
        // of whatever narrowing decision the containing `if` makes about
        // it. `void`, `!`, unary `+`/`-`, `~`, and `delete` aren't typed
        // yet.
        Expression::UnaryExpression(unary) if unary.operator == oxc_ast::ast::UnaryOperator::Typeof => {
            infer_expression_type(&unary.argument, scoping, ctx);
            ctx.arena.string()
        }

        _ => {
            ctx.warning("This expression kind is not yet checked by ts-rust.", expr.span());
            ctx.arena.error()
        }
    }
}

fn resolve_identifier_type(ident: &IdentifierReference, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) -> TypeId {
    let Some(symbol_id) = resolve_symbol_id(ident, scoping) else {
        ctx.error(format!("Cannot find name '{}'.", ident.name), ident.span());
        return ctx.arena.error();
    };

    // A narrowed type, if one is currently in effect (inside an `if`
    // branch that narrowed this binding), always wins over its declared
    // type — that's the entire point of narrowing.
    if let Some(narrowed) = ctx.narrow.get(symbol_id) {
        return narrowed;
    }

    match ctx.symbols.get(symbol_id) {
        Some(type_id) => type_id,
        None => {
            // oxc's own binder resolved this reference to a real
            // declaration, so this is not an unresolved name. It means a
            // resolved symbol reached checking without ever being
            // registered: an unannotated variable, a destructured
            // parameter, or a declare-time bug. Logged at warn level
            // rather than silently swallowed — run with RUST_LOG=warn to
            // see it.
            tracing::warn!(
                name = %ident.name,
                "resolved symbol has no registered type, falling back to the error sentinel"
            );
            ctx.arena.error()
        }
    }
}

fn infer_member_access_type(object_type: TypeId, property_name: &str, span: Span, ctx: &mut CheckContext<'_, '_>) -> TypeId {
    let Type::Object(object) = ctx.arena.get(object_type) else {
        if !matches!(ctx.arena.get(object_type), Type::Any | Type::Error) {
            ctx.error(format!("Property '{property_name}' does not exist on this type."), span);
        }
        return ctx.arena.error();
    };

    match object.properties.iter().find(|p| p.name == property_name) {
        Some(property) => property.type_id,
        None => {
            ctx.error(format!("Property '{property_name}' does not exist on this type."), span);
            ctx.arena.error()
        }
    }
}

fn infer_object_expression_type(
    object: &oxc_ast::ast::ObjectExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut properties = Vec::with_capacity(object.properties.len());

    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            // Spread properties aren't merged into the resulting type yet.
            continue;
        };
        let PropertyKey::StaticIdentifier(key) = &property.key else { continue };
        let type_id = infer_expression_type(&property.value, scoping, ctx);
        let type_id = crate::types::widen(&ctx.arena, type_id);
        properties.push(PropertyEntry { name: key.name.to_string(), type_id, optional: false });
    }

    properties.sort_by(|a, b| a.name.cmp(&b.name));
    ctx.arena.alloc(Type::Object(ObjectType { properties }))
}

fn infer_array_expression_type(
    array: &oxc_ast::ast::ArrayExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut element_types = Vec::with_capacity(array.elements.len());

    for element in &array.elements {
        let Some(expr) = element.as_expression() else {
            // Spread elements and elisions aren't handled yet.
            continue;
        };
        let type_id = infer_expression_type(expr, scoping, ctx);
        // Widening also collapses e.g. three separately-allocated `1`
        // literals to the same TypeId, so the dedup below actually works.
        let type_id = crate::types::widen(&ctx.arena, type_id);
        if !element_types.contains(&type_id) {
            element_types.push(type_id);
        }
    }

    let element_type = match element_types.len() {
        0 => ctx.arena.unknown(),
        1 => element_types[0],
        _ => ctx.arena.alloc_union(element_types),
    };

    ctx.arena.alloc(Type::Array(element_type))
}

fn infer_binary_expression_type(
    operator: BinaryOperator,
    left: TypeId,
    right: TypeId,
    span: Span,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    match operator {
        BinaryOperator::Equality
        | BinaryOperator::Inequality
        | BinaryOperator::StrictEquality
        | BinaryOperator::StrictInequality
        | BinaryOperator::LessThan
        | BinaryOperator::LessEqualThan
        | BinaryOperator::GreaterThan
        | BinaryOperator::GreaterEqualThan => ctx.arena.boolean(),

        BinaryOperator::Addition => {
            let is_string = crate::subtyping::is_subtype(&ctx.arena, left, ctx.arena.string())
                || crate::subtyping::is_subtype(&ctx.arena, right, ctx.arena.string());
            let is_number = crate::subtyping::is_subtype(&ctx.arena, left, ctx.arena.number())
                && crate::subtyping::is_subtype(&ctx.arena, right, ctx.arena.number());
            if is_string {
                ctx.arena.string()
            } else if is_number {
                ctx.arena.number()
            } else if left == ctx.arena.any() || right == ctx.arena.any() {
                ctx.arena.any()
            } else {
                push_binary_op_mismatch(ctx, span, "+");
                ctx.arena.error()
            }
        }

        BinaryOperator::Subtraction
        | BinaryOperator::Multiplication
        | BinaryOperator::Division
        | BinaryOperator::Remainder
        | BinaryOperator::Exponential => {
            if crate::subtyping::is_subtype(&ctx.arena, left, ctx.arena.number())
                && crate::subtyping::is_subtype(&ctx.arena, right, ctx.arena.number())
            {
                ctx.arena.number()
            } else {
                push_binary_op_mismatch(ctx, span, operator.as_str());
                ctx.arena.error()
            }
        }

        _ => ctx.arena.error(),
    }
}

fn infer_call_expression_type(
    call: &oxc_ast::ast::CallExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let (callee_type, callee_name) = match &call.callee {
        Expression::Identifier(ident) => (resolve_identifier_type(ident, scoping, ctx), ident.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            let property_type = infer_member_access_type(object_type, &member.property.name, member.span(), ctx);
            (property_type, member.property.name.to_string())
        }
        _ => {
            ctx.warning("This kind of call expression is not yet checked by ts-rust.", call.span());
            return ctx.arena.error();
        }
    };

    let not_callable = format!("'{callee_name}' is not callable.");
    check_callable(callee_type, &call.arguments, call.span(), &not_callable, scoping, ctx)
}

fn infer_new_expression_type(
    new_expr: &oxc_ast::ast::NewExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Expression::Identifier(callee_ident) = &new_expr.callee else {
        ctx.warning("`new` on anything other than a plain name is not yet checked by ts-rust.", new_expr.span());
        return ctx.arena.error();
    };

    let callee_type = resolve_identifier_type(callee_ident, scoping, ctx);

    // declare.rs marks a class's constructor Function type `is_untyped`
    // when a constructor exists but has a parameter without a type
    // annotation, distinct from a class with no constructor at all
    // (params: vec![], is_untyped: false, which is correctly
    // arity-checked as zero-arg). Reading that flag here, rather than
    // re-deriving it from the AST during Pass 2, keeps this a single
    // lookup instead of duplicating declare.rs's own resolution work.
    if let Type::Function(function_type) = ctx.arena.get(callee_type).clone() {
        if function_type.is_untyped {
            ctx.warning(
                format!(
                    "'{}' has a constructor with an untyped parameter, so ts-rust can't check \
                     arity for `new {}(...)` yet.",
                    callee_ident.name, callee_ident.name
                ),
                new_expr.span(),
            );
            // Still worth checking each argument on its own terms (catches
            // e.g. `new Foo(1 + "x")`) even though arity itself is skipped.
            for arg in &new_expr.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    infer_expression_type(arg_expr, scoping, ctx);
                }
            }
            return function_type.return_type;
        }
    }

    let not_callable = format!("'{}' is not a constructor.", callee_ident.name);
    check_callable(callee_type, &new_expr.arguments, new_expr.span(), &not_callable, scoping, ctx)
}

/// Shared by `f(...)` and `new C(...)`: both are "resolve a function type,
/// check arity, check each argument, return the function's return type."
/// A `new` expression's "return type" here is the class's instance type,
/// since `declare.rs` registers a class's constructor as a `Type::Function`
/// whose return type already is the instance type.
fn check_callable(
    callee_type: TypeId,
    arguments: &[oxc_ast::ast::Argument],
    span: Span,
    not_callable_message: &str,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Type::Function(function_type) = ctx.arena.get(callee_type).clone() else {
        if !matches!(ctx.arena.get(callee_type), Type::Error) {
            ctx.error(not_callable_message, span);
        }
        return ctx.arena.error();
    };

    if arguments.len() != function_type.params.len() {
        ctx.error(format!("Expected {} argument(s), but got {}.", function_type.params.len(), arguments.len()), span);
        return function_type.return_type;
    }

    for (arg, &param_type) in arguments.iter().zip(&function_type.params) {
        let Some(arg_expr) = arg.as_expression() else { continue };
        let arg_type = infer_expression_type(arg_expr, scoping, ctx);
        if !crate::subtyping::is_subtype(&ctx.arena, arg_type, param_type) {
            ctx.error("Argument type is not assignable to parameter type.", arg_expr.span());
        }
    }

    function_type.return_type
}

fn push_binary_op_mismatch(ctx: &mut CheckContext<'_, '_>, span: Span, operator: &str) {
    ctx.error(format!("Operator '{operator}' cannot be applied to these types."), span);
}
