use oxc_ast::ast::{
    ArrowFunctionBody, BinaryOperator, Expression, Function, IdentifierReference, LogicalOperator,
    ObjectPropertyKind, PropertyKey,
};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::{TypeArena, TypeId};
use crate::type_annotation::{
    resolve_params_with_any_fallback, resolve_ts_type, resolve_type_annotation,
};
use crate::types::{FunctionType, ObjectType, PropertyEntry, Type};

use super::context::CheckContext;
use super::narrow::{
    narrow_condition, narrow_to_falsy, narrow_to_non_nullish, narrow_to_truthy, resolve_symbol_id,
};
use super::statements::{bind_params, check_statement};

#[tracing::instrument(skip_all)]
pub fn infer_expression_type(
    expr: &Expression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    match expr {
        Expression::NumericLiteral(n) => ctx.arena.alloc(Type::NumberLiteral(n.value)),
        Expression::StringLiteral(s) => ctx.arena.alloc(Type::StringLiteral(s.value.to_string())),
        Expression::BooleanLiteral(b) => ctx.arena.alloc(Type::BooleanLiteral(b.value)),
        Expression::NullLiteral(_) => ctx.arena.null(),

        Expression::Identifier(ident) => resolve_identifier_type(ident, scoping, ctx),

        Expression::ThisExpression(_) => ctx
            .current_class_instance
            .unwrap_or_else(|| ctx.arena.error()),

        Expression::BinaryExpression(bin) => {
            let left = infer_expression_type(&bin.left, scoping, ctx);
            let right = infer_expression_type(&bin.right, scoping, ctx);
            infer_binary_expression_type(bin.operator, left, right, bin.span(), ctx)
        }

        Expression::CallExpression(call) => infer_call_expression_type(call, scoping, ctx),

        Expression::NewExpression(new_expr) => infer_new_expression_type(new_expr, scoping, ctx),

        Expression::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_member_access_type_with_optional(
                object_type,
                &member.property.name,
                member.optional,
                member.span(),
                ctx,
            )
        }

        Expression::ComputedMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_computed_member_access_type(
                object_type,
                &member.expression,
                member.optional,
                member.span(),
                scoping,
                ctx,
            )
        }

        Expression::ChainExpression(chain) => {
            infer_chain_element_type(&chain.expression, scoping, ctx)
        }

        Expression::ObjectExpression(object) => infer_object_expression_type(object, scoping, ctx),

        Expression::ArrayExpression(array) => infer_array_expression_type(array, scoping, ctx),

        Expression::ArrowFunctionExpression(arrow) => {
            infer_arrow_function_type(arrow, scoping, ctx)
        }

        Expression::FunctionExpression(func) => infer_function_expression_type(func, scoping, ctx),

        Expression::LogicalExpression(logical) => {
            infer_logical_expression_type(logical, scoping, ctx)
        }

        Expression::ConditionalExpression(conditional) => {
            infer_conditional_expression_type(conditional, scoping, ctx)
        }

        Expression::TSAsExpression(as_expr) => infer_as_expression_type(as_expr, scoping, ctx),

        Expression::TSNonNullExpression(non_null) => {
            infer_non_null_expression_type(non_null, scoping, ctx)
        }

        Expression::UnaryExpression(unary)
            if unary.operator == oxc_ast::ast::UnaryOperator::Typeof =>
        {
            infer_expression_type(&unary.argument, scoping, ctx);
            ctx.arena.string()
        }

        _ => {
            ctx.warning(
                "This expression kind is not yet checked by ts-rust.",
                expr.span(),
            );
            ctx.arena.error()
        }
    }
}

fn resolve_identifier_type(
    ident: &IdentifierReference,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Some(symbol_id) = resolve_symbol_id(ident, scoping) else {
        ctx.error(format!("Cannot find name '{}'.", ident.name), ident.span());
        return ctx.arena.error();
    };

    if let Some(narrowed) = ctx.narrow.get(symbol_id) {
        return narrowed;
    }

    match ctx.symbols.get(symbol_id) {
        Some(type_id) => type_id,
        None => {
            tracing::warn!(
                name = %ident.name,
                "resolved symbol has no registered type, falling back to the error sentinel"
            );
            ctx.arena.error()
        }
    }
}

