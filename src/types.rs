//! The type vocabulary. V1 covered primitives and plain function
//! signatures. V2 added structural objects, arrays, and unions. V3a adds
//! literal types. Generics arrive in a later version (see
//! `docs/ROADMAP.md`).

use crate::arena::{TypeArena, TypeId};

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Number,
    String,
    Boolean,
    Null,
    Undefined,
    /// Assignable to and from anything. TypeScript's opt-out of checking.
    Any,
    /// TypeScript's `unknown`: anything is assignable to it, but it is not
    /// assignable to anything else without a narrowing check. Only produced
    /// from an explicit `unknown` annotation, never used as an internal
    /// "couldn't infer this" fallback (see `Type::Error`).
    Unknown,
    /// Internal-only sentinel meaning "a diagnostic was already raised for
    /// this expression." Bidirectionally compatible with everything so one
    /// root-cause error doesn't produce a chain of unrelated mismatches.
    Error,
    Function(FunctionType),
    Object(ObjectType),
    /// `T[]`. A single element type, covariant in subtyping. This matches
    /// real TypeScript behavior (technically unsound under mutation, but
    /// V1/V2 don't model mutation-site variance; see `docs/DESIGN.md`).
    Array(TypeId),
    /// A finite set of alternative types. Assignability is all-of on the
    /// `sub` side, any-of on the `sup` side. See `subtyping.rs`.
    Union(Vec<TypeId>),
    StringLiteral(String),
    /// TS numeric literals are always finite, written directly in source —
    /// no NaN, so plain f64 equality is safe here.
    NumberLiteral(f64),
    BooleanLiteral(bool),
    /// The bottom type. See `TypeArena::never` for when this shows up.
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionType {
    pub params: Vec<TypeId>,
    pub return_type: TypeId,
    /// True when this signature exists but couldn't be fully resolved
    /// (currently: a class constructor with at least one untyped
    /// parameter). Lets a caller distinguish that from "genuinely zero
    /// parameters" without re-deriving the answer from the AST during
    /// Pass 2. A regular function never reaches this struct with
    /// `is_untyped: true`: declare.rs skips registering it entirely if
    /// any parameter or the return type doesn't resolve, so its call
    /// sites go unchecked rather than checked-with-a-flag. Constructors
    /// are the one case that couldn't take that same route, since a
    /// class's instance type still needs to exist even when its
    /// constructor doesn't fully resolve.
    pub is_untyped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectType {
    /// Sorted by `name` at construction time (see
    /// `type_annotation::resolve_object_type`), never re-sorted later. This
    /// lets `subtyping.rs` compare two objects with a single merge-join
    /// pass instead of an O(n times m) `.find()` per property.
    pub properties: Vec<PropertyEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyEntry {
    pub name: String,
    pub type_id: TypeId,
    pub optional: bool,
}

/// A literal type widens to its base primitive; anything else is returned
/// unchanged. Used wherever TypeScript widens by default: array/object
/// literal members, and `let`/`var` (but not `const`) variable
/// declarations without an explicit annotation.
pub fn widen(arena: &TypeArena, type_id: TypeId) -> TypeId {
    match arena.get(type_id) {
        Type::StringLiteral(_) => arena.string(),
        Type::NumberLiteral(_) => arena.number(),
        Type::BooleanLiteral(_) => arena.boolean(),
        _ => type_id,
    }
}
