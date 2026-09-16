use crate::arena::{TypeArena, TypeId};
use crate::subtyping;

/// Read-only semantic operations exposed to AST-facing checker code.
///
/// The query layer is intentionally small. It gives bridge features a stable
/// place to ask semantic questions without making the bridge depend on the
/// implementation details of individual type-system algorithms.
pub struct SemanticQueries<'a> {
    arena: &'a TypeArena,
}

impl<'a> SemanticQueries<'a> {
    pub(crate) fn new(arena: &'a TypeArena) -> Self {
        Self { arena }
    }

    /// Whether a value of type `source` can be used where `target` is
    /// expected. Currently identical to `is_subtype`: TypeScript's real
    /// assignability rules (excess property checks, `const` assertions,
    /// etc.) diverge from plain subtyping in cases ts-rust doesn't
    /// implement yet. `is_assignable` is kept as its own method so bridge
    /// code calls the concept it actually means, and so that divergence
    /// can be added here later without touching any call site.
    pub fn is_assignable(&self, source: TypeId, target: TypeId) -> bool {
        self.is_subtype(source, target)
    }

    pub fn is_subtype(&self, source: TypeId, target: TypeId) -> bool {
        subtyping::is_subtype(self.arena, source, target)
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticQueries;
    use crate::arena::TypeArena;

    #[test]
    fn assignability_delegates_to_the_central_type_relation() {
        let arena = TypeArena::new();
        let queries = SemanticQueries::new(&arena);

        assert!(queries.is_assignable(arena.string(), arena.unknown()));
        assert!(!queries.is_assignable(arena.number(), arena.string()));
    }
}