pub(super) fn infer_member_access_type(
    object_type: TypeId,
    property_name: &str,
    span: Span,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Type::Object(object) = ctx.arena.get(object_type) else {
        if !matches!(ctx.arena.get(object_type), Type::Any | Type::Error) {
            ctx.error(
                format!("Property '{property_name}' does not exist on this type."),
                span,
            );
        }
        return ctx.arena.error();
    };

    match object.properties.iter().find(|p| p.name == property_name) {
        Some(property) => property.type_id,
        None => {
            ctx.error(
                format!("Property '{property_name}' does not exist on this type."),
                span,
            );
            ctx.arena.error()
        }
    }
}

fn infer_member_access_type_with_optional(
    object_type: TypeId,
    property_name: &str,
    optional: bool,
    span: Span,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    if !optional {
        return infer_member_access_type(object_type, property_name, span, ctx);
    }

    let non_nullish = narrow_to_non_nullish(&mut ctx.arena, object_type);
    let property_type = infer_member_access_type(non_nullish, property_name, span, ctx);
    ctx.arena
        .alloc_union(vec![property_type, ctx.arena.undefined()])
}

fn infer_computed_member_access_type(
    object_type: TypeId,
    key_expr: &Expression,
    optional: bool,
    span: Span,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    infer_expression_type(key_expr, scoping, ctx);

    if let &Type::Array(element_type) = ctx.arena.get(object_type) {
        return match key_expr {
            Expression::NumericLiteral(_) => element_type,
            _ => ctx
                .arena
                .alloc_union(vec![element_type, ctx.arena.undefined()]),
        };
    }

    let Expression::StringLiteral(key) = key_expr else {
        ctx.warning(
            "Computed member access with a non-literal key is not yet checked by ts-rust.",
            span,
        );
        return ctx.arena.error();
    };

    infer_member_access_type_with_optional(object_type, &key.value, optional, span, ctx)
}

fn infer_chain_element_type(
    element: &oxc_ast::ast::ChainElement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    use oxc_ast::ast::ChainElement;

    match element {
        ChainElement::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_member_access_type_with_optional(
                object_type,
                &member.property.name,
                member.optional,
                member.span(),
                ctx,
            )
        }
        ChainElement::ComputedMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_computed_member_access_type(
                object_type,
                &member.expression,
                member.optional,
                member.span(),
                scoping,
                ctx,
            )
        }
        _ => {
            ctx.warning(
                "This kind of optional-chain link is not yet checked by ts-rust.",
                element.span(),
            );
            ctx.arena.error()
        }
    }
}

fn infer_arrow_function_type(
    arrow: &oxc_ast::ast::ArrowFunctionExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let param_types =
        resolve_params_with_any_fallback(&arrow.params, &mut ctx.namespace, &mut ctx.arena);
    bind_params(&arrow.params, &param_types, scoping, ctx);

    let declared_return = arrow
        .return_type
        .as_ref()
        .and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));

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
            return ctx.arena.error();
        };
        let return_type = declared_return.unwrap_or_else(|| ctx.arena.any());
        let outer_return_type = ctx.current_return_type.replace(return_type);
        for body_stmt in &body.statements {
            check_statement(body_stmt, scoping, ctx);
        }
        ctx.current_return_type = outer_return_type;
        return_type
    };

    ctx.arena.alloc(Type::Function(FunctionType {
        params: param_types,
        return_type,
        is_untyped: false,
    }))
}

fn infer_function_expression_type(
    func: &Function,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let param_types =
        resolve_params_with_any_fallback(&func.params, &mut ctx.namespace, &mut ctx.arena);
    bind_params(&func.params, &param_types, scoping, ctx);

    let declared_return = func
        .return_type
        .as_ref()
        .and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));
    let return_type = declared_return.unwrap_or_else(|| ctx.arena.any());

    if let Some(body) = &func.body {
        let outer_return_type = ctx.current_return_type.replace(return_type);

        let outer_class_instance = ctx.current_class_instance.take();
        for body_stmt in &body.statements {
            check_statement(body_stmt, scoping, ctx);
        }
        ctx.current_class_instance = outer_class_instance;
        ctx.current_return_type = outer_return_type;
    }

    ctx.arena.alloc(Type::Function(FunctionType {
        params: param_types,
        return_type,
        is_untyped: false,
    }))
}

