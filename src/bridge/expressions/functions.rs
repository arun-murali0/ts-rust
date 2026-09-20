use oxc_ast::ast::{ArrowFunctionBody, Function};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;
use crate::type_annotation::{resolve_params_with_any_fallback, resolve_type_annotation};
use crate::types::{FunctionType, Type};

use super::super::context::CheckContext;
use super::super::statements::{bind_params, check_statement};
use super::infer_expression_type;

// Arrow functions lexically inherit `this` from their enclosing scope in real
// JavaScript, so current_class_instance is deliberately left untouched here,
// unlike infer_function_expression_type below, which resets it.
pub(super) fn infer_arrow_function_type(
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
                if !ctx.semantic().is_assignable(inferred, declared) {
                    ctx.error(
                        crate::diagnostic_messages::messages::return_type_mismatch(),
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

pub(super) fn infer_function_expression_type(
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

        // Unlike an arrow function, a plain function expression gets its own
        // `this` binding at call time rather than inheriting the enclosing one,
        // so any class-instance context from an outer method body must not leak
        // into this function's body.
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
