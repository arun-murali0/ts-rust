use crate::arena::{TypeArena, TypeId};
use crate::fxhash::FxHashMap;
use crate::subtyping;

/// Semantic operations exposed to AST-facing checker code.
///
/// The query layer is intentionally small. It gives bridge features a stable
/// place to ask semantic questions without making the bridge depend on the
/// implementation details of individual type-system algorithms.
///
/// Despite taking `&mut` (for the cache below), this still asks nothing of
/// the type system that a read-only view couldn't answer -- it does not
/// allocate types or otherwise mutate the arena. The `&mut` is purely to let
/// repeated identical questions be answered from a cache instead of
/// re-walking the type structure.
pub struct SemanticQueries<'a> {
    arena: &'a TypeArena,

    // Memoizes is_subtype by the exact (source, target) TypeId pair. See the
    // doc comment on CheckContext::subtype_cache, which owns this map for the
    // lifetime of one file's check, for what this does and does not cover.
    cache: &'a mut FxHashMap<(TypeId, TypeId), bool>,
}

impl<'a> SemanticQueries<'a> {
    pub(crate) fn new(
        arena: &'a TypeArena,
        cache: &'a mut FxHashMap<(TypeId, TypeId), bool>,
    ) -> Self {
        Self { arena, cache }
    }

    /// Whether a value of type `source` can be used where `target` is
    /// expected. Currently identical to `is_subtype`: TypeScript's real
    /// assignability rules (excess property checks, `const` assertions,
    /// etc.) diverge from plain subtyping in cases ts-rust doesn't
    /// implement yet. `is_assignable` is kept as its own method so bridge
    /// code calls the concept it actually means, and so that divergence
    /// can be added here later without touching any call site.
    pub fn is_assignable(&mut self, source: TypeId, target: TypeId) -> bool {
        self.is_subtype(source, target)
    }

    // Every result gets cached unconditionally, including a single point lookup
    // that would never be asked again -- checking for reuse potential first
    // would cost more than the memoization ever saves. This is a leaf method:
    // it is the only thing in the checker allowed to call subtyping::is_subtype
    // directly, precisely so "did this go through the cache" has one place to
    // check rather than needing an audit of every call site. (One exception
    // exists today, generics::infer_type_param_bindings, which predates this
    // cache and works on a bare &mut TypeArena with no CheckContext to draw the
    // cache from -- see the note on CheckContext::subtype_cache.)
    pub fn is_subtype(&mut self, source: TypeId, target: TypeId) -> bool {
        let key = (source, target);
        if let Some(&cached) = self.cache.get(&key) {
            return cached;
        }

        let result = subtyping::is_subtype(self.arena, source, target);
        self.cache.insert(key, result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticQueries;
    use crate::arena::TypeArena;
    use crate::fxhash::FxHashMap;
    use crate::subtyping;

    #[test]
    fn assignability_delegates_to_the_central_type_relation() {
        let arena = TypeArena::new();
        let mut cache = FxHashMap::default();
        let mut queries = SemanticQueries::new(&arena, &mut cache);

        assert!(queries.is_assignable(arena.string(), arena.unknown()));
        assert!(!queries.is_assignable(arena.number(), arena.string()));
    }

    #[test]
    fn cached_result_matches_the_uncached_answer() {
        let arena = TypeArena::new();
        let mut cache = FxHashMap::default();
        let mut queries = SemanticQueries::new(&arena, &mut cache);

        // First call computes and caches; second call must hit the cache and
        // still return the same answer, not a stale or default value.
        assert!(queries.is_subtype(arena.number(), arena.unknown()));
        assert!(queries.is_subtype(arena.number(), arena.unknown()));

        assert!(!queries.is_subtype(arena.boolean(), arena.string()));
        assert!(!queries.is_subtype(arena.boolean(), arena.string()));
    }

    #[test]
    fn cache_does_not_populate_the_reverse_pair() {
        // Subtyping is not symmetric (see subtyping.rs: literal -> primitive
        // widening only goes one way). A cache keyed without direction would
        // silently make is_subtype(b, a) return whatever is_subtype(a, b)
        // returned. This must not happen.
        let mut arena = TypeArena::new();
        let mut cache = FxHashMap::default();

        let five = arena.alloc(crate::types::Type::NumberLiteral(5.0));
        let number = arena.number();

        let mut queries = SemanticQueries::new(&arena, &mut cache);

        assert!(queries.is_subtype(five, number), "5 is a subtype of number");
        assert!(
            !queries.is_subtype(number, five),
            "number is not a subtype of the literal 5, even after caching the reverse pair"
        );
    }

    #[test]
    fn repeated_calls_across_many_pairs_stay_correct() {
        // Stress the cache with many distinct pairs, interleaved with repeats
        // of earlier pairs, checking every answer against a direct
        // (uncached) call every time. This is the property a cache must
        // never violate: memoizing must never change an answer, no matter
        // how many other entries have been inserted since, or in what order
        // pairs are revisited.
        let mut arena = TypeArena::new();
        let mut cache = FxHashMap::default();

        let mut ids = vec![
            arena.number(),
            arena.string(),
            arena.boolean(),
            arena.unknown(),
            arena.any(),
            arena.never(),
        ];
        for i in 0..20 {
            ids.push(arena.alloc(crate::types::Type::NumberLiteral(i as f64)));
        }
        for i in 0..20 {
            ids.push(arena.alloc(crate::types::Type::StringLiteral(format!("s{i}"))));
        }

        let mut queries = SemanticQueries::new(&arena, &mut cache);

        // Round 1: every ordered pair, populating the cache.
        for &a in &ids {
            for &b in &ids {
                let cached = queries.is_subtype(a, b);
                let direct = subtyping::is_subtype(&arena, a, b);
                assert_eq!(cached, direct, "mismatch on first visit to ({a:?}, {b:?})");
            }
        }

        // Round 2: same pairs again, now every one is a cache hit.
        for &a in &ids {
            for &b in &ids {
                let cached = queries.is_subtype(a, b);
                let direct = subtyping::is_subtype(&arena, a, b);
                assert_eq!(
                    cached, direct,
                    "mismatch on cached revisit to ({a:?}, {b:?})"
                );
            }
        }
    }
}
