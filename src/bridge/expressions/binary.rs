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
            // Read before calling ctx.semantic(): SemanticQueries now borrows
            // ctx.arena and ctx.subtype_cache together for as long as it lives,
            // so ctx.arena can't be reached again (even just to read a fixed
            // primitive id) while a SemanticQueries value from an earlier call
            // in this same expression is still alive.
            let any_ty = ctx.arena.any();
            if left == any_ty || right == any_ty {
                // Checked before the string/number rules below, not after: `any`
                // is a subtype of every type, string included, so `is_string`
                // would otherwise already be true for `any + 1` and wrongly win,
                // reporting `string` instead of the `any` real TypeScript infers.
                return ctx.arena.any();
            }
            let string_ty = ctx.arena.string();
            let number_ty = ctx.arena.number();
            let is_string = ctx.semantic().is_assignable(left, string_ty)
                || ctx.semantic().is_assignable(right, string_ty);
            let is_number = ctx.semantic().is_assignable(left, number_ty)
                && ctx.semantic().is_assignable(right, number_ty);
            if is_string {
                ctx.arena.string()
            } else if is_number {
                ctx.arena.number()
            } else {
                push_binary_op_mismatch(ctx, span, "+", left, right);
                ctx.arena.error()
            }
        }

        BinaryOperator::Subtraction
        | BinaryOperator::Multiplication
        | BinaryOperator::Division
        | BinaryOperator::Remainder
        | BinaryOperator::Exponential => {
            // No `any` special-case here, unlike Addition above: verified
            // against real tsc (see
            // tests/fixtures/misc-fixes/any_minus_number_infers_number_not_any.ts),
            // `any - 1` actually infers `number`, not `any`. Only `+` propagates
            // `any` in real TypeScript, because of its ambiguity between the
            // string and number overloads when one side is `any`; every other
            // arithmetic operator only has a (number, number) => number
            // overload, and `any` simply satisfies that parameter type the same
            // way it satisfies any other, giving back the overload's own
            // `number` result rather than `any`.
            let number_ty = ctx.arena.number();
            if ctx.semantic().is_assignable(left, number_ty)
                && ctx.semantic().is_assignable(right, number_ty)
            {
                ctx.arena.number()
            } else {
                push_binary_op_mismatch(ctx, span, operator.as_str(), left, right);
                ctx.arena.error()
            }
        }

        _ => ctx.arena.error(),
    }
}

// Both mismatch sites above (`+` and the arithmetic group) need the same
// diagnostic shape and differ only in which operator string to name, so it's
// pulled out here rather than duplicated -- if the message format changes,
// there is one call to update instead of two that have to be kept in sync.
fn push_binary_op_mismatch(
    ctx: &mut CheckContext<'_, '_>,
    span: Span,
    operator: &str,
    left: TypeId,
    right: TypeId,
) {
    ctx.error(
        crate::diagnostic_messages::messages::binary_operand_type_mismatch(
            &ctx.arena, operator, left, right,
        ),
        span,
    );
}
