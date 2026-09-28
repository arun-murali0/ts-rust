use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::arena::{TypeArena, TypeId};

// PartialEq, Eq and Hash on Type are written by hand (below the enum) rather than
// derived, for one reason: NumberLiteral(f64). f64 has no total Eq or Hash (NaN is
// not equal to itself), but TypeArena::alloc uses a Type as a hash-map key to
// reuse the TypeId of an identical composite. NumberLiteral is therefore compared
// and hashed by f64::to_bits(), so `==` and `hash` always agree with each other.
//
// Both are SHALLOW: a composite's TypeId fields are compared and hashed as opaque
// ids, never followed through the arena. That is what makes the intern table safe
// for self-referential types (two still-empty placeholders have different ids, so
// they never look alike) and it is only sound because every child id was itself
// obtained from the arena. A consequence: `==` on a Type still cannot answer "are
// these the same type by shape" for composites whose children are different ids.
// Use TypeArena::structurally_equal for that; it follows TypeIds through the arena.
#[derive(Clone, Debug)]
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

    // A function with no meaningful return value (`function f(): void {}`).
    // Distinct from Undefined: undefined is assignable to void (an implicit or
    // bare `return;` satisfies a void return type), but void is not assignable
    // back to undefined or anything else concrete -- see subtyping's own note on
    // this. Real TypeScript also lets a *function type* whose return is void
    // accept an implementation that returns something else, e.g. assigning
    // `() => number` where `() => void` is expected; that call-site leniency
    // isn't implemented, only the plain assignability rule above.
    Void,

    // The Option is the parameter's own `extends` bound (`T extends { length:
    // number }`), resolved once when the placeholder is first created and reused
    // from the same TypeParameterId cache as everything else about this
    // parameter. `None` means fully unconstrained.
    GenericParameter(TypeParameterId, String, Option<TypeId>),
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
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

            (Type::Function(a), Type::Function(b)) => a == b,
            (Type::Object(a), Type::Object(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => a == b,
            (Type::Union(a), Type::Union(b)) => a == b,
            (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
            // Bit pattern, not IEEE ==: keeps this consistent with Hash below, and
            // makes 0.0 and -0.0 distinct while a NaN literal equals itself.
            (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a.to_bits() == b.to_bits(),
            (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
            (
                Type::GenericParameter(id_a, name_a, bound_a),
                Type::GenericParameter(id_b, name_b, bound_b),
            ) => id_a == id_b && name_a == name_b && bound_a == bound_b,

            // Different variants. Every variant is named here on purpose, with no
            // wildcard, so adding a Type variant is a compile error until it is
            // handled above instead of silently comparing unequal to itself.
            (
                Type::Number
                | Type::String
                | Type::Boolean
                | Type::Null
                | Type::Undefined
                | Type::Any
                | Type::Unknown
                | Type::Error
                | Type::Never
                | Type::Void
                | Type::Function(_)
                | Type::Object(_)
                | Type::Array(_)
                | Type::Union(_)
                | Type::StringLiteral(_)
                | Type::NumberLiteral(_)
                | Type::BooleanLiteral(_)
                | Type::GenericParameter(..),
                _,
            ) => false,
        }
    }
}

impl Eq for Type {}

impl Hash for Type {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::Null
            | Type::Undefined
            | Type::Any
            | Type::Unknown
            | Type::Error
            | Type::Never
            | Type::Void => {}

            Type::Function(function) => function.hash(state),
            Type::Object(object) => object.hash(state),
            Type::Array(element) => element.hash(state),
            // Order-sensitive, matching the derived Vec equality above. Unions are
            // not keyed by Type in the arena's intern table (alloc_union interns
            // them by their finished member list instead), so this exists only to
            // keep Hash total.
            Type::Union(members) => members.hash(state),
            Type::StringLiteral(text) => text.hash(state),
            Type::NumberLiteral(value) => value.to_bits().hash(state),
            Type::BooleanLiteral(value) => value.hash(state),
            Type::GenericParameter(id, name, bound) => {
                id.hash(state);
                name.hash(state);
                bound.hash(state);
            }
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FunctionType {
    pub params: Vec<Param>,
    pub return_type: TypeId,

    pub is_untyped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PropertyEntry {
    // `Rc<str>` rather than `String`: class inheritance resolution clones the whole
    // accumulated property list once per level of an inheritance chain (see
    // `resolve_class` in namespace.rs), so cloning a `PropertyEntry` is on the hot
    // path for any class hierarchy of meaningful depth. An `Rc<str>` clone is a
    // refcount bump; a `String` clone is a heap allocation and byte copy every time.
    pub name: Rc<str>,
    pub type_id: TypeId,
    pub optional: bool,

    // Declared with method syntax (`get(): T` in an interface, or a class method)
    // rather than as a function-typed property (`get: () => T`). tsc compares the
    // parameters of a method bivariantly even under strictFunctionTypes, and the
    // ones of a function-typed property contravariantly, and which rule applies
    // follows the *target* property's declaration (see subtyping::object_is_subtype).
    pub is_method: bool,
}

pub fn widen(arena: &TypeArena, type_id: TypeId) -> TypeId {
    match arena.get(type_id) {
        Type::StringLiteral(_) => arena.string(),
        Type::NumberLiteral(_) => arena.number(),
        Type::BooleanLiteral(_) => arena.boolean(),
        _ => type_id,
    }
}
