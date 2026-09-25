use crate::fxhash::FxHashMap;
use crate::types::Type;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(u32);

pub struct TypeArena {
    types: Vec<Type>,

    // How a TypeId should read in a diagnostic, when it's not just its
    // structural shape: an interface, class or alias by its declared name
    // ("Dog"), a generic instantiation with its arguments ("Box<number>"), an
    // enum by its name ("Weird"). Keyed by TypeId rather than carried on Type
    // itself, so display is a side concern display_type can consult, not
    // something every match arm over Type has to thread through. Safe because
    // alloc() never reuses a TypeId for a different value (see its own doc
    // comment) -- one TypeId always means one thing for the life of this arena.
    display_names: FxHashMap<TypeId, String>,
}

impl TypeArena {
    // The nine primitives are allocated in this fixed order so their ids are known
    // constants below (number(), string(), and so on). Any part of the checker that
    // needs "the number type" calls arena.number() directly instead of having to
    // thread a TypeId through from wherever that primitive was first resolved.
    pub fn new() -> Self {
        let mut arena = Self {
            types: Vec::new(),
            display_names: FxHashMap::default(),
        };

        arena.alloc(Type::Number);
        arena.alloc(Type::String);
        arena.alloc(Type::Boolean);
        arena.alloc(Type::Null);
        arena.alloc(Type::Undefined);
        arena.alloc(Type::Any);
        arena.alloc(Type::Unknown);
        arena.alloc(Type::Error);
        arena.alloc(Type::Never);
        arena.alloc(Type::Void);
        arena
    }

    // Always pushes a new slot, even for a Type value equal to one already stored.
    // Two structurally identical Type values can still be semantically distinct
    // (see TypeParameterId in types.rs, whose identity depends on exactly this), so
    // the arena never tries to deduplicate types on the caller's behalf.
    pub fn alloc(&mut self, ty: Type) -> TypeId {
        self.types.push(ty);
        TypeId((self.types.len() - 1) as u32)
    }

    // Flattens nested unions and drops never members, since never contributes
    // nothing to what a union can hold. A one-member result collapses to that
    // member directly rather than a redundant single-member Type::Union, so
    // callers get a plain type back instead of having to unwrap a trivial union.
    pub fn alloc_union(&mut self, members: Vec<TypeId>) -> TypeId {
        let mut flat: Vec<TypeId> = Vec::with_capacity(members.len());
        let mut queue = members;
        while let Some(id) = queue.pop() {
            match self.get(id) {
                Type::Union(nested) => queue.extend(nested.iter().copied()),
                Type::Never => {}
                _ => {
                    let already_present = flat
                        .iter()
                        .any(|&existing| self.structurally_equal(existing, id));
                    if !already_present {
                        flat.push(id);
                    }
                }
            }
        }

        match flat.len() {
            0 => self.never(),
            1 => flat[0],
            _ => self.alloc(Type::Union(flat)),
        }
    }

    pub fn get(&self, id: TypeId) -> &Type {
        &self.types[id.0 as usize]
    }

    // Registers how type_id should print. A later call for the same TypeId
    // replaces the earlier name rather than erroring, since a generic's own
    // cached shape and a specific instantiation of it are sometimes the exact
    // same TypeId (see namespace::resolve's own comment on this) and the more
    // specific caller should win.
    pub fn set_display_name(&mut self, type_id: TypeId, name: impl Into<String>) {
        self.display_names.insert(type_id, name.into());
    }

    pub fn display_name(&self, type_id: TypeId) -> Option<&str> {
        self.display_names.get(&type_id).map(String::as_str)
    }

