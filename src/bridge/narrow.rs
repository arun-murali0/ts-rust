use oxc_ast::ast::{BinaryOperator, Expression, IdentifierReference, UnaryOperator};
use oxc_semantic::{Scoping, SymbolId};

use crate::arena::{TypeArena, TypeId};
use crate::bridge::context::CheckContext;
use crate::types::Type;

// A narrow overlay on top of the declared symbol types in CheckContext.symbols: a
// small association list, not a full map, since only a handful of variables are
// ever narrowed within one branch. Looked up before falling back to a symbol's
// declared type wherever narrowing might apply.
#[derive(Default, Clone)]
pub struct NarrowState(Vec<(SymbolId, TypeId)>);

impl NarrowState {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    fn from_single(symbol_id: SymbolId, type_id: TypeId) -> Self {
        Self(vec![(symbol_id, type_id)])
    }

    pub fn get(&self, symbol_id: SymbolId) -> Option<TypeId> {
        self.0
            .iter()
            .find(|&&(id, _)| id == symbol_id)
            .map(|&(_, ty)| ty)
    }

    pub fn insert(&mut self, symbol_id: SymbolId, type_id: TypeId) {
        if let Some(entry) = self.0.iter_mut().find(|(id, _)| *id == symbol_id) {
            entry.1 = type_id;
        } else {
            self.0.push((symbol_id, type_id));
        }
    }

    pub fn extend(&mut self, other: NarrowState) {
        for (symbol_id, type_id) in other.0 {
            self.insert(symbol_id, type_id);
        }
    }
}

// Given a condition expression, returns the type overlay that should apply in the
// true branch and the one that should apply in the false branch. Recognizes a
// handful of specific shapes (typeof checks, equality against null or undefined,
// and a bare identifier's own truthiness); anything else narrows nothing in
// either branch, which is always safe, just less precise.
pub fn narrow_condition(
    test: &Expression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> (NarrowState, NarrowState) {
    match test {
        Expression::BinaryExpression(bin) => {
            // Treats == and != identically to === and !==, just flipping which
            // computed branch (true_type vs false_type) ends up where, so the
            // detection logic below only needs to handle the equality case once.
            let negated = matches!(
                bin.operator,
                BinaryOperator::Inequality | BinaryOperator::StrictInequality
            );
            let is_equality = matches!(
                bin.operator,
                BinaryOperator::Equality
                    | BinaryOperator::Inequality
                    | BinaryOperator::StrictEquality
                    | BinaryOperator::StrictInequality
            );
            if !is_equality {
                return empty_pair();
            }

            if let Some((symbol_id, tag)) = typeof_check(&bin.left, &bin.right, scoping)
                .or_else(|| typeof_check(&bin.right, &bin.left, scoping))
            {
                // A tag no type in this checker can ever match, "bigint" or
                // "symbol", would filter every union member out and leave never
                // in the true branch, turning a gap in the type model into
                // errors on the source. Narrowing nothing is less precise but
                // never wrong.
                if !is_known_typeof_tag(&tag) {
                    return empty_pair();
                }
                return by_symbol(ctx, symbol_id, |arena, current, want_true| {
                    narrow_by_typeof(arena, current, &tag, want_true != negated)
                });
            }

            if let Some((symbol_id, is_null)) = nullish_check(&bin.left, &bin.right, scoping)
                .or_else(|| nullish_check(&bin.right, &bin.left, scoping))
            {
                return by_symbol(ctx, symbol_id, |arena, current, want_true| {
                    narrow_by_nullish(arena, current, is_null, want_true != negated)
                });
            }

            empty_pair()
        }

        Expression::Identifier(ident) => {
            let Some(symbol_id) = resolve_symbol_id(ident, scoping) else {
                return empty_pair();
            };
            by_symbol(ctx, symbol_id, narrow_truthy)
        }

        _ => empty_pair(),
    }
}

fn empty_pair() -> (NarrowState, NarrowState) {
    (NarrowState::new(), NarrowState::new())
}

fn by_symbol(
    ctx: &mut CheckContext<'_, '_>,
    symbol_id: SymbolId,
    narrow: impl Fn(&mut TypeArena, TypeId, bool) -> TypeId,
) -> (NarrowState, NarrowState) {
    // The overlay first, so narrowing composes: an inner condition refines what
    // the enclosing branch already established about this symbol rather than
    // starting again from its declared type and silently widening it back.
    let Some(current) = ctx
        .narrow
        .get(symbol_id)
        .or_else(|| ctx.symbols.get(symbol_id))
    else {
        return empty_pair();
    };
    let true_type = narrow(&mut ctx.arena, current, true);
    let false_type = narrow(&mut ctx.arena, current, false);
    (
        NarrowState::from_single(symbol_id, true_type),
        NarrowState::from_single(symbol_id, false_type),
    )
}

pub fn resolve_symbol_id(ident: &IdentifierReference, scoping: &Scoping) -> Option<SymbolId> {
    ident
        .reference_id
        .get()
        .and_then(|reference_id| scoping.get_reference(reference_id).symbol_id())
}

fn typeof_check(
    maybe_typeof: &Expression,
    maybe_tag: &Expression,
    scoping: &Scoping,
) -> Option<(SymbolId, String)> {
    let Expression::UnaryExpression(unary) = maybe_typeof else {
        return None;
    };
    if unary.operator != UnaryOperator::Typeof {
        return None;
    }
    let Expression::Identifier(ident) = &unary.argument else {
        return None;
    };
    let symbol_id = resolve_symbol_id(ident, scoping)?;
    let Expression::StringLiteral(tag) = maybe_tag else {
        return None;
    };
    Some((symbol_id, tag.value.to_string()))
}

fn nullish_check(
    maybe_ident: &Expression,
    maybe_nullish: &Expression,
    scoping: &Scoping,
) -> Option<(SymbolId, bool)> {
    let Expression::Identifier(ident) = maybe_ident else {
        return None;
    };
    let symbol_id = resolve_symbol_id(ident, scoping)?;
    match maybe_nullish {
        Expression::NullLiteral(_) => Some((symbol_id, true)),

        Expression::Identifier(other) if other.name == "undefined" => Some((symbol_id, false)),
        _ => None,
    }
}

fn is_known_typeof_tag(tag: &str) -> bool {
    matches!(
        tag,
        "string" | "number" | "boolean" | "undefined" | "object" | "function"
    )
}

// `typeof null` is "object" in JavaScript, so null belongs with objects and
// arrays here rather than with the primitives, and a `typeof x === "object"`
// check therefore does not remove null from x.
fn matches_typeof_tag(arena: &TypeArena, id: TypeId, tag: &str) -> bool {
    matches!(
        (arena.get(id), tag),
        (Type::String | Type::StringLiteral(_), "string")
            | (Type::Number | Type::NumberLiteral(_), "number")
            | (Type::Boolean | Type::BooleanLiteral(_), "boolean")
            | (Type::Undefined, "undefined")
            | (Type::Object(_) | Type::Array(_) | Type::Null, "object")
            | (Type::Function(_), "function")
    )
}

// Shared shape for every narrow_by_* and narrow_to_* function below: if id is a
// union, keep only the members that satisfy the predicate and rebuild the union;
// if it is a single type, either keep it whole or collapse it to never, since a
// single type either fully satisfies the predicate or does not, there is no
// partial member to keep.
fn narrow_by_typeof(arena: &mut TypeArena, id: TypeId, tag: &str, want_match: bool) -> TypeId {
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members
                .into_iter()
                .filter(|&m| matches_typeof_tag(arena, m, tag) == want_match)
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            if matches_typeof_tag(arena, id, tag) == want_match {
                id
            } else {
                arena.never()
            }
        }
    }
}

