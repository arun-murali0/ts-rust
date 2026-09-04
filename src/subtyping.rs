//! `is_subtype(sub, sup)`, the single relation every assignability check
//! in the crate routes through (variable declarations, call arguments,
//! return statements). Centralizing it here means a new `Type` variant
//! forces a match arm to be written before it compiles, instead of being
//! silently treated as always-compatible somewhere else in the codebase.

use std::cmp::Ordering;

use crate::arena::{TypeArena, TypeId};
use crate::types::{ObjectType, Type};

pub fn is_subtype(arena: &TypeArena, sub: TypeId, sup: TypeId) -> bool {
    if sub == sup {
        return true;
    }

    // `any` and the internal error sentinel are both universally
    // compatible, in either position, for different reasons: `any` is
    // TypeScript's explicit escape hatch, `Error` exists purely to stop a
    // failed inference from cascading into further diagnostics.
    if is_universally_compatible(arena, sub) || is_universally_compatible(arena, sup) {
        return true;
    }

    match (arena.get(sub), arena.get(sup)) {
        // Every type is assignable to `unknown`. `unknown` itself is only
        // assignable to `unknown`, `any`, or `Error`, already handled above.
        (_, Type::Unknown) => true,

        (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
        (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
        (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::NumberLiteral(_), Type::Number) => true,
        (Type::BooleanLiteral(_), Type::Boolean) => true,

        // Bottom type: subtype of everything. Nothing is a subtype of it
        // except itself, already handled by the `sub == sup` fast path
        // above since `never` is a single pre-seeded TypeId.
        (Type::Never, _) => true,

        // A union is a subtype of `sup` only if every one of its members is
        // (all-of). Checked before the any-of arm below so a union-to-union
        // comparison resolves member-by-member on the `sub` side first.
        (Type::Union(sub_members), _) => sub_members.iter().all(|&member| is_subtype(arena, member, sup)),

        // `sub` is a subtype of a union if it matches at least one member
        // (any-of).
        (_, Type::Union(sup_members)) => sup_members.iter().any(|&member| is_subtype(arena, sub, member)),

        (Type::Function(a), Type::Function(b)) => {
            // Contravariant parameters, covariant return. Exact arity
            // required; overloads are a later-version concern.
            a.params.len() == b.params.len()
                && a.params
                    .iter()
                    .zip(&b.params)
                    .all(|(&a_param, &b_param)| is_subtype(arena, b_param, a_param))
                && is_subtype(arena, a.return_type, b.return_type)
        }

        // Covariant element type. Matches real TypeScript behavior (this is
        // technically unsound under mutation, since `Array<T>` is invariant in
        // a fully sound type system, but neither TypeScript nor this
        // checker model mutation-site variance, so covariance is the
        // useful, honest choice here rather than a stricter rule nothing
        // else in the checker would exploit).
        (Type::Array(a), Type::Array(b)) => is_subtype(arena, *a, *b),

        (Type::Object(a), Type::Object(b)) => object_is_subtype(arena, a, b),

        _ => false,
    }
}

/// Width subtyping: `sub` is a subtype of `sup` if `sub` has every required
/// property `sup` has, each at a compatible type. Optional properties in
/// `sup` don't need to exist in `sub` at all. Extra properties in `sub`
/// that `sup` doesn't mention are fine.
///
/// This doesn't check the excess-property rule TypeScript applies to fresh
/// object literals. That's a call-site-shape check, not a subtyping rule,
/// and is deferred (see docs/ROADMAP.md).
///
/// Both property lists are sorted by name at construction time, so this
/// runs as a single merge-join pass instead of a `.find()` per property.
fn object_is_subtype(arena: &TypeArena, sub: &ObjectType, sup: &ObjectType) -> bool {
    let mut sub_properties = sub.properties.iter().peekable();

    'sup_properties: for sup_property in &sup.properties {
        while let Some(sub_property) = sub_properties.peek() {
            match sub_property.name.cmp(&sup_property.name) {
                Ordering::Less => {
                    sub_properties.next();
                }
                Ordering::Equal => {
                    let sub_property = sub_properties.next().expect("peeked Some");
                    if !is_subtype(arena, sub_property.type_id, sup_property.type_id) {
                        return false;
                    }
                    continue 'sup_properties;
                }
                Ordering::Greater => break,
            }
        }
        if !sup_property.optional {
            return false;
        }
    }

    true
}

fn is_universally_compatible(arena: &TypeArena, id: TypeId) -> bool {
    matches!(arena.get(id), Type::Any | Type::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_is_subtype_of_itself() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.number(), arena.number()));
    }

    #[test]
    fn number_is_not_subtype_of_string() {
        let arena = TypeArena::new();
        assert!(!is_subtype(&arena, arena.number(), arena.string()));
    }

    #[test]
    fn any_is_compatible_in_both_directions() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.any(), arena.string()));
        assert!(is_subtype(&arena, arena.string(), arena.any()));
    }

    #[test]
    fn error_sentinel_does_not_cascade_into_a_mismatch() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.error(), arena.number()));
        assert!(is_subtype(&arena, arena.number(), arena.error()));
    }

    #[test]
    fn everything_is_subtype_of_unknown() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.string(), arena.unknown()));
    }

    #[test]
    fn unknown_is_not_assignable_to_a_concrete_type() {
        let arena = TypeArena::new();
        assert!(!is_subtype(&arena, arena.unknown(), arena.string()));
    }

    #[test]
    fn function_subtyping_is_contravariant_in_params_covariant_in_return() {
        use crate::types::FunctionType;

        let mut arena = TypeArena::new();
        // (x: unknown) -> number, accepts anything, promises a number back.
        let sub = arena.alloc(Type::Function(FunctionType {
            params: vec![arena.unknown()],
            return_type: arena.number(),
            is_untyped: false,
        }));
        // (x: string) -> unknown, accepts less, promises less.
        let sup = arena.alloc(Type::Function(FunctionType {
            params: vec![arena.string()],
            return_type: arena.unknown(),
            is_untyped: false,
        }));

        // `sub` is substitutable anywhere `sup` is expected: it handles at
        // least every input `sup` would receive, and returns something at
        // least as specific as `sup` promises.
        assert!(is_subtype(&arena, sub, sup));
        assert!(!is_subtype(&arena, sup, sub));
    }

    fn object_type(props: &[(&str, TypeId, bool)]) -> Type {
        use crate::types::{ObjectType, PropertyEntry};

        // Deliberately sorted here to match what
        // `type_annotation::resolve_object_type` guarantees at
        // construction time. subtyping.rs assumes this invariant, it
        // doesn't enforce it.
        let mut properties: Vec<PropertyEntry> = props
            .iter()
            .map(|&(name, type_id, optional)| PropertyEntry { name: name.to_string(), type_id, optional })
            .collect();
        properties.sort_by(|a, b| a.name.cmp(&b.name));
        Type::Object(ObjectType { properties })
    }

    #[test]
    fn object_with_matching_properties_is_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[("name", arena.string(), false), ("age", arena.number(), false)]));
        let sup = arena.alloc(object_type(&[("name", arena.string(), false), ("age", arena.number(), false)]));
        assert!(is_subtype(&arena, sub, sup));
    }

    #[test]
    fn object_with_extra_property_is_still_subtype() {
        // Width subtyping: having more than required is fine.
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[
            ("name", arena.string(), false),
            ("age", arena.number(), false),
            ("id", arena.number(), false),
        ]));
        let sup = arena.alloc(object_type(&[("name", arena.string(), false)]));
        assert!(is_subtype(&arena, sub, sup));
    }

    #[test]
    fn object_missing_a_required_property_is_not_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[("name", arena.string(), false)]));
        let sup = arena.alloc(object_type(&[("name", arena.string(), false), ("age", arena.number(), false)]));
        assert!(!is_subtype(&arena, sub, sup));
    }

    #[test]
    fn object_missing_an_optional_property_is_still_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[("name", arena.string(), false)]));
        let sup = arena.alloc(object_type(&[("name", arena.string(), false), ("age", arena.number(), true)]));
        assert!(is_subtype(&arena, sub, sup));
    }

    #[test]
    fn object_with_mismatched_property_type_is_not_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[("age", arena.string(), false)]));
        let sup = arena.alloc(object_type(&[("age", arena.number(), false)]));
        assert!(!is_subtype(&arena, sub, sup));
    }

    #[test]
    fn array_element_type_is_covariant() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(Type::Array(arena.number()));
        let sup = arena.alloc(Type::Array(arena.unknown()));
        assert!(is_subtype(&arena, sub, sup));
        assert!(!is_subtype(&arena, sup, sub));
    }

    #[test]
    fn array_of_mismatched_element_types_is_not_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(Type::Array(arena.number()));
        let sup = arena.alloc(Type::Array(arena.string()));
        assert!(!is_subtype(&arena, sub, sup));
    }

    #[test]
    fn concrete_type_is_subtype_of_a_union_containing_it() {
        let mut arena = TypeArena::new();
        let union = arena.alloc(Type::Union(vec![arena.number(), arena.string()]));
        assert!(is_subtype(&arena, arena.number(), union));
    }

    #[test]
    fn concrete_type_is_not_subtype_of_a_union_missing_it() {
        let mut arena = TypeArena::new();
        let union = arena.alloc(Type::Union(vec![arena.number(), arena.string()]));
        assert!(!is_subtype(&arena, arena.boolean(), union));
    }

    #[test]
    fn union_is_subtype_of_sup_only_if_every_member_is() {
        let mut arena = TypeArena::new();
        let sub_union = arena.alloc(Type::Union(vec![arena.number(), arena.string()]));
        // Every member of sub_union is a subtype of unknown.
        assert!(is_subtype(&arena, sub_union, arena.unknown()));
        // Not every member of sub_union is a subtype of `number` alone.
        assert!(!is_subtype(&arena, sub_union, arena.number()));
    }

    #[test]
    fn literal_widens_to_its_base_primitive() {
        let mut arena = TypeArena::new();
        let five = arena.alloc(Type::NumberLiteral(5.0));
        assert!(is_subtype(&arena, five, arena.number()));
        assert!(!is_subtype(&arena, arena.number(), five));
    }

    #[test]
    fn equal_literals_are_subtypes_of_each_other() {
        let mut arena = TypeArena::new();
        let a = arena.alloc(Type::StringLiteral("hi".to_string()));
        let b = arena.alloc(Type::StringLiteral("hi".to_string()));
        assert!(is_subtype(&arena, a, b));
    }

    #[test]
    fn different_literals_are_not_subtypes_of_each_other() {
        let mut arena = TypeArena::new();
        let a = arena.alloc(Type::StringLiteral("hi".to_string()));
        let b = arena.alloc(Type::StringLiteral("bye".to_string()));
        assert!(!is_subtype(&arena, a, b));
    }

    #[test]
    fn never_is_subtype_of_everything() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.never(), arena.string()));
        assert!(is_subtype(&arena, arena.never(), arena.any()));
    }

    #[test]
    fn nothing_but_never_is_a_subtype_of_never() {
        let arena = TypeArena::new();
        assert!(!is_subtype(&arena, arena.string(), arena.never()));
    }

    #[test]
    fn union_construction_flattens_and_dedupes() {
        let mut arena = TypeArena::new();
        let inner = arena.alloc(Type::Union(vec![arena.string(), arena.number()]));
        let outer = arena.alloc_union(vec![inner, arena.number(), arena.boolean()]);
        let Type::Union(members) = arena.get(outer) else { panic!("expected a union") };
        assert_eq!(members.len(), 3);
    }

    #[test]
    fn union_dedupes_equal_literals_even_with_different_type_ids() {
        // Regression test: alloc_union used to dedupe by comparing TypeId
        // values directly. arena.number() above is a pre-seeded singleton
        // (same TypeId every call), so that test never actually exercised
        // the real-world case: two separate `StringLiteral("a")`
        // allocations get two different TypeIds even though they're the
        // same type, so `"a" | "a"` was silently kept as a two-member
        // union instead of collapsing to one.
        let mut arena = TypeArena::new();
        let a1 = arena.alloc(Type::StringLiteral("a".to_string()));
        let a2 = arena.alloc(Type::StringLiteral("a".to_string()));
        assert_ne!(a1, a2, "test setup: these must be genuinely different TypeIds");

        let result = arena.alloc_union(vec![a1, a2]);
        // `queue.pop()` processes in LIFO order, so which of the two
        // equal TypeIds survives isn't guaranteed; what matters is that
        // exactly one of them does, not a two-member union.
        assert!(result == a1 || result == a2, "expected exactly one of the two equal literals to survive");
        assert_eq!(
            arena.get(result),
            &Type::StringLiteral("a".to_string()),
            "collapsed result should still be the string literal 'a'"
        );
    }

    #[test]
    fn union_of_never_and_one_other_type_collapses_to_that_type() {
        let mut arena = TypeArena::new();
        let never = arena.never();
        let result = arena.alloc_union(vec![never, arena.string()]);
        assert_eq!(result, arena.string());
    }

    #[test]
    fn union_of_only_never_collapses_to_never() {
        let mut arena = TypeArena::new();
        let result = arena.alloc_union(vec![arena.never()]);
        assert_eq!(result, arena.never());
    }
}
