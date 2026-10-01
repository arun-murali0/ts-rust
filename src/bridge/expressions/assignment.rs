use oxc_ast::ast::{AssignmentExpression, AssignmentOperator, AssignmentTarget, BinaryOperator};
use oxc_semantic::{Scoping, SymbolId};
use oxc_span::GetSpan;

use crate::arena::TypeId;
use crate::types::Type;

use super::super::context::CheckContext;
use super::super::narrow::resolve_symbol_id;
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
            // A plain assignment to a variable is checked against the type it was
            // declared with, not the narrowed type it happens to have at this
            // point: `if (x !== null) { x = null; }` is valid for a
            // `string | null`. Compound assignment below keeps the narrowed type,
            // since it reads the current value.
            let declared = declared_identifier_target(&assign.left, scoping, ctx);
            let check_against = declared.map_or(target_type, |(_, declared_type)| declared_type);
            if !ctx.semantic().is_assignable(right_type, check_against) {
                ctx.error(
                    crate::diagnostic_messages::messages::declared_type_mismatch(
                        &ctx.arena,
                        right_type,
                        check_against,
                    ),
                    assign.span(),
                );
            }
            if let Some((symbol_id, declared_type)) = declared {
                narrow_on_assignment(symbol_id, declared_type, right_type, ctx);
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

// The symbol and declared type behind a plain identifier target, read from the
// symbol table rather than the narrowing overlay. None for any other target, or an
// identifier with no registered type, in which case the caller falls back to the
// type resolve_assignment_target_type produced.
fn declared_identifier_target(
    target: &AssignmentTarget,
    scoping: &Scoping,
    ctx: &CheckContext<'_, '_>,
) -> Option<(SymbolId, TypeId)> {
    let AssignmentTarget::AssignmentTargetIdentifier(ident) = target else {
        return None;
    };
    let symbol_id = resolve_symbol_id(ident, scoping)?;
    let declared = ctx.symbols.get(symbol_id)?;
    Some((symbol_id, declared))
}

// After `x = value`, x reads as the members of its declared union that the
// assigned type can be assigned to (tsc's assignment narrowing), or as the declared
// type when that leaves nothing or the declared type is not a union. Either way
// this replaces any overlay entry a guard left behind, which would otherwise be
// stale the moment x holds a new value.
fn narrow_on_assignment(
    symbol_id: SymbolId,
    declared: TypeId,
    assigned: TypeId,
    ctx: &mut CheckContext<'_, '_>,
) {
    let members = match ctx.arena.get(declared) {
        Type::Union(members) => members.clone(),
        _ => {
            ctx.narrow.insert(symbol_id, declared);
            return;
        }
    };
    let mut kept = Vec::with_capacity(members.len());
    for member in members {
        if ctx.semantic().is_assignable(assigned, member) {
            kept.push(member);
        }
    }
    let narrowed = if kept.is_empty() {
        declared
    } else {
        ctx.arena.alloc_union(kept)
    };
    ctx.narrow.insert(symbol_id, narrowed);
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
