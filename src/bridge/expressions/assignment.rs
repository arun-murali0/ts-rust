use oxc_ast::ast::{AssignmentExpression, AssignmentOperator, AssignmentTarget, BinaryOperator};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;

use super::super::context::CheckContext;
use super::binary::infer_binary_expression_type;
use super::core::resolve_identifier_type;
use super::infer_expression_type;
use super::members::{infer_computed_member_access_type, infer_member_access_type};

// `x = y` and `x op= y`. The right-hand side is always inferred (and so
// always gets whatever errors it has on its own), regardless of whether the
// target itself can be resolved.
pub(super) fn infer_assignment_expression_type(
    assign: &AssignmentExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let right_type = infer_expression_type(&assign.right, scoping, ctx);

    let Some(target_type) = resolve_assignment_target_type(&assign.left, scoping, ctx) else {
        // A destructuring target (`[a, b] = arr`, `({x} = obj)`), or one
        // wrapped in `as`/`!`/a type assertion -- not modeled. Skips the
        // assignability check rather than guessing; the right-hand side's own
        // errors above still stand.
        return right_type;
    };

    match assign.operator {
        AssignmentOperator::Assign => {
            if !ctx.semantic().is_assignable(right_type, target_type) {
                ctx.error(
                    crate::diagnostic_messages::messages::declared_type_mismatch(
                        &ctx.arena, right_type, target_type,
                    ),
                    assign.span(),
                );
            }
            right_type
        }
        // `x op= y` is `x = x op y`: whatever the operator's own rule is,
        // applied with the target's current type on the left. That reuses
        // infer_binary_expression_type's own mismatch reporting (e.g. `x -=
        // "a"` reports the same way `x - "a"` would). Not checked here, and a
        // narrower gap than tsc: whether the *result* is itself assignable
        // back to the target's type (`x: "a" | "b"; x += "c";` is invalid in
        // tsc since the result widens to plain string) -- left for later.
        operator => match compound_assignment_operator(operator) {
            Some(binary_operator) => infer_binary_expression_type(
                binary_operator,
                target_type,
                right_type,
                assign.span(),
                ctx,
            ),
            // Logical assignment (&&=, ||=, ??=): short-circuiting, so not a
            // plain binary op the way the arithmetic ones are. Not modeled.
            None => right_type,
        },
    }
}

fn resolve_assignment_target_type(
    target: &AssignmentTarget,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> Option<TypeId> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            Some(resolve_identifier_type(ident, scoping, ctx))
        }
        AssignmentTarget::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            Some(infer_member_access_type(
                object_type,
                &member.property.name,
                member.span(),
                ctx,
            ))
        }
        AssignmentTarget::ComputedMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            Some(infer_computed_member_access_type(
                object_type,
                &member.expression,
                member.optional,
                member.span(),
                scoping,
                ctx,
            ))
        }
        _ => None,
    }
}

fn compound_assignment_operator(operator: AssignmentOperator) -> Option<BinaryOperator> {
    Some(match operator {
        AssignmentOperator::Addition => BinaryOperator::Addition,
        AssignmentOperator::Subtraction => BinaryOperator::Subtraction,
        AssignmentOperator::Multiplication => BinaryOperator::Multiplication,
        AssignmentOperator::Division => BinaryOperator::Division,
        AssignmentOperator::Remainder => BinaryOperator::Remainder,
        AssignmentOperator::Exponential => BinaryOperator::Exponential,
        _ => return None,
    })
}
