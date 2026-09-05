//! Pass 2 (expression half): infers an expression's TypeId, pushing
//! diagnostics for any mismatch found along the way.

use oxc_ast::ast::{ArrowFunctionBody, BinaryOperator, Expression, Function, IdentifierReference, ObjectPropertyKind, PropertyKey};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::TypeId;
use crate::type_annotation::{resolve_params_with_any_fallback, resolve_type_annotation};
use crate::types::{FunctionType, ObjectType, PropertyEntry, Type};

use super::context::CheckContext;
use super::narrow::resolve_symbol_id;
use super::statements::{bind_params, check_statement};

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

        Expression::ArrowFunctionExpression(arrow) => infer_arrow_function_type(arrow, scoping, ctx),

        Expression::FunctionExpression(func) => infer_function_expression_type(func, scoping, ctx),

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

fn infer_arrow_function_type(
    arrow: &oxc_ast::ast::ArrowFunctionExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let param_types = resolve_params_with_any_fallback(&arrow.params, &mut ctx.namespace, &mut ctx.arena);
    bind_params(&arrow.params.items, &param_types, ctx);

    let declared_return =
        arrow.return_type.as_ref().and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));

    let return_type = if let Some(body_expr) = arrow.body.as_expression() {
        let inferred = infer_expression_type(body_expr, scoping, ctx);
        match declared_return {
            Some(declared) => {
                if !crate::subtyping::is_subtype(&ctx.arena, inferred, declared) {
                    ctx.error(
                        "Return type does not match the function's declared return type.",
                        body_expr.span(),
                    );
                }
                declared
            }
            None => inferred,
        }
    } else {
        let ArrowFunctionBody::FunctionBody(body) = &arrow.body else {
            unreachable!("as_expression() returned None, so this must be the block-body variant")
        };
        let return_type = declared_return.unwrap_or_else(|| ctx.arena.any());
        let outer_return_type = ctx.current_return_type.replace(return_type);
        for body_stmt in &body.statements {
            check_statement(body_stmt, scoping, ctx);
        }
        ctx.current_return_type = outer_return_type;
        return_type
    };

    ctx.arena.alloc(Type::Function(FunctionType { params: param_types, return_type, is_untyped: false }))
}

fn infer_function_expression_type(func: &Function, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) -> TypeId {
    let param_types = resolve_params_with_any_fallback(&func.params, &mut ctx.namespace, &mut ctx.arena);
    bind_params(&func.params.items, &param_types, ctx);

    let declared_return =
        func.return_type.as_ref().and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));
    let return_type = declared_return.unwrap_or_else(|| ctx.arena.any());

    // A named function expression's own name (`const f = function named()
    // {}`) is only usable for recursion inside its own body in real JS.
    // That self-reference isn't registered here; a rare enough pattern
    // that it's an accepted, undocumented-elsewhere gap rather than
    // something worth the extra machinery right now.
    if let Some(body) = &func.body {
        let outer_return_type = ctx.current_return_type.replace(return_type);
        // Unlike an arrow function (which lexically inherits `this` from
        // its enclosing scope, so correctly leaves current_class_instance
        // untouched — see infer_arrow_function_type), a plain `function`
        // expression gets its own dynamic `this` binding in real JS,
        // independent of any lexically enclosing class. Without clearing
        // this, a function expression nested inside a class method would
        // incorrectly resolve `this` to that class's instance type, e.g.:
        //   class Counter {
        //     count: number = 0;
        //     scheduleIncrement() {
        //       const cb = function () { this.count++; }; // `this` is
        //       // NOT the Counter instance here at runtime — real tsc
        //       // flags this; ts-rust silently accepted it before this
        //       // fix, by leaking the enclosing class's instance type in.
        //     }
        //   }
        // `None` here means Expression::ThisExpression resolves to Error
        // instead (see its match arm above) — consistent with "we don't
        // know this function's `this` type" rather than guessing wrong.
        let outer_class_instance = ctx.current_class_instance.take();
        for body_stmt in &body.statements {
            check_statement(body_stmt, scoping, ctx);
        }
        ctx.current_class_instance = outer_class_instance;
        ctx.current_return_type = outer_return_type;
    }

    ctx.arena.alloc(Type::Function(FunctionType { params: param_types, return_type, is_untyped: false }))
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
    let untyped_message =
        format!("'{callee_name}' has an untyped parameter, so ts-rust can't check this call's arity yet.");
    check_callable(callee_type, &call.arguments, call.span(), &not_callable, &untyped_message, scoping, ctx)
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

    let not_callable = format!("'{}' is not a constructor.", callee_ident.name);
    let untyped_message = format!(
        "'{}' has a constructor with an untyped parameter, so ts-rust can't check arity for `new {}(...)` yet.",
        callee_ident.name, callee_ident.name
    );
    check_callable(callee_type, &new_expr.arguments, new_expr.span(), &not_callable, &untyped_message, scoping, ctx)
}

/// Shared by `f(...)`, `obj.method(...)`, and `new C(...)`: all three are
/// "resolve a function type, check arity, check each argument, return
/// the function's return type." A `new` expression's "return type" here
/// is the class's instance type, since `declare.rs` registers a class's
/// constructor as a `Type::Function` whose return type already is the
/// instance type.
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
        // Type::Any is deliberately excluded from the "not callable"
        // error, alongside the existing Type::Error exclusion: calling a
        // value of type `any` is always legal in real TS/JS — `any`
        // erases all checking, including whether the thing is callable
        // at all. (Mirrors infer_member_access_type's identical
        // Any | Error exclusion a few lines above for the same reason.)
        if !matches!(ctx.arena.get(callee_type), Type::Any | Type::Error) {
            ctx.error(not_callable_message, span);
            return ctx.arena.error();
        }
        // Any/Error: the call itself can't be arity- or type-checked,
        // but each argument is still worth checking on its own terms —
        // `anyFn(1 + "x")` should still flag the `1 + "x"` mismatch
        // inside the argument, same reasoning as the is_untyped branch
        // below.
        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return ctx.arena.error();
    };

    // Set by declare.rs for a class whose constructor has an untyped
    // parameter, or by namespace.rs's resolve_class for a method with
    // one (see resolve_class's method branch): `params` is deliberately
    // empty and not to be trusted for arity checking in either case.
    // Every argument is still worth checking on its own terms, though —
    // this only skips *arity* checking, e.g. `new Foo(1 + "x")` or
    // `obj.method(1 + "x")` should still flag the `1 + "x"` mismatch
    // even though how many arguments were expected is unknown.
    if function_type.is_untyped {
        ctx.warning(untyped_message, span);
        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return function_type.return_type;
    }

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
