//! Flat, append-only storage for `Type` values.
//!
//! Types are referred to by a `Copy` handle (`TypeId`) rather than an owned
//! or boxed tree. This makes recursive type shapes (e.g. `Tree = { children:
//! Tree[] }`) trivial to represent and makes equality a `u32` compare
//! instead of structural recursion.

use crate::types::Type;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(u32);

pub struct TypeArena {
    types: Vec<Type>,
}

impl TypeArena {
    /// Pre-seeds the well-known primitive and sentinel types so callers can
    /// use `arena.number()`, `arena.error()`, etc. without allocating.
    pub fn new() -> Self {
        let mut arena = Self { types: Vec::new() };
        // Order must match the accessor methods below.
        arena.alloc(Type::Number);
        arena.alloc(Type::String);
        arena.alloc(Type::Boolean);
        arena.alloc(Type::Null);
        arena.alloc(Type::Undefined);
        arena.alloc(Type::Any);
        arena.alloc(Type::Unknown);
        arena.alloc(Type::Error);
        arena.alloc(Type::Never);
        arena
    }

    pub fn alloc(&mut self, ty: Type) -> TypeId {
        self.types.push(ty);
        TypeId((self.types.len() - 1) as u32)
    }

    /// Builds a union: flattens any nested unions, drops duplicate and
    /// `never` members, and collapses to a single type when only one
    /// member remains (or to `never` when none do). Narrowing rebuilds
    /// unions repeatedly as it filters branches, so this keeps them from
    /// accumulating nested or duplicate members over time.
    pub fn alloc_union(&mut self, members: Vec<TypeId>) -> TypeId {
        let mut flat: Vec<TypeId> = Vec::with_capacity(members.len());
        let mut queue = members;
        while let Some(id) = queue.pop() {
            match self.get(id) {
                Type::Union(nested) => queue.extend(nested.iter().copied()),
                Type::Never => {}
                _ => {
                    // Dedupe by value, not by TypeId. Two separately
                    // allocated `StringLiteral("a")`s get different
                    // TypeIds even though they're the same type, so
                    // comparing IDs directly let `"a" | "a"` through as a
                    // two-member union instead of collapsing to `"a"`.
                    // `Type` already derives `PartialEq` structurally, so
                    // this is just comparing what the IDs point to.
                    let already_present = flat.iter().any(|&existing| self.get(existing) == self.get(id));
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

    /// Sentinel type meaning "checking already failed here." Not the same
    /// thing as TypeScript's `unknown`. We hand this back when inference
    /// can't pin down a real type, e.g. an unresolved identifier or a
    /// callee that isn't callable, so the one root-cause diagnostic doesn't
    /// snowball into a pile of unrelated mismatch errors further down.
    /// `subtyping` treats it as compatible with everything, same as `any`.
    pub fn error(&self) -> TypeId {
        TypeId(7)
    }

    /// The bottom type: a subtype of everything, nothing is a subtype of
    /// it except itself. Produced when narrowing eliminates every member
    /// of a union (an exhaustive `typeof` chain, for instance).
    pub fn never(&self) -> TypeId {
        TypeId(8)
    }
}

impl Default for TypeArena {
    fn default() -> Self {
        Self::new()
    }
}
