use std::cmp::Ordering;

use crate::arena::{TypeArena, TypeId};
use crate::types::{ObjectType, Param, Type};

pub fn is_subtype(arena: &TypeArena, sub: TypeId, sup: TypeId) -> bool {
    if sub == sup {
        return true;
    }

    if is_universally_compatible(arena, sub) || is_universally_compatible(arena, sup) {
        return true;
    }

    match (arena.get(sub), arena.get(sup)) {
        (_, Type::Unknown) => true,

        (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
        (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
        (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::NumberLiteral(_), Type::Number) => true,
        (Type::BooleanLiteral(_), Type::Boolean) => true,

        (Type::Never, _) => true,

        (Type::Union(sub_members), _) => sub_members
            .iter()
            .all(|&member| is_subtype(arena, member, sup)),

        (_, Type::Union(sup_members)) => sup_members
            .iter()
            .any(|&member| is_subtype(arena, sub, member)),

        (Type::Function(a), Type::Function(b)) => function_is_subtype(arena, a, b),

        (Type::Array(a), Type::Array(b)) => is_subtype(arena, *a, *b),

        (Type::Object(a), Type::Object(b)) => object_is_subtype(arena, a, b),

        _ => false,
    }
}

fn function_is_subtype(
    arena: &TypeArena,
    sub: &crate::types::FunctionType,
    sup: &crate::types::FunctionType,
) -> bool {
    if required_param_count(&sub.params) > required_param_count(&sup.params) {
        return false;
    }

    let checked_positions = sub.params.len().max(sup.params.len());
    for position in 0..checked_positions {
        if let (Some(sub_param), Some(sup_param)) = (
            param_type_at(arena, &sub.params, position),
            param_type_at(arena, &sup.params, position),
        ) && !is_subtype(arena, sup_param, sub_param)
        {
            return false;
        }
    }

    is_subtype(arena, sub.return_type, sup.return_type)
}

fn required_param_count(params: &[Param]) -> usize {
    params.iter().filter(|p| !p.optional && !p.rest).count()
}

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

fn object_is_subtype(arena: &TypeArena, sub: &ObjectType, sup: &ObjectType) -> bool {
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

        let mut properties: Vec<PropertyEntry> = props
            .iter()
            .map(|&(name, type_id, optional)| PropertyEntry {
                name: name.to_string(),
                type_id,
                optional,
            })
            .collect();
        properties.sort_by(|a, b| a.name.cmp(&b.name));
        Type::Object(ObjectType { properties })
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