fn infer_logical_expression_type(
    logical: &oxc_ast::ast::LogicalExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    match logical.operator {
        LogicalOperator::And => {
            let left_type = infer_expression_type(&logical.left, scoping, ctx);

            let (truthy_overrides, _) = narrow_condition(&logical.left, scoping, ctx);
            let outer_narrow = ctx.narrow.clone();
            ctx.narrow.extend(truthy_overrides);
            let right_type = infer_expression_type(&logical.right, scoping, ctx);
            ctx.narrow = outer_narrow;

            let left_falsy = narrow_to_falsy(&mut ctx.arena, left_type);
            ctx.arena.alloc_union(vec![left_falsy, right_type])
        }

        LogicalOperator::Or => {
            let left_type = infer_expression_type(&logical.left, scoping, ctx);

            let (_, falsy_overrides) = narrow_condition(&logical.left, scoping, ctx);
            let outer_narrow = ctx.narrow.clone();
            ctx.narrow.extend(falsy_overrides);
            let right_type = infer_expression_type(&logical.right, scoping, ctx);
            ctx.narrow = outer_narrow;

            let left_truthy = narrow_to_truthy(&mut ctx.arena, left_type);
            ctx.arena.alloc_union(vec![left_truthy, right_type])
        }

        LogicalOperator::Coalesce => {
            let left_type = infer_expression_type(&logical.left, scoping, ctx);
            let right_type = infer_expression_type(&logical.right, scoping, ctx);

            let left_non_nullish = narrow_to_non_nullish(&mut ctx.arena, left_type);
            ctx.arena.alloc_union(vec![left_non_nullish, right_type])
        }
    }
}

fn infer_conditional_expression_type(
    conditional: &oxc_ast::ast::ConditionalExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    infer_expression_type(&conditional.test, scoping, ctx);
    let (true_overrides, false_overrides) = narrow_condition(&conditional.test, scoping, ctx);
    let outer_narrow = ctx.narrow.clone();

    ctx.narrow.extend(true_overrides);
    let consequent_type = infer_expression_type(&conditional.consequent, scoping, ctx);
    ctx.narrow = outer_narrow.clone();

    ctx.narrow.extend(false_overrides);
    let alternate_type = infer_expression_type(&conditional.alternate, scoping, ctx);
    ctx.narrow = outer_narrow;

    ctx.arena.alloc_union(vec![consequent_type, alternate_type])
}

fn infer_as_expression_type(
    as_expr: &oxc_ast::ast::TSAsExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    infer_expression_type(&as_expr.expression, scoping, ctx);
    resolve_ts_type(&as_expr.type_annotation, &mut ctx.namespace, &mut ctx.arena)
        .unwrap_or_else(|| ctx.arena.error())
}

fn infer_non_null_expression_type(
    non_null: &oxc_ast::ast::TSNonNullExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let inner = infer_expression_type(&non_null.expression, scoping, ctx);
    narrow_to_non_nullish(&mut ctx.arena, inner)
}

fn infer_object_expression_type(
    object: &oxc_ast::ast::ObjectExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut properties = Vec::with_capacity(object.properties.len());

    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            continue;
        };
        let type_id = infer_expression_type(&property.value, scoping, ctx);
        let type_id = crate::types::widen(&ctx.arena, type_id);
        properties.push(PropertyEntry {
            name: key.name.to_string(),
            type_id,
            optional: false,
        });
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
            continue;
        };
        let type_id = infer_expression_type(expr, scoping, ctx);

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

