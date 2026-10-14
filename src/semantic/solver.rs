//! Stable entry points for semantic type-system operations used by the AST checker.
//!
//! The checker should depend on these operations rather than on the layout of the
//! current implementation in `generics.rs`. This first layer is deliberately a
//! behavior-preserving adapter: it keeps the existing implementation and caches,
//! while giving the upcoming solver migration one narrow seam to replace.

use crate::arena::{TypeArena, TypeId};
use crate::fxhash::FxHashMap;
use crate::semantic::queries::RelationCache;
use crate::types::{Param, TypeParameterId};

/// Relative strength of a candidate source. Explicit type arguments are not candidates
/// at all: their bindings are locked before inference starts. Contextual candidates are
/// higher priority than ordinary argument candidates; low-priority candidates are kept
/// as fallback evidence if no stronger source exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub(crate) enum InferencePriority {
    Low,
    Normal,
    Contextual,
}

#[derive(Clone, Copy, Debug)]
struct InferenceCandidate {
    id: TypeParameterId,
    ty: TypeId,
    priority: InferencePriority,
    preserves_literals: bool,
}

/// Mutable state for inferring one generic call or constructor application.
///
/// Structural matching only collects candidates. `finish` resolves candidates after all
/// arguments have been visited, giving us a stable point to apply priorities, defaults and
/// constraints without letting an early argument irrevocably decide the answer.
pub(crate) struct GenericInferenceSession {
    bindings: Vec<(TypeParameterId, TypeId)>,
    locked: Vec<TypeParameterId>,
    candidates: Vec<InferenceCandidate>,
}

impl GenericInferenceSession {
    pub(crate) fn new(
        bindings: Vec<(TypeParameterId, TypeId)>,
        locked: Vec<TypeParameterId>,
    ) -> Self {
        Self {
            bindings,
            locked,
            candidates: Vec::new(),
        }
    }

    pub(crate) fn infer_argument(
        &mut self,
        arena: &mut TypeArena,
        parameter_type: TypeId,
        argument_type: TypeId,
        cache: &mut RelationCache,
    ) {
        self.infer_argument_with_priority(
            arena,
            parameter_type,
            argument_type,
            InferencePriority::Normal,
            cache,
        );
    }

    pub(crate) fn infer_argument_with_priority(
        &mut self,
        arena: &mut TypeArena,
        parameter_type: TypeId,
        argument_type: TypeId,
        priority: InferencePriority,
        cache: &mut RelationCache,
    ) {
        let mut candidates = Vec::new();
        GenericSolver::infer_type_param_bindings(
            arena,
            parameter_type,
            argument_type,
            &mut candidates,
            &self.locked,
            cache,
        );
        let mut constraints = Vec::new();
        GenericSolver::collect_generic_param_constraints(arena, parameter_type, &mut constraints);
        for (id, ty) in candidates {
            if self.locked.contains(&id) {
                continue;
            }
            let preserves_literals = constraints
                .iter()
                .find(|(constraint_id, _, _)| *constraint_id == id)
                .is_some_and(|(_, _, constraint)| {
                    GenericSolver::preserves_literal_inference(arena, *constraint)
                });
            self.candidates.push(InferenceCandidate {
                id,
                ty,
                priority,
                preserves_literals,
            });
        }
    }

    /// Resolve the candidates accumulated for this call. Only candidates at the highest
    /// priority for a parameter participate; same-priority candidates are reconciled by
    /// subtype coverage and the existing literal-union rule. Incomparable candidates do
    /// not automatically form a union, so the later assignability pass still diagnoses
    /// conflicting arguments such as `pair(1, "x")`.
    pub(crate) fn finish(
        mut self,
        arena: &mut TypeArena,
        cache: &mut RelationCache,
    ) -> Vec<(TypeParameterId, TypeId)> {
        let mut highest_priority = FxHashMap::default();
        for candidate in &self.candidates {
            highest_priority
                .entry(candidate.id)
                .and_modify(|priority: &mut InferencePriority| {
                    *priority = (*priority).max(candidate.priority);
                })
                .or_insert(candidate.priority);
        }

        for candidate in self.candidates {
            if self.locked.contains(&candidate.id)
                || highest_priority.get(&candidate.id) != Some(&candidate.priority)
            {
                continue;
            }
            GenericSolver::merge_inference_candidate(
                arena,
                candidate.id,
                candidate.ty,
                candidate.preserves_literals,
                &mut self.bindings,
                cache,
            );
        }
        self.bindings
    }
}

/// Entry point for type-system operations used while checking generic signatures.
///
/// The facade does not own an arena or cache globally. Mutable inference state belongs
/// to one call in `GenericInferenceSession`; relation caching remains owned by the checker.
pub(crate) struct GenericSolver;

impl GenericSolver {
    /// Collect generic type-parameter bindings from one parameter/argument pair.
    pub(crate) fn infer_type_param_bindings(
        arena: &mut TypeArena,
        param_type: TypeId,
        arg_type: TypeId,
        bindings: &mut Vec<(TypeParameterId, TypeId)>,
        locked: &[TypeParameterId],
        cache: &mut RelationCache,
    ) {
        super::generics::infer_type_param_bindings(
            arena, param_type, arg_type, bindings, locked, cache,
        );
    }

