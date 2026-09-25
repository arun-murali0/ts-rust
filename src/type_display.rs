use crate::arena::{TypeArena, TypeId};
use crate::types::Type;

// "value is not assignable" told you nothing was wrong you didn't already
// suspect. tsc's own wording ("'number' is not assignable to 'string'") is
// useful because it names the two types. This module is what lets our
// messages do the same: turn a TypeId back into the source-like text tsc
// would print for it.
//
// A cyclic type would recurse here forever. Nothing today builds one --
// namespace::resolve reports a self-reference as Circular instead of looping
// -- but a printer has no way to know that stays true, so depth is capped.
const MAX_DEPTH: usize = 32;

pub fn display_type(arena: &TypeArena, type_id: TypeId) -> String {
    let mut out = String::new();
    write_type(arena, type_id, &mut out, MAX_DEPTH);
    out
}

fn write_type(arena: &TypeArena, type_id: TypeId, out: &mut String, depth: usize) {
    if depth == 0 {
        out.push_str("...");
        return;
    }

    // A named interface, class, alias, generic instantiation or enum prints
    // its name instead of unfolding its shape -- `Dog`, not
    // `{ name: string; breed: string }`. Checked before the structural match
    // below so it applies uniformly, whatever the underlying Type is.
    if let Some(name) = arena.display_name(type_id) {
        out.push_str(name);
        return;
    }

    match arena.get(type_id) {
        Type::Number => out.push_str("number"),
        Type::String => out.push_str("string"),
        Type::Boolean => out.push_str("boolean"),
        Type::Null => out.push_str("null"),
        Type::Undefined => out.push_str("undefined"),
        Type::Any => out.push_str("any"),
        Type::Unknown => out.push_str("unknown"),
        Type::Never => out.push_str("never"),
        Type::Void => out.push_str("void"),
        Type::Error => out.push_str("error"),

        Type::NumberLiteral(value) => {
            // Matches tsc: an integral literal like 5.0 prints as `5`.
            if value.fract() == 0.0 && value.is_finite() {
                out.push_str(&format!("{value:.0}"));
            } else {
                out.push_str(&value.to_string());
            }
        }
        Type::BooleanLiteral(value) => out.push_str(if *value { "true" } else { "false" }),
        Type::StringLiteral(value) => {
            out.push('"');
            for ch in value.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    _ => out.push(ch),
                }
            }
            out.push('"');
        }

        Type::Array(element) => {
            // `A | B[]` reads as `A | (B[])`, not what an array of a union means.
            let needs_parens = matches!(arena.get(*element), Type::Union(_));
            if needs_parens {
                out.push('(');
                write_type(arena, *element, out, depth - 1);
                out.push(')');
            } else {
                write_type(arena, *element, out, depth - 1);
            }
            out.push_str("[]");
        }

        Type::Union(members) => {
            for (index, &member) in members.iter().enumerate() {
                if index > 0 {
                    out.push_str(" | ");
                }
                write_type(arena, member, out, depth - 1);
            }
        }

        Type::Object(object) => {
            // Case study: `{ y: string, x: number }` as written prints as
            // `{ x: number; y: string }` here, because ObjectType keeps
            // properties sorted by name for subtyping's merge-join (see its
            // own doc comment). tsc would print declaration order. Not fixed
            // here, since fixing it means ObjectType stops being sorted, and
            // subtyping is not something to touch to make a message prettier.
            if object.properties.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{ ");
            for (index, property) in object.properties.iter().enumerate() {
                if index > 0 {
                    out.push_str("; ");
                }
                out.push_str(&property.name);
                if property.optional {
                    out.push('?');
                }
                out.push_str(": ");
                write_type(arena, property.type_id, out, depth - 1);
            }
            out.push_str(" }");
        }

        Type::Function(function) => {
            out.push('(');
            for (index, param) in function.params.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                match &param.name {
                    Some(name) => out.push_str(name),
                    // Every resolved parameter has a name today (see Param's
                    // own doc comment); this only guards against a future
                    // caller that doesn't, so the printer degrades instead of
                    // panicking.
                    None => out.push_str(&format!("arg{index}")),
                }
                if param.optional && !param.rest {
                    out.push('?');
                }
                out.push_str(": ");
                if param.rest {
                    out.push_str("...");
                }
                write_type(arena, param.type_id, out, depth - 1);
            }
            out.push_str(") => ");
            write_type(arena, function.return_type, out, depth - 1);
        }

        Type::GenericParameter(_, name, _) => out.push_str(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionType, ObjectType, Param, PropertyEntry, TypeParameterId};

    fn render(arena: &TypeArena, id: TypeId) -> String {
        display_type(arena, id)
    }

    #[test]
    fn primitives() {
        let arena = TypeArena::new();
        assert_eq!(render(&arena, arena.number()), "number");
        assert_eq!(render(&arena, arena.string()), "string");
        assert_eq!(render(&arena, arena.boolean()), "boolean");
        assert_eq!(render(&arena, arena.null()), "null");
        assert_eq!(render(&arena, arena.undefined()), "undefined");
        assert_eq!(render(&arena, arena.any()), "any");
        assert_eq!(render(&arena, arena.unknown()), "unknown");
        assert_eq!(render(&arena, arena.never()), "never");
        assert_eq!(render(&arena, arena.void()), "void");
        assert_eq!(render(&arena, arena.error()), "error");
    }

    #[test]
    fn literals() {
        let mut arena = TypeArena::new();
        let s = arena.alloc(Type::StringLiteral("left".to_string()));
        assert_eq!(render(&arena, s), "\"left\"");

        let n = arena.alloc(Type::NumberLiteral(5.0));
        assert_eq!(render(&arena, n), "5");

        let frac = arena.alloc(Type::NumberLiteral(5.5));
        assert_eq!(render(&arena, frac), "5.5");

        let b = arena.alloc(Type::BooleanLiteral(true));
        assert_eq!(render(&arena, b), "true");
    }

    #[test]
    fn string_literal_escapes_quotes_and_backslashes() {
        let mut arena = TypeArena::new();
        let s = arena.alloc(Type::StringLiteral("a\"b\\c".to_string()));
        assert_eq!(render(&arena, s), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn array_of_primitive() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let arr = arena.alloc(Type::Array(number));
        assert_eq!(render(&arena, arr), "number[]");
    }

    #[test]
    fn array_of_union_gets_parens() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        // Built directly rather than via alloc_union: alloc_union flattens and
        // dedups by popping a stack, which reverses member order. That reversal
        // is alloc_union's own behavior, not something display_type should
        // second-guess -- it prints members in whatever order the Union holds
        // them.
        let union = arena.alloc(Type::Union(vec![number, string]));
        let arr = arena.alloc(Type::Array(union));
        assert_eq!(render(&arena, arr), "(number | string)[]");
    }

    #[test]
    fn union_of_primitives() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        // See array_of_union_gets_parens on why this bypasses alloc_union.
        let union = arena.alloc(Type::Union(vec![number, string]));
        assert_eq!(render(&arena, union), "number | string");
    }

    #[test]
    fn empty_object() {
        let mut arena = TypeArena::new();
        let obj = arena.alloc(Type::Object(ObjectType::new(vec![])));
        assert_eq!(render(&arena, obj), "{}");
    }

    #[test]
    fn object_with_properties_prints_sorted_and_marks_optional() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let obj = arena.alloc(Type::Object(ObjectType::new(vec![
            PropertyEntry {
                name: "y".into(),
                type_id: string,
                optional: true,
                is_method: false,
            },
            PropertyEntry {
                name: "x".into(),
                type_id: number,
                optional: false,
                is_method: false,
            },
        ])));
        // ObjectType::new sorts by name, so x comes before y regardless of
        // construction order.
        assert_eq!(render(&arena, obj), "{ x: number; y?: string }");
    }

    #[test]
    fn nested_object_in_property() {
        let mut arena = TypeArena::new();
        let string = arena.string();
        let inner = arena.alloc(Type::Object(ObjectType::new(vec![PropertyEntry {
            name: "name".into(),
            type_id: string,
            optional: false,
            is_method: false,
        }])));
        let arr = arena.alloc(Type::Array(inner));
        let outer = arena.alloc(Type::Object(ObjectType::new(vec![PropertyEntry {
            name: "items".into(),
            type_id: arr,
            optional: false,
            is_method: false,
        }])));
        assert_eq!(render(&arena, outer), "{ items: { name: string }[] }");
    }

    #[test]
    fn function_with_named_params() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let func = arena.alloc(Type::Function(FunctionType {
            params: vec![
                Param {
                    type_id: number,
                    optional: false,
                    rest: false,
                    name: Some("a".into()),
                },
                Param {
                    type_id: string,
                    optional: true,
                    rest: false,
                    name: Some("b".into()),
                },
            ],
            return_type: number,
            is_untyped: false,
        }));
        assert_eq!(render(&arena, func), "(a: number, b?: string) => number");
    }

    #[test]
    fn function_with_rest_param() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let arr = arena.alloc(Type::Array(number));
        let void_like = arena.undefined();
        let func = arena.alloc(Type::Function(FunctionType {
            params: vec![Param {
                type_id: arr,
                optional: false,
                rest: true,
                name: Some("rest".into()),
            }],
            return_type: void_like,
            is_untyped: false,
        }));
        assert_eq!(render(&arena, func), "(rest: ...number[]) => undefined");
    }

    #[test]
    fn function_with_unnamed_param_falls_back_positionally() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let func = arena.alloc(Type::Function(FunctionType {
            params: vec![Param {
                type_id: number,
                optional: false,
                rest: false,
                name: None,
            }],
            return_type: number,
            is_untyped: false,
        }));
        assert_eq!(render(&arena, func), "(arg0: number) => number");
    }

    #[test]
    fn generic_parameter_prints_its_name() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let param = arena.alloc(Type::GenericParameter(id, "T".to_string(), None));
        assert_eq!(render(&arena, param), "T");
    }

    #[test]
    fn a_named_type_prints_its_name_instead_of_its_shape() {
        let mut arena = TypeArena::new();
        let string = arena.string();
        let dog = arena.alloc(Type::Object(ObjectType::new(vec![PropertyEntry {
            name: "name".into(),
            type_id: string,
            optional: false,
            is_method: false,
        }])));
        arena.set_display_name(dog, "Dog");
        assert_eq!(render(&arena, dog), "Dog");
    }

    #[test]
    fn a_named_type_nested_in_another_type_still_prints_its_name() {
        let mut arena = TypeArena::new();
        let string = arena.string();
        let dog = arena.alloc(Type::Object(ObjectType::new(vec![PropertyEntry {
            name: "name".into(),
            type_id: string,
            optional: false,
            is_method: false,
        }])));
        arena.set_display_name(dog, "Dog");
        let dogs = arena.alloc(Type::Array(dog));
        assert_eq!(render(&arena, dogs), "Dog[]");
    }

    #[test]
    fn depth_limit_does_not_hang_on_a_deep_type() {
        let mut arena = TypeArena::new();
        let mut current = arena.number();
        for _ in 0..(MAX_DEPTH + 5) {
            current = arena.alloc(Type::Array(current));
        }
        // Should terminate and end with the depth-limit marker, not hang or panic.
        let rendered = render(&arena, current);
        assert!(
            rendered.ends_with("...[]") || rendered.contains("..."),
            "{rendered}"
        );
    }
}
