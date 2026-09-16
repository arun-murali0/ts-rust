use crate::types::Type;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(u32);

pub struct TypeArena {
    types: Vec<Type>,
}

impl TypeArena {
    // The nine primitives are allocated in this fixed order so their ids are known
    // constants below (number(), string(), and so on). Any part of the checker that
    // needs "the number type" calls arena.number() directly instead of having to
    // thread a TypeId through from wherever that primitive was first resolved.
    pub fn new() -> Self {
        let mut arena = Self { types: Vec::new() };

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
                        .any(|&existing| self.get(existing) == self.get(id));
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
}

impl Default for TypeArena {
    fn default() -> Self {
        Self::new()
    }
}