    pub(crate) fn merge_inference_candidate(
        arena: &mut TypeArena,
        id: TypeParameterId,
        candidate: TypeId,
        preserves_literals: bool,
        bindings: &mut Vec<(TypeParameterId, TypeId)>,
        cache: &mut RelationCache,
    ) {
        super::generics::merge_inference_candidate(
            arena,
            id,
            candidate,
            preserves_literals,
            bindings,
            cache,
        );
    }

    pub(crate) fn preserves_literal_inference(arena: &TypeArena, constraint: TypeId) -> bool {
        super::generics::preserves_literal_inference(arena, constraint)
    }

    /// Find type parameters in declaration order, used for positional explicit args.
    pub(crate) fn ordered_generic_param_ids(
        arena: &TypeArena,
        type_id: TypeId,
        out: &mut Vec<TypeParameterId>,
    ) {
        super::generics::ordered_generic_param_ids(arena, type_id, out);
    }

    /// Collect generic constraints reachable from a signature type.
    pub(crate) fn collect_generic_param_constraints(
        arena: &TypeArena,
        type_id: TypeId,
        out: &mut Vec<(TypeParameterId, String, TypeId)>,
    ) {
        super::generics::collect_generic_param_constraints(arena, type_id, out);
    }

    /// Resolve the expected element type for an argument, including rest parameters.
    pub(crate) fn expected_param_type(
        arena: &TypeArena,
        params: &[Param],
        index: usize,
    ) -> Option<TypeId> {
        super::generics::expected_param_type(arena, params, index)
    }

    /// Whether a type still mentions an uninstantiated type parameter.
    pub(crate) fn contains_type_param(arena: &TypeArena, type_id: TypeId) -> bool {
        super::generics::contains_type_param(arena, type_id)
    }

    /// Substitute bound parameters, preserving any parameters without a binding.
    pub(crate) fn substitute_bound_type_params(
        arena: &mut TypeArena,
        type_id: TypeId,
        bindings: &[(TypeParameterId, TypeId)],
    ) -> TypeId {
        super::generics::substitute_bound_type_params(arena, type_id, bindings)
    }

    /// Substitute parameters, resolving any unbound parameter to `unknown`.
    pub(crate) fn substitute_type_params(
        arena: &mut TypeArena,
        type_id: TypeId,
        bindings: &[(TypeParameterId, TypeId)],
    ) -> TypeId {
        super::generics::substitute_type_params(arena, type_id, bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypeParameterId;

    #[test]
    fn inference_session_does_not_override_explicit_type_arguments() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let parameter = arena.alloc(crate::types::Type::GenericParameter(
            id,
            "T".to_string(),
            None,
        ));
        let explicit = arena.string();
        let argument = arena.number();
        let mut session = GenericInferenceSession::new(vec![(id, explicit)], vec![id]);
        let mut cache = RelationCache::default();

        session.infer_argument(&mut arena, parameter, argument, &mut cache);

        assert_eq!(session.finish(&mut arena, &mut cache), vec![(id, explicit)]);
    }

    #[test]
    fn higher_priority_candidate_wins_over_lower_priority_candidate() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(10, 0);
        let parameter = arena.alloc(crate::types::Type::GenericParameter(
            id,
            "T".to_string(),
            None,
        ));
        let number = arena.number();
        let string = arena.string();
        let mut session = GenericInferenceSession::new(Vec::new(), Vec::new());
        let mut cache = RelationCache::default();
        session.infer_argument_with_priority(
            &mut arena,
            parameter,
            string,
            InferencePriority::Low,
            &mut cache,
        );
        session.infer_argument_with_priority(
            &mut arena,
            parameter,
            number,
            InferencePriority::Contextual,
            &mut cache,
        );
        assert_eq!(session.finish(&mut arena, &mut cache), vec![(id, number)]);
    }

    #[test]
    fn same_priority_literal_candidates_use_the_existing_union_policy() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(11, 0);
        let number = arena.number();
        let parameter = arena.alloc(crate::types::Type::GenericParameter(
            id,
            "T".to_string(),
            Some(number),
        ));
        let one = arena.alloc(crate::types::Type::NumberLiteral(1.0));
        let two = arena.alloc(crate::types::Type::NumberLiteral(2.0));
        let mut session = GenericInferenceSession::new(Vec::new(), Vec::new());
        let mut cache = RelationCache::default();
        session.infer_argument(&mut arena, parameter, one, &mut cache);
        session.infer_argument(&mut arena, parameter, two, &mut cache);

        let bindings = session.finish(&mut arena, &mut cache);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, id);
        let crate::types::Type::Union(members) = arena.get(bindings[0].1) else {
            panic!("expected the two literal candidates to be combined");
        };
        assert_eq!(members.len(), 2);
    }
}