fn infer_new_expression_type(
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

    let arg_types: Vec<Option<TypeId>> = arguments
        .iter()
        .map(|arg| {
            arg.as_expression()
                .map(|expr| infer_expression_type(expr, scoping, ctx))
        })
        .collect();

    let mut bindings: Vec<(String, TypeId)> = Vec::new();
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
        if !crate::subtyping::is_subtype(&ctx.arena, arg_type, expected) {
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

fn infer_type_param_bindings(
    arena: &TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(String, TypeId)>,
) {
    match arena.get(param_type) {
        Type::GenericParameter(name) => {
            if !bindings.iter().any(|(bound, _)| bound == name) {
                bindings.push((name.clone(), crate::types::widen(arena, arg_type)));
            }
        }
        Type::Array(param_element) => {
            if let Type::Array(arg_element) = arena.get(arg_type) {
                infer_type_param_bindings(arena, *param_element, *arg_element, bindings);
            }
        }
        Type::Object(param_object) => {
            if let Type::Object(arg_object) = arena.get(arg_type) {
                for param_prop in &param_object.properties {
                    if let Some(arg_prop) = arg_object
                        .properties
                        .iter()
                        .find(|p| p.name == param_prop.name)
                    {
                        infer_type_param_bindings(
                            arena,
                            param_prop.type_id,
                            arg_prop.type_id,
                            bindings,
                        );
                    }
                }
            }
        }
        Type::Function(param_fn) => {
            if let Type::Function(arg_fn) = arena.get(arg_type) {
                for (p, a) in param_fn.params.iter().zip(&arg_fn.params) {
                    infer_type_param_bindings(arena, p.type_id, a.type_id, bindings);
                }
                infer_type_param_bindings(
                    arena,
                    param_fn.return_type,
                    arg_fn.return_type,
                    bindings,
                );
            }
        }
        _ => {}
    }
}

fn substitute_type_params(
    arena: &mut TypeArena,
    type_id: TypeId,
    bindings: &[(String, TypeId)],
) -> TypeId {
    if bindings.is_empty() || !contains_type_param(arena, type_id) {
        return type_id;
    }

    match arena.get(type_id).clone() {
        Type::GenericParameter(name) => bindings
            .iter()
            .find(|(bound, _)| *bound == name)
            .map(|(_, resolved)| *resolved)
            .unwrap_or_else(|| arena.unknown()),
        Type::Array(element) => {
            let substituted = substitute_type_params(arena, element, bindings);
            arena.alloc(Type::Array(substituted))
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|p| crate::types::Param {
                    type_id: substitute_type_params(arena, p.type_id, bindings),
                    optional: p.optional,
                    rest: p.rest,
                })
                .collect();
            let return_type = substitute_type_params(arena, function.return_type, bindings);
            arena.alloc(Type::Function(FunctionType {
                params,
                return_type,
                is_untyped: function.is_untyped,
            }))
        }
        Type::Object(object) => {
            let properties = object
                .properties
                .iter()
                .map(|p| PropertyEntry {
                    name: p.name.clone(),
                    type_id: substitute_type_params(arena, p.type_id, bindings),
                    optional: p.optional,
                })
                .collect();
            arena.alloc(Type::Object(ObjectType { properties }))
        }
        Type::Union(members) => {
            let substituted = members
                .iter()
                .map(|&m| substitute_type_params(arena, m, bindings))
                .collect();
            arena.alloc_union(substituted)
        }
        _ => type_id,
    }
}

fn contains_type_param(arena: &TypeArena, type_id: TypeId) -> bool {
    match arena.get(type_id) {
        Type::GenericParameter(_) => true,
        Type::Array(element) => contains_type_param(arena, *element),
        Type::Function(f) => {
            f.params
                .iter()
                .any(|p| contains_type_param(arena, p.type_id))
                || contains_type_param(arena, f.return_type)
        }
        Type::Object(o) => o
            .properties
            .iter()
            .any(|p| contains_type_param(arena, p.type_id)),
        Type::Union(members) => members.iter().any(|&m| contains_type_param(arena, m)),
        _ => false,
    }
}

fn expected_param_type(
    arena: &crate::arena::TypeArena,
    params: &[crate::types::Param],
    index: usize,
) -> Option<TypeId> {
    let param = match params.get(index) {
        Some(param) => param,
        None => params.last().filter(|p| p.rest)?,
    };
    Some(if param.rest {
        match arena.get(param.type_id) {
            Type::Array(element) => *element,
            _ => param.type_id,
        }
    } else {
        param.type_id
    })
}

fn push_binary_op_mismatch(ctx: &mut CheckContext<'_, '_>, span: Span, operator: &str) {
    ctx.error(
        format!("Operator '{operator}' cannot be applied to these types."),
        span,
    );
}