fn narrow_by_nullish(
    arena: &mut TypeArena,
    id: TypeId,
    target_is_null: bool,
    want_match: bool,
) -> TypeId {
    let is_target = |arena: &TypeArena, t: TypeId| {
        if target_is_null {
            matches!(arena.get(t), Type::Null)
        } else {
            matches!(arena.get(t), Type::Undefined)
        }
    };
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members
                .into_iter()
                .filter(|&m| is_target(arena, m) == want_match)
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            if is_target(arena, id) == want_match {
                id
            } else {
                arena.never()
            }
        }
    }
}

// Conservative on purpose: only returns true for a type that is provably falsy in
// every case, such as the literal 0 or an empty string literal. A plain, un-narrowed
// number or string is neither definitely falsy nor definitely truthy, so it is left
// alone by narrow_truthy below rather than guessed at.
fn is_definitely_falsy(arena: &TypeArena, id: TypeId) -> bool {
    match arena.get(id) {
        Type::Null | Type::Undefined | Type::BooleanLiteral(false) => true,
        Type::NumberLiteral(n) => *n == 0.0,
        Type::StringLiteral(s) => s.is_empty(),
        _ => false,
    }
}

fn is_definitely_truthy(arena: &TypeArena, id: TypeId) -> bool {
    match arena.get(id) {
        Type::Object(_) | Type::Array(_) | Type::Function(_) | Type::BooleanLiteral(true) => true,
        Type::NumberLiteral(n) => *n != 0.0,
        Type::StringLiteral(s) => !s.is_empty(),
        _ => false,
    }
}

fn narrow_truthy(arena: &mut TypeArena, id: TypeId, want_truthy: bool) -> TypeId {
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members
                .into_iter()
                .filter(|&m| {
                    if want_truthy {
                        !is_definitely_falsy(arena, m)
                    } else {
                        !is_definitely_truthy(arena, m)
                    }
                })
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            let keep = if want_truthy {
                !is_definitely_falsy(arena, id)
            } else {
                !is_definitely_truthy(arena, id)
            };
            if keep { id } else { arena.never() }
        }
    }
}

fn union_members(arena: &TypeArena, id: TypeId) -> Option<Vec<TypeId>> {
    match arena.get(id) {
        Type::Union(members) => Some(members.clone()),
        _ => None,
    }
}

pub fn narrow_to_truthy(arena: &mut TypeArena, id: TypeId) -> TypeId {
    narrow_truthy(arena, id, true)
}

pub fn narrow_to_falsy(arena: &mut TypeArena, id: TypeId) -> TypeId {
    narrow_truthy(arena, id, false)
}

pub fn narrow_to_non_nullish(arena: &mut TypeArena, id: TypeId) -> TypeId {
    let is_nullish =
        |arena: &TypeArena, t: TypeId| matches!(arena.get(t), Type::Null | Type::Undefined);
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members
                .into_iter()
                .filter(|&m| !is_nullish(arena, m))
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            if is_nullish(arena, id) {
                arena.never()
            } else {
                id
            }
        }
    }
}
