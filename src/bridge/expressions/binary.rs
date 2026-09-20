use oxc_ast::ast::BinaryOperator;
use oxc_span::Span;

use crate::arena::TypeId;

use super::super::context::CheckContext;

pub(super) fn infer_binary_expression_type(
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

        // JavaScript's + is overloaded: string concatenation if either side could
        // be a string, otherwise numeric addition if both sides could be numbers.
        // The string check is tried first since that matches runtime semantics,
        // where "1" + 1 is string concatenation, not addition.
        BinaryOperator::Addition => {
            let is_string = ctx.semantic().is_assignable(left, ctx.arena.string())
                || ctx.semantic().is_assignable(right, ctx.arena.string());
            let is_number = ctx.semantic().is_assignable(left, ctx.arena.number())
                && ctx.semantic().is_assignable(right, ctx.arena.number());
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
            if ctx.semantic().is_assignable(left, ctx.arena.number())
                && ctx.semantic().is_assignable(right, ctx.arena.number())
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

fn push_binary_op_mismatch(ctx: &mut CheckContext<'_, '_>, span: Span, operator: &str) {
    ctx.error(
        crate::diagnostic_messages::messages::binary_operand_type_mismatch(operator),
        span,
    );
}