    // Whether two types are the same type by shape, not by arena slot.
    //
    // The derived PartialEq on Type cannot answer this: it compares a composite's
    // TypeId fields by raw slot, so `{ a: { b: number } }` written at two sites,
    // whose inner objects sit at different slots, compare as unequal even though
    // nothing distinguishes them. This follows TypeIds through the arena instead.
    //
    // Terminates because a Type can only refer to slots allocated before it (the
    // arena is append-only and a type is built from ids that already exist), so the
    // graph of types is acyclic and every recursion below strictly descends.
    //
    // Identity is the one thing shape must not override. A GenericParameter is
    // equal only to the same declared parameter (its TypeParameterId), never to
    // another `T` that merely has the same name or bound.
    pub fn structurally_equal(&self, a: TypeId, b: TypeId) -> bool {
        if a == b {
            return true;
        }

        match (self.get(a), self.get(b)) {
            (Type::Number, Type::Number)
            | (Type::String, Type::String)
            | (Type::Boolean, Type::Boolean)
            | (Type::Null, Type::Null)
            | (Type::Undefined, Type::Undefined)
            | (Type::Any, Type::Any)
            | (Type::Unknown, Type::Unknown)
            | (Type::Error, Type::Error)
            | (Type::Never, Type::Never)
            | (Type::Void, Type::Void) => true,

            (Type::StringLiteral(x), Type::StringLiteral(y)) => x == y,
            (Type::NumberLiteral(x), Type::NumberLiteral(y)) => x == y,
            (Type::BooleanLiteral(x), Type::BooleanLiteral(y)) => x == y,

            (Type::Array(x), Type::Array(y)) => self.structurally_equal(*x, *y),

            (Type::Function(f), Type::Function(g)) => {
                f.is_untyped == g.is_untyped
                    && f.params.len() == g.params.len()
                    && f.params.iter().zip(&g.params).all(|(p, q)| {
                        p.optional == q.optional
                            && p.rest == q.rest
                            && self.structurally_equal(p.type_id, q.type_id)
                    })
                    && self.structurally_equal(f.return_type, g.return_type)
            }

            // Pairwise, which is only right because ObjectType keeps its
            // properties sorted by name.
            (Type::Object(x), Type::Object(y)) => {
                x.properties.len() == y.properties.len()
                    && x.properties.iter().zip(&y.properties).all(|(p, q)| {
                        p.name == q.name
                            && p.optional == q.optional
                            && self.structurally_equal(p.type_id, q.type_id)
                    })
            }

            // A union is a set: member order carries no meaning. Union members are
            // already deduplicated by alloc_union, so equal length plus every
            // member of one having a match in the other is set equality.
            (Type::Union(xs), Type::Union(ys)) => {
                xs.len() == ys.len()
                    && xs
                        .iter()
                        .all(|&x| ys.iter().any(|&y| self.structurally_equal(x, y)))
            }

            (Type::GenericParameter(x, _, _), Type::GenericParameter(y, _, _)) => x == y,

            _ => false,
        }
    }

    // Fixed slots assigned in new(). These never change for the lifetime of an arena.
    pub fn number(&self) -> TypeId {
        TypeId(0)
    }
    pub fn string(&self) -> TypeId {
        TypeId(1)
    }
    pub fn boolean(&self) -> TypeId {
        TypeId(2)
    }
    pub fn null(&self) -> TypeId {
        TypeId(3)
    }
    pub fn undefined(&self) -> TypeId {
        TypeId(4)
    }
    pub fn any(&self) -> TypeId {
        TypeId(5)
    }
    pub fn unknown(&self) -> TypeId {
        TypeId(6)
    }

    pub fn error(&self) -> TypeId {
        TypeId(7)
    }

    pub fn never(&self) -> TypeId {
        TypeId(8)
    }

    pub fn void(&self) -> TypeId {
        TypeId(9)
    }
}

