//! Narrowing for `if (test) { ... } else { ... }`. Scoped narrowing only:
//! a narrowed type is visible inside the branch that earned it and reverts
//! once the `if` statement ends. Persisting a narrowed type past the `if`
//! when both branches agree (TypeScript's real "join" behavior) isn't
//! implemented yet — see `docs/ROADMAP.md`.
//!
//! Three forms are recognized: `typeof x === "tag"`, `x === null` /
//! `x === undefined`, and bare truthiness (`if (x)`). Anything else
//! narrows nothing — both branches just see `x`'s declared type.

use oxc_ast::ast::{BinaryOperator, Expression, IdentifierReference, UnaryOperator};
use oxc_semantic::{Scoping, SymbolId};

use crate::arena::{TypeArena, TypeId};
use crate::bridge::context::CheckContext;
use crate::types::Type;

/// Per-branch narrowing overlay: which symbols have a narrower type in
/// effect than their declared one, for as long as the current branch
/// lasts. Deliberately not a `HashMap`, unlike `SymbolTypeMap`: a fresh
/// one of these is created for every `if` (and, once built, every `&&`
/// and ternary), and each one typically narrows only a handful of
/// symbols — often exactly one — out of however many exist in the whole
/// file. A small linear-scanned `Vec` beats hashing at this size (no
/// hash computation, one contiguous cache line to scan) and critically
/// doesn't allocate memory proportional to the *file's* total symbol
/// count on every single branch the way a dense, `SymbolTypeMap`-style
/// `Vec<Option<TypeId>>` would. Same key type as `SymbolTypeMap`,
/// opposite right representation — the deciding factor is whether the
/// *use* is dense or sparse, not the key type alone.
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
        self.0.iter().find(|&&(id, _)| id == symbol_id).map(|&(_, ty)| ty)
    }

    pub fn insert(&mut self, symbol_id: SymbolId, type_id: TypeId) {
        if let Some(entry) = self.0.iter_mut().find(|(id, _)| *id == symbol_id) {
            entry.1 = type_id;
        } else {
            self.0.push((symbol_id, type_id));
        }
    }

    /// Merges another branch's overrides into this one, later entry
    /// winning on a duplicate key — the same semantics
    /// `HashMap::extend` had, which this replaces.
    pub fn extend(&mut self, other: NarrowState) {
        for (symbol_id, type_id) in other.0 {
            self.insert(symbol_id, type_id);
        }
    }
}

pub fn narrow_condition(test: &Expression, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) -> (NarrowState, NarrowState) {
    match test {
        Expression::BinaryExpression(bin) => {
            let negated = matches!(bin.operator, BinaryOperator::Inequality | BinaryOperator::StrictInequality);
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
            let Some(symbol_id) = resolve_symbol_id(ident, scoping) else { return empty_pair() };
            by_symbol(ctx, symbol_id, |arena, current, want_true| narrow_truthy(arena, current, want_true))
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
    let Some(current) = ctx.symbols.get(symbol_id) else { return empty_pair() };
    let true_type = narrow(&mut ctx.arena, current, true);
    let false_type = narrow(&mut ctx.arena, current, false);
    (NarrowState::from_single(symbol_id, true_type), NarrowState::from_single(symbol_id, false_type))
}

pub fn resolve_symbol_id(ident: &IdentifierReference, scoping: &Scoping) -> Option<SymbolId> {
    ident.reference_id.get().and_then(|reference_id| scoping.get_reference(reference_id).symbol_id())
}

fn typeof_check(maybe_typeof: &Expression, maybe_tag: &Expression, scoping: &Scoping) -> Option<(SymbolId, String)> {
    let Expression::UnaryExpression(unary) = maybe_typeof else { return None };
    if unary.operator != UnaryOperator::Typeof {
        return None;
    }
    let Expression::Identifier(ident) = &unary.argument else { return None };
    let symbol_id = resolve_symbol_id(ident, scoping)?;
    let Expression::StringLiteral(tag) = maybe_tag else { return None };
    Some((symbol_id, tag.value.to_string()))
}

fn nullish_check(maybe_ident: &Expression, maybe_nullish: &Expression, scoping: &Scoping) -> Option<(SymbolId, bool)> {
    let Expression::Identifier(ident) = maybe_ident else { return None };
    let symbol_id = resolve_symbol_id(ident, scoping)?;
    match maybe_nullish {
        Expression::NullLiteral(_) => Some((symbol_id, true)),
        // `undefined` is a global identifier in JS, not a literal keyword,
        // so this is a name check rather than a dedicated AST node. A
        // locally shadowed `undefined` would misfire this — an accepted
        // edge case for now.
        Expression::Identifier(other) if other.name == "undefined" => Some((symbol_id, false)),
        _ => None,
    }
}

fn matches_typeof_tag(arena: &TypeArena, id: TypeId, tag: &str) -> bool {
    match (arena.get(id), tag) {
        (Type::String | Type::StringLiteral(_), "string") => true,
        (Type::Number | Type::NumberLiteral(_), "number") => true,
        (Type::Boolean | Type::BooleanLiteral(_), "boolean") => true,
        (Type::Undefined, "undefined") => true,
        // "object", "function", "bigint", "symbol" aren't distinguishable
        // in this type system yet.
        _ => false,
    }
}

fn narrow_by_typeof(arena: &mut TypeArena, id: TypeId, tag: &str, want_match: bool) -> TypeId {
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members.into_iter().filter(|&m| matches_typeof_tag(arena, m, tag) == want_match).collect();
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

fn narrow_by_nullish(arena: &mut TypeArena, id: TypeId, target_is_null: bool, want_match: bool) -> TypeId {
    let is_target = |arena: &TypeArena, t: TypeId| {
        if target_is_null { matches!(arena.get(t), Type::Null) } else { matches!(arena.get(t), Type::Undefined) }
    };
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members.into_iter().filter(|&m| is_target(arena, m) == want_match).collect();
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

/// A value is "definitely falsy" only when its type pins down a single
/// known-falsy value. A plain `number` or `string` might be falsy at
/// runtime (`0`, `""`) but we can't rule out the truthy values they could
/// also hold, so those aren't narrowed away here.
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
                .filter(|&m| if want_truthy { !is_definitely_falsy(arena, m) } else { !is_definitely_truthy(arena, m) })
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            let keep = if want_truthy { !is_definitely_falsy(arena, id) } else { !is_definitely_truthy(arena, id) };
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
