use std::rc::Rc;

use crate::arena::{TypeArena, TypeId};

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

    GenericParameter(TypeParameterId, String),
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
}

impl Param {
    #[allow(dead_code)]
    pub fn required(type_id: TypeId) -> Self {
        Self {
            type_id,
            optional: false,
            rest: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectType {
    pub properties: Vec<PropertyEntry>,
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
