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
// handful of specific shapes (typeof checks, equality against null, undefined, or
// a literal value, a bare identifier's own truthiness, and `!` or parentheses
// wrapping any of those); anything else narrows nothing in either branch, which
// is always safe, just less precise.
pub fn narrow_condition(
    test: &Expression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> (NarrowState, NarrowState) {
    match test {
        // oxc keeps parentheses as their own node instead of discarding them, so
        // `!(typeof x === "number")`'s inner expression is a ParenthesizedExpression
        // wrapping the BinaryExpression, not the BinaryExpression itself. Without
        // this arm that inner shape never matches anything below and the whole
        // condition narrows nothing, in either branch, silently.
        Expression::ParenthesizedExpression(inner) => {
            narrow_condition(&inner.expression, scoping, ctx)
        }

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

            let is_loose = matches!(
                bin.operator,
                BinaryOperator::Equality | BinaryOperator::Inequality
            );

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
                    narrow_by_nullish(arena, current, is_null, is_loose, want_true != negated)
                });
            }

            // Loose equality against a literal (x == "a") is left alone: unlike
            // null/undefined, JavaScript's abstract-equality coercions for other
            // literals (0 == "0", "" == false) mean a matching member of x's own
            // union is not the only way this can pass, so narrowing to it could
            // remove a value the true branch can still actually hold.
            if !is_loose {
                if let Some((symbol_id, literal)) =
                    literal_check(&bin.left, &bin.right, scoping, &mut ctx.arena)
                        .or_else(|| literal_check(&bin.right, &bin.left, scoping, &mut ctx.arena))
                {
                    return by_symbol(ctx, symbol_id, move |arena, current, want_true| {
                        narrow_by_literal(arena, current, literal, want_true != negated)
                    });
                }
            }

            empty_pair()
        }

        // `!x`, `!(x === null)`, `!(typeof x === "string")`: whatever the inner
        // expression's own pair would be, just with the two branches swapped, so
        // this composes with every condition shape above and below for free.
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::LogicalNot => {
            let (true_overrides, false_overrides) = narrow_condition(&unary.argument, scoping, ctx);
            (false_overrides, true_overrides)
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

// A switch case's own narrowing: `case <test>:` behaves like `if (discriminant
// === test)` for the statements in that case, but only the true side is ever
// needed here -- check_switch_statement (control_flow.rs) already resets to the
// outer state before each case, so there is no false branch to compute the way
// an if/else needs one. `symbol_id` is the discriminant's own symbol, already
// resolved by the caller (only a bare identifier discriminant is handled -- see
// the comment above this function's call site).
pub fn narrow_by_identifier_equals(
    symbol_id: SymbolId,
    test: &Expression,
    ctx: &mut CheckContext<'_, '_>,
) -> NarrowState {
    let Some(current) = ctx
        .narrow
        .get(symbol_id)
        .or_else(|| ctx.symbols.get(symbol_id))
    else {
        return NarrowState::new();
    };
    let Some(literal) = resolve_literal(test, &mut ctx.arena) else {
        return NarrowState::new();
    };
    let narrowed = narrow_by_literal(&mut ctx.arena, current, literal, true);
    NarrowState::from_single(symbol_id, narrowed)
}

// Resolves a switch case's own test the same way narrow_by_identifier_equals
// does, exposed so check_switch_statement can build up the set every sibling
// case excludes before default gets its turn (see narrow_by_identifier_excluding).
pub fn resolve_case_literal(test: &Expression, arena: &mut TypeArena) -> Option<TypeId> {
    resolve_literal(test, arena)
}

// A switch's default case: the discriminant is narrowed to whatever remains once
// every literal a sibling case matched on is excluded. `excluded` is built by the
// caller from every sibling case's own test via resolve_case_literal; a case this
// checker cannot resolve a literal from (a computed test, say) is simply absent
// from that list rather than aborting the whole exclusion -- default still
// narrows out what it can prove, which is always safe, and keeps whatever member
// it could not rule out.
pub fn narrow_by_identifier_excluding(
    symbol_id: SymbolId,
    excluded: &[TypeId],
    ctx: &mut CheckContext<'_, '_>,
) -> NarrowState {
    let Some(current) = ctx
        .narrow
        .get(symbol_id)
        .or_else(|| ctx.symbols.get(symbol_id))
    else {
        return NarrowState::new();
    };
    let narrowed = narrow_by_literals(&mut ctx.arena, current, excluded, false);
    NarrowState::from_single(symbol_id, narrowed)
}

// The literal-resolution half of literal_check, split out so
// narrow_by_identifier_equals above can reuse it without needing a second
// identifier to match against -- a case's test is compared to a discriminant
// already known by symbol_id, not discovered from an ident/other pair the way a
// binary expression's two operands are.
fn resolve_literal(expr: &Expression, arena: &mut TypeArena) -> Option<TypeId> {
    match expr {
        Expression::StringLiteral(lit) => {
            Some(arena.alloc(Type::StringLiteral(lit.value.to_string())))
        }
        Expression::NumericLiteral(lit) => Some(arena.alloc(Type::NumberLiteral(lit.value))),
        Expression::BooleanLiteral(lit) => Some(arena.alloc(Type::BooleanLiteral(lit.value))),
        _ => None,
    }
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

// Recognizes `ident === <literal>`, where the literal is a value this checker's
// type model already represents as its own TypeId (a string, number, or boolean
// literal), matching how a discriminant property or a narrowed union is typically
// spelled: "circle", 0, true. Anything else on the literal side (an identifier, a
// computed expression) is not something narrow_by_literal has a TypeId to compare
// union members against, so it returns None and the caller falls through to
// empty_pair the same way an unrecognized shape always has. Takes the arena
// directly, unlike nullish_check and typeof_check, since allocating the literal's
// TypeId (interned, so a repeated "circle" always resolves to the same id -- see
// narrow_by_literal) needs a mutable arena, not just the shared Scoping the other
// *_check helpers read from.
fn literal_check(
    maybe_ident: &Expression,
    maybe_literal: &Expression,
    scoping: &Scoping,
    arena: &mut TypeArena,
) -> Option<(SymbolId, TypeId)> {
    let Expression::Identifier(ident) = maybe_ident else {
        return None;
    };
    let symbol_id = resolve_symbol_id(ident, scoping)?;
    let literal = resolve_literal(maybe_literal, arena)?;
    Some((symbol_id, literal))
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

// Same shared shape as narrow_by_typeof: keeps union members equal to `literal`
// (or every member but that one, in the false-branch/negated case), collapses a
// non-union type to itself or never. `literal` is compared by TypeId, not by
// value, which is safe here because every literal Type is interned (see
// TypeArena::alloc_string_literal and friends): two occurrences of the string
// literal "circle" always resolve to the same TypeId, so this is exactly the
// same identity a `structurally_equal` comparison would reach for a leaf type.
fn narrow_by_literal(
    arena: &mut TypeArena,
    id: TypeId,
    literal: TypeId,
    want_match: bool,
) -> TypeId {
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members
                .into_iter()
                .filter(|&m| (m == literal) == want_match)
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            if (id == literal) == want_match {
                id
            } else {
                arena.never()
            }
        }
    }
}

// Same shape as narrow_by_literal, generalized to a set: keeps every union member
// that does (or, with want_match false, does not) match one of `literals`. An
// empty slice narrows nothing -- want_match false against no literals keeps every
// member, which is the correct answer for a default case with no siblings to
// exclude, not an accidental never.
fn narrow_by_literals(
    arena: &mut TypeArena,
    id: TypeId,
    literals: &[TypeId],
    want_match: bool,
) -> TypeId {
    match union_members(arena, id) {
        Some(members) => {
            let filtered = members
                .into_iter()
                .filter(|m| literals.contains(m) == want_match)
                .collect();
            arena.alloc_union(filtered)
        }
        None => {
            if literals.contains(&id) == want_match {
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
    is_loose: bool,
    want_match: bool,
) -> TypeId {
    let is_target = |arena: &TypeArena, t: TypeId| {
        // `==`/`!=` against either `null` or `undefined` matches both: this is
        // JavaScript's specific loose-equality-with-null quirk, not general
        // loose equality, and it is symmetric -- `x == undefined` matches a
        // `null` value too, the same as `x == null` does. `===`/`!==` only ever
        // matches the exact literal on the right, so target_is_null still
        // decides which single type counts there.
        if is_loose {
            matches!(arena.get(t), Type::Null | Type::Undefined)
        } else if target_is_null {
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
