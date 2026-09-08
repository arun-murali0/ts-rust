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

    GenericParameter(String),
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
    pub name: String,
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
