use std::cmp::Ordering;

use crate::arena::{TypeArena, TypeId};
use crate::types::{ObjectType, Param, Type};

// Structural subtyping: is sub usable wherever sup is expected. The identity check
// comes first since it is the cheapest and covers the common case of comparing a
// type to itself. Any and Error are checked next because they need to short-circuit
// before the big match below; otherwise Error, used as a sentinel for an expression
// that already failed to check, would need its own arm in every single case of that
// match instead of one shared escape hatch here.
pub fn is_subtype(arena: &TypeArena, sub: TypeId, sup: TypeId) -> bool {
    is_subtype_inner(arena, sub, sup, &mut Vec::new())
}

// A recursive type (Node { next: Node }, built by namespace::resolve's
// placeholder backpatch) means comparing sub and sup can lead back to
// comparing the same (sub, sup) pair again before either call has returned.
// `seen` tracks pairs currently on the call stack. Re-entering one is treated
// as true (coinductively: two types that only differ by "going in circles"
// are equivalent) rather than as a fresh comparison to keep making -- this is
// the standard rule for equirecursive subtyping and is what breaks the loop.
fn is_subtype_inner(
    arena: &TypeArena,
    sub: TypeId,
    sup: TypeId,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    if sub == sup {
        return true;
    }

    if is_universally_compatible(arena, sub) || is_universally_compatible(arena, sup) {
        return true;
    }

    let pair = (sub, sup);
    if seen.contains(&pair) {
        return true;
    }
    seen.push(pair);
    let result = is_subtype_uncached(arena, sub, sup, seen);
    seen.pop();
    result
}

