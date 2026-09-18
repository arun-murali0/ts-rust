use oxc_ast::ast::Expression;
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;
use crate::types::Type;

use super::context::CheckContext;

mod binary;
mod calls;
mod core;
mod functions;
mod logical;
mod members;
mod objects;

pub(super) use members::infer_member_access_type;

pub(super) use crate::semantic::{
    expected_param_type, infer_type_param_bindings, substitute_type_params,
    collect_generic_param_constraints
};
use binary::infer_binary_expression_type;
use calls::{infer_call_expression_type, infer_new_expression_type};
pub(super) use core::resolve_identifier_type;
use functions::{infer_arrow_function_type, infer_function_expression_type};
use logical::{
    infer_as_expression_type, infer_conditional_expression_type, infer_logical_expression_type,
    infer_non_null_expression_type,
};
use members::{
    infer_chain_element_type, infer_computed_member_access_type,
    infer_member_access_type_with_optional,
};
use objects::{infer_array_expression_type, infer_object_expression_type};

// The single dispatch point for every expression kind this checker understands.
// Each arm delegates to a feature module (binary.rs, calls.rs, and so on); this
// function's own job is only to route, not to hold checking logic itself. An
// expression kind with no arm here falls through to the catch-all at the bottom,
// which reports it honestly as unsupported rather than guessing a type for it.
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

        // Outside any class method body, current_class_instance is None and this
        // is left as an Error sentinel rather than reported as an error itself;
        // whatever this expression's result feeds into will already be flagged
        // there if using this without a class context is genuinely a mistake.
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
            // The operand is still checked for its own errors, but typeof's
            // result is always the literal runtime string "string", "number", and
            // so on, so the operand's own inferred type is discarded here rather
            // than reused.
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
