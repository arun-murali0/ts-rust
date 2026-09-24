use std::rc::Rc;

use crate::arena::{TypeArena, TypeId};

// The derived PartialEq compares every TypeId field by its raw arena slot, so two
// composites (Object, Function, Array) that have the same shape but were allocated
// at different times compare as unequal, and only literals and the fixed
// primitives compare the way their content suggests. Do not use `==` on a Type to
// ask "are these the same type"; use TypeArena::structurally_equal, which follows
// TypeIds through the arena.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Number,
    String,
    Boolean,
    Null,
    Undefined,

    Any,

    Unknown,

    Error,
    Function(FunctionType),
    Object(ObjectType),

    Array(TypeId),

    Union(Vec<TypeId>),
    StringLiteral(String),

    NumberLiteral(f64),
    BooleanLiteral(bool),

    Never,

    // The Option is the parameter's own `extends` bound (`T extends { length:
    // number }`), resolved once when the placeholder is first created and reused
    // from the same TypeParameterId cache as everything else about this
    // parameter. `None` means fully unconstrained.
    GenericParameter(TypeParameterId, String, Option<TypeId>),
}

// Identifies a declared type parameter by where it is written in source, not by an
// arena slot and not by an AST pointer. Two distinct resolutions of the same declared
// `T` (once while resolving a function's signature, again while checking its body)
// must agree on this value so both reuse the same `TypeId` for it. A source-derived
// key is stable across separate parses of unchanged source; a pointer or arena slot
// is only guaranteed stable within the one parse/allocation that produced it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeParameterId {
    declaration_span_start: u32,
    parameter_index: u32,
}

impl TypeParameterId {
    pub fn new(declaration_span_start: u32, parameter_index: u32) -> Self {
        Self {
            declaration_span_start,
            parameter_index,
        }
    }

    // The position of this parameter within its own declaration's type parameter
    // list (0 for the first `<T, ...>`, 1 for the second, and so on). Exposed so
    // an explicit call-site type argument list, which has no declaration_span of
    // its own to key off of, can still be zipped positionally against the
    // parameters found in a function's structural type -- see
    // generics::ordered_generic_param_ids.
    pub fn parameter_index(&self) -> u32 {
        self.parameter_index
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionType {
    pub params: Vec<Param>,
    pub return_type: TypeId,

    pub is_untyped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub type_id: TypeId,
    pub optional: bool,

    pub rest: bool,

    // Kept for diagnostics only -- naming a missing argument -- so structural
    // equality ignores it; `(x: number) => void` and `(y: number) => void`
    // stay the same type.
    pub name: Option<Rc<str>>,
}

impl Param {
    #[allow(dead_code)]
    pub fn required(type_id: TypeId) -> Self {
        Self {
            type_id,
            optional: false,
            rest: false,
            name: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectType {
    // Always sorted by name. subtyping::object_is_subtype walks two of these in
    // step (a merge-join, not a lookup per property) and TypeArena::structurally_equal
    // compares them pairwise, and both are only correct on sorted input. Build one
    // with ObjectType::new, which sorts, and never assemble the struct by hand.
    pub properties: Vec<PropertyEntry>,
}

impl ObjectType {
    // The only supported way to construct an ObjectType. Sorting an already sorted
    // list is a single linear pass, so callers that happen to have sorted input
    // pay almost nothing for not having to know that.
    pub fn new(mut properties: Vec<PropertyEntry>) -> Self {
        properties.sort_by(|a, b| a.name.cmp(&b.name));
        Self { properties }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyEntry {
    // `Rc<str>` rather than `String`: class inheritance resolution clones the whole
    // accumulated property list once per level of an inheritance chain (see
    // `resolve_class` in namespace.rs), so cloning a `PropertyEntry` is on the hot
    // path for any class hierarchy of meaningful depth. An `Rc<str>` clone is a
    // refcount bump; a `String` clone is a heap allocation and byte copy every time.
    pub name: Rc<str>,
    pub type_id: TypeId,
    pub optional: bool,
}

pub fn widen(arena: &TypeArena, type_id: TypeId) -> TypeId {
    match arena.get(type_id) {
        Type::StringLiteral(_) => arena.string(),
        Type::NumberLiteral(_) => arena.number(),
        Type::BooleanLiteral(_) => arena.boolean(),
        _ => type_id,
    }
}