fn is_subtype_uncached(
    arena: &TypeArena,
    sub: TypeId,
    sup: TypeId,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    match (arena.get(sub), arena.get(sup)) {
        (_, Type::Unknown) => true,

        (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
        (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
        (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::NumberLiteral(_), Type::Number) => true,
        (Type::BooleanLiteral(_), Type::Boolean) => true,

        (Type::Never, _) => true,

        // undefined satisfies void (an implicit or bare `return;` is fine for a
        // void-returning function) but not the reverse, and nothing else is
        // interchangeable with void either -- see Type::Void's own doc comment
        // on the leniency this deliberately doesn't implement.
        (Type::Undefined, Type::Void) => true,

        // These two arms are not symmetric on purpose. A union is a subtype of sup
        // only if every member is, since the value could be any of them and all
        // must qualify. sup accepts sub if any one member of sup does, since
        // matching one alternative is enough to satisfy an expected union.
        (Type::Union(sub_members), _) => sub_members
            .iter()
            .all(|&member| is_subtype_inner(arena, member, sup, seen)),

        (_, Type::Union(sup_members)) => sup_members
            .iter()
            .any(|&member| is_subtype_inner(arena, sub, member, seen)),

        (Type::Function(a), Type::Function(b)) => function_is_subtype(arena, a, b, seen),

        (Type::Array(a), Type::Array(b)) => is_subtype_inner(arena, *a, *b, seen),

        (Type::Object(a), Type::Object(b)) => object_is_subtype(arena, a, b, seen),

        _ => false,
    }
}

// Contravariant in parameters, covariant in return type: a function can stand in
// for another if it accepts everything the target promises to pass, or more, and
// returns something at least as specific as what the target promises to produce.
// Arity is checked separately from position-by-position compatibility, since
// optional and rest parameters mean two functions can have different declared
// parameter counts and still be safely interchangeable.
fn function_is_subtype(
    arena: &TypeArena,
    sub: &crate::types::FunctionType,
    sup: &crate::types::FunctionType,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    function_is_subtype_with(arena, sub, sup, false, seen)
}

// `bivariant_params` is tsc's rule for methods: a parameter position is compatible
// when the types are related in *either* direction, not only contravariantly. It
// applies to the parameters only; the return type stays covariant.
fn function_is_subtype_with(
    arena: &TypeArena,
    sub: &crate::types::FunctionType,
    sup: &crate::types::FunctionType,
    bivariant_params: bool,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    // sub cannot require more arguments than callers of sup are guaranteed to
    // supply. It is free to require fewer; its extra optional or rest slots simply
    // never get filled by such a caller.
    if required_param_count(&sub.params) > required_param_count(&sup.params) {
        return false;
    }

    let checked_positions = sub.params.len().max(sup.params.len());
    for position in 0..checked_positions {
        if let (Some(sub_param), Some(sup_param)) = (
            param_type_at(arena, &sub.params, position),
            param_type_at(arena, &sup.params, position),
        ) && !is_subtype_inner(arena, sup_param, sub_param, seen)
            && !(bivariant_params && is_subtype_inner(arena, sub_param, sup_param, seen))
        {
            return false;
        }
    }

    is_subtype_inner(arena, sub.return_type, sup.return_type, seen)
}

fn required_param_count(params: &[Param]) -> usize {
    params.iter().filter(|p| !p.optional && !p.rest).count()
}

// The type expected at a given argument position. A rest parameter absorbs every
// position past the end of the declared list, and its own declared type is the
// array type, such as number[], not the per-argument type, so it gets unwrapped to
// the element type here.
fn param_type_at(arena: &TypeArena, params: &[Param], position: usize) -> Option<TypeId> {
    let param = match params.get(position) {
        Some(param) => param,
        None => params.last().filter(|p| p.rest)?,
    };
    Some(if param.rest {
        match arena.get(param.type_id) {
            Type::Array(element) => *element,
            _ => param.type_id,
        }
    } else {
        param.type_id
    })
}

// A merge-join over both property lists, which relies on ObjectType.properties
// always being kept sorted by name. This makes the check a single linear pass
// instead of a lookup per property. sub's extra properties are simply skipped by
// the peekable iterator, which is where width subtyping (sub may have more fields
// than sup) falls out for free, and any sup property sub never reaches is only
// acceptable if sup itself marks that property optional.
fn object_is_subtype(
    arena: &TypeArena,
    sub: &ObjectType,
    sup: &ObjectType,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    // The merge-join below silently gives wrong answers on unsorted input, so an
    // unsorted ObjectType reaching here is a construction bug elsewhere, not
    // something to tolerate. Checked in debug builds (which is what the tests run).
    debug_assert!(
        is_sorted_by_name(&sub.properties) && is_sorted_by_name(&sup.properties),
        "ObjectType properties must be sorted by name; build them with ObjectType::new"
    );

    let mut sub_properties = sub.properties.iter().peekable();

    'sup_properties: for sup_property in &sup.properties {
        while let Some(sub_property) = sub_properties.peek() {
            match sub_property.name.cmp(&sup_property.name) {
                Ordering::Less => {
                    sub_properties.next();
                }
                Ordering::Equal => {
                    let Some(sub_property) = sub_properties.next() else {
                        break;
                    };
                    if sub_property.optional && !sup_property.optional {
                        return false;
                    }
                    if !property_is_subtype(arena, sub_property.type_id, sup_property, seen) {
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

// A property's type against the target property. When the target was declared
// with method syntax and both sides are functions, tsc's method rule applies
// (bivariant parameters); anything else is ordinary subtyping. The rule follows
// the target's declaration, so a function-typed property (`f: (a: A) => R`) stays
// strictly contravariant.
fn property_is_subtype(
    arena: &TypeArena,
    sub_type: TypeId,
    sup_property: &crate::types::PropertyEntry,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    if sup_property.is_method
        && let (Type::Function(sub_function), Type::Function(sup_function)) =
            (arena.get(sub_type), arena.get(sup_property.type_id))
    {
        return function_is_subtype_with(arena, sub_function, sup_function, true, seen);
    }
    is_subtype_inner(arena, sub_type, sup_property.type_id, seen)
}

fn is_sorted_by_name(properties: &[crate::types::PropertyEntry]) -> bool {
    properties.is_sorted_by(|a, b| a.name <= b.name)
}

// Any and Error both act as escape hatches, compatible with everything in both
// directions, but for different reasons. Any is TypeScript's own opt-out from
// checking. Error is this checker's internal sentinel for an expression that
// already failed to type-check; treating it as universally compatible stops one
// mistake from cascading into a wall of unrelated-looking follow-on errors.
fn is_universally_compatible(arena: &TypeArena, id: TypeId) -> bool {
    matches!(arena.get(id), Type::Any | Type::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undefined_satisfies_void() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.undefined(), arena.void()));
    }

    #[test]
    fn void_does_not_satisfy_undefined() {
        let arena = TypeArena::new();
        assert!(!is_subtype(&arena, arena.void(), arena.undefined()));
    }

    #[test]
    fn nothing_else_satisfies_void() {
        let arena = TypeArena::new();
        assert!(!is_subtype(&arena, arena.number(), arena.void()));
        assert!(!is_subtype(&arena, arena.string(), arena.void()));
    }

    #[test]
    fn void_does_not_satisfy_other_types() {
        let arena = TypeArena::new();
        assert!(!is_subtype(&arena, arena.void(), arena.number()));
    }

    #[test]
    fn never_satisfies_void_like_it_satisfies_everything() {
        let arena = TypeArena::new();
        assert!(is_subtype(&arena, arena.never(), arena.void()));
    }

    #[test]
    fn object_built_in_reverse_name_order_is_still_a_subtype() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        // Declared z-before-a, the way enum members are in source order.
        let sub = arena.alloc(object_type(&[("z", string, false), ("a", number, false)]));
        let sup = arena.alloc(object_type(&[("a", number, false), ("z", string, false)]));

        assert!(is_subtype(&arena, sub, sup));
        assert!(is_subtype(&arena, sup, sub));
    }

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

        let sub = arena.alloc(Type::Function(FunctionType {
            params: vec![Param::required(arena.unknown())],
            return_type: arena.number(),
            is_untyped: false,
        }));

        let sup = arena.alloc(Type::Function(FunctionType {
            params: vec![Param::required(arena.string())],
            return_type: arena.unknown(),
            is_untyped: false,
        }));

        assert!(is_subtype(&arena, sub, sup));
        assert!(!is_subtype(&arena, sup, sub));
    }

    #[test]
    fn optional_param_lets_sub_accept_fewer_required_args() {
        use crate::types::FunctionType;

        let mut arena = TypeArena::new();

        let sub = arena.alloc(Type::Function(FunctionType {
            params: vec![
                Param::required(arena.number()),
                Param {
                    type_id: arena.number(),
                    optional: true,
                    rest: false,
                    name: None,
                },
            ],
            return_type: arena.undefined(),
            is_untyped: false,
        }));

        let sup = arena.alloc(Type::Function(FunctionType {
            params: vec![Param::required(arena.number())],
            return_type: arena.undefined(),
            is_untyped: false,
        }));

        assert!(is_subtype(&arena, sub, sup));
    }

    #[test]
    fn function_requiring_more_args_than_target_guarantees_is_not_a_subtype() {
        use crate::types::FunctionType;

        let mut arena = TypeArena::new();

        let needs_two = arena.alloc(Type::Function(FunctionType {
            params: vec![
                Param::required(arena.number()),
                Param::required(arena.number()),
            ],
            return_type: arena.undefined(),
            is_untyped: false,
        }));

        let needs_one = arena.alloc(Type::Function(FunctionType {
            params: vec![Param::required(arena.number())],
            return_type: arena.undefined(),
            is_untyped: false,
        }));

        assert!(!is_subtype(&arena, needs_two, needs_one));

        assert!(is_subtype(&arena, needs_one, needs_two));
    }

    #[test]
    fn rest_param_lets_sub_accept_any_number_of_trailing_args() {
        use crate::types::FunctionType;

        let mut arena = TypeArena::new();
        let number_array = arena.alloc(Type::Array(arena.number()));

        let sub = arena.alloc(Type::Function(FunctionType {
            params: vec![Param {
                type_id: number_array,
                optional: false,
                rest: true,
                name: None,
            }],
            return_type: arena.undefined(),
            is_untyped: false,
        }));

        let sup = arena.alloc(Type::Function(FunctionType {
            params: vec![
                Param::required(arena.number()),
                Param::required(arena.number()),
            ],
            return_type: arena.undefined(),
            is_untyped: false,
        }));

        assert!(is_subtype(&arena, sub, sup));
    }

    fn object_type(props: &[(&str, TypeId, bool)]) -> Type {
        use crate::types::{ObjectType, PropertyEntry};

        let properties: Vec<PropertyEntry> = props
            .iter()
            .map(|&(name, type_id, optional)| PropertyEntry {
                name: name.into(),
                type_id,
                optional,
                is_method: false,
            })
            .collect();
        Type::Object(ObjectType::new(properties))
    }

    #[test]
    fn object_with_matching_properties_is_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[
            ("name", arena.string(), false),
            ("age", arena.number(), false),
        ]));
        let sup = arena.alloc(object_type(&[
            ("name", arena.string(), false),
            ("age", arena.number(), false),
        ]));
        assert!(is_subtype(&arena, sub, sup));
    }

    #[test]
    fn object_with_extra_property_is_still_subtype() {
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
        let sup = arena.alloc(object_type(&[
            ("name", arena.string(), false),
            ("age", arena.number(), false),
        ]));
        assert!(!is_subtype(&arena, sub, sup));
    }

    #[test]
    fn object_missing_an_optional_property_is_still_subtype() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[("name", arena.string(), false)]));
        let sup = arena.alloc(object_type(&[
            ("name", arena.string(), false),
            ("age", arena.number(), true),
        ]));
        assert!(is_subtype(&arena, sub, sup));
    }

    #[test]
    fn optional_property_is_not_subtype_of_required_property() {
        let mut arena = TypeArena::new();
        let sub = arena.alloc(object_type(&[("value", arena.number(), true)]));
        let sup = arena.alloc(object_type(&[("value", arena.number(), false)]));
        assert!(!is_subtype(&arena, sub, sup));
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

        assert!(is_subtype(&arena, sub_union, arena.unknown()));

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
        let members = match arena.get(outer) {
            Type::Union(members) => members,
            _ => panic!("expected a union"),
        };
        assert_eq!(members.len(), 3);
    }

    #[test]
    fn union_dedupes_equal_literals_even_with_different_type_ids() {
        let mut arena = TypeArena::new();
        let a1 = arena.alloc(Type::StringLiteral("a".to_string()));
        let a2 = arena.alloc(Type::StringLiteral("a".to_string()));
        assert_ne!(
            a1, a2,
            "test setup: these must be genuinely different TypeIds"
        );

        let result = arena.alloc_union(vec![a1, a2]);

        assert!(
            result == a1 || result == a2,
            "expected exactly one of the two equal literals to survive"
        );
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