impl Default for TypeArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionType, ObjectType, Param, PropertyEntry, TypeParameterId};

    fn property(name: &str, type_id: TypeId, optional: bool) -> PropertyEntry {
        PropertyEntry {
            name: name.into(),
            type_id,
            optional,
            is_method: false,
        }
    }

    fn object(arena: &mut TypeArena, properties: Vec<PropertyEntry>) -> TypeId {
        arena.alloc(Type::Object(ObjectType::new(properties)))
    }

    // `{ a: { b: number } }`, built from scratch each call so every call lands in
    // fresh arena slots.
    fn nested(arena: &mut TypeArena) -> TypeId {
        let number = arena.number();
        let inner = object(arena, vec![property("b", number, false)]);
        object(arena, vec![property("a", inner, false)])
    }

    #[test]
    fn derived_equality_cannot_see_through_a_slot_but_structural_equality_can() {
        let mut arena = TypeArena::new();
        let first = nested(&mut arena);
        let second = nested(&mut arena);

        assert_ne!(first, second, "separately allocated, so different slots");
        assert_ne!(
            arena.get(first),
            arena.get(second),
            "this is the gap: the derived == compares the inner object's slot"
        );
        assert!(arena.structurally_equal(first, second));
    }

    #[test]
    fn union_collapses_structurally_identical_composites() {
        let mut arena = TypeArena::new();
        let first = nested(&mut arena);
        let second = nested(&mut arena);

        let union = arena.alloc_union(vec![first, second]);

        assert!(
            matches!(arena.get(union), Type::Object(_)),
            "two identical shapes must collapse to one member, not stay a two-member union"
        );
    }

    #[test]
    fn union_keeps_composites_that_differ() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let a = object(&mut arena, vec![property("a", number, false)]);
        let b = object(&mut arena, vec![property("a", string, false)]);

        let union = arena.alloc_union(vec![a, b]);

        assert!(matches!(arena.get(union), Type::Union(members) if members.len() == 2));
    }

    #[test]
    fn objects_that_differ_only_in_optionality_or_name_are_not_equal() {
        let mut arena = TypeArena::new();
        let number = arena.number();
        let required = object(&mut arena, vec![property("a", number, false)]);
        let optional = object(&mut arena, vec![property("a", number, true)]);
        let renamed = object(&mut arena, vec![property("b", number, false)]);

        assert!(!arena.structurally_equal(required, optional));
        assert!(!arena.structurally_equal(required, renamed));
    }

    #[test]
    fn object_property_order_at_construction_does_not_matter() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let one = object(
            &mut arena,
            vec![property("z", string, false), property("a", number, false)],
        );
        let two = object(
            &mut arena,
            vec![property("a", number, false), property("z", string, false)],
        );

        assert!(arena.structurally_equal(one, two));
    }

    #[test]
    fn arrays_compare_by_element_shape() {
        let mut arena = TypeArena::new();
        let first_element = nested(&mut arena);
        let second_element = nested(&mut arena);
        let first = arena.alloc(Type::Array(first_element));
        let second = arena.alloc(Type::Array(second_element));
        let numbers = arena.alloc(Type::Array(arena.number()));

        assert!(arena.structurally_equal(first, second));
        assert!(!arena.structurally_equal(first, numbers));
    }

    #[test]
    fn functions_compare_by_parameters_and_return_type() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let make = |arena: &mut TypeArena, param: TypeId, ret: TypeId, optional: bool| {
            arena.alloc(Type::Function(FunctionType {
                params: vec![Param {
                    type_id: param,
                    optional,
                    rest: false,
                    name: None,
                }],
                return_type: ret,
                is_untyped: false,
            }))
        };
        let base = make(&mut arena, number, string, false);
        let same = make(&mut arena, number, string, false);
        let other_param = make(&mut arena, string, string, false);
        let other_return = make(&mut arena, number, number, false);
        let optional_param = make(&mut arena, number, string, true);

        assert!(arena.structurally_equal(base, same));
        assert!(!arena.structurally_equal(base, other_param));
        assert!(!arena.structurally_equal(base, other_return));
        assert!(!arena.structurally_equal(base, optional_param));
    }

    #[test]
    fn union_equality_ignores_member_order() {
        let mut arena = TypeArena::new();
        let (number, string) = (arena.number(), arena.string());
        let one = arena.alloc_union(vec![number, string]);
        let two = arena.alloc_union(vec![string, number]);

        assert!(arena.structurally_equal(one, two));
    }

    #[test]
    fn generic_parameters_are_equal_only_by_declaration_identity() {
        let mut arena = TypeArena::new();
        let same_declaration = TypeParameterId::new(10, 0);
        let other_declaration = TypeParameterId::new(50, 0);

        let first = arena.alloc(Type::GenericParameter(same_declaration, "T".into(), None));
        let again = arena.alloc(Type::GenericParameter(same_declaration, "T".into(), None));
        let lookalike = arena.alloc(Type::GenericParameter(other_declaration, "T".into(), None));

        assert!(arena.structurally_equal(first, again));
        assert!(
            !arena.structurally_equal(first, lookalike),
            "another `T` with the same name is a different type parameter"
        );
    }

    #[test]
    fn object_type_new_sorts_its_properties() {
        let arena = TypeArena::new();
        let number = arena.number();
        let built = ObjectType::new(vec![
            property("c", number, false),
            property("a", number, false),
            property("b", number, false),
        ]);

        let names: Vec<&str> = built.properties.iter().map(|p| &*p.name).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }
}
