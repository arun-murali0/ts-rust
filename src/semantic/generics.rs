use crate::arena::{TypeArena, TypeId};
use crate::fxhash::FxHashMap;
use crate::semantic::queries::{SemanticQueries, SubtypeCache};
use crate::types::{FunctionType, ObjectType, PropertyEntry, Type};

// Structural inference: walks a generic function's declared parameter type
// alongside a call's actual argument type in lockstep, recording a binding the
// first time it reaches a GenericParameter leaf. This is only the argument-driven
// part: an explicit call-site list like identity<string>(x) is applied by the
// caller (calls.rs), which pre-binds those parameters and passes them in as
// `locked` so nothing here can override them.
//
// A second argument resolving to an already-bound parameter does not start a
// union (real TypeScript does not do this either: pair(1, "x") for
// pair<T>(a: T, b: T) is a genuine type error in TypeScript, not T = number |
// string). Instead the two candidates are resolved through ordinary subtyping:
// if one is already a supertype of the other, the binding widens to cover both,
// matching pick(new Dog(), new Animal()) inferring T = Animal. Two candidates
// with no subtype relationship in either direction are left as the first
// binding; the real mismatch is still caught afterward by the normal
// per-argument assignability check in calls.rs, the same way it always was.
pub(crate) fn infer_type_param_bindings(
    arena: &mut TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
    locked: &[crate::types::TypeParameterId],
    cache: &mut SubtypeCache,
) {
    infer_type_param_bindings_inner(
        arena,
        param_type,
        arg_type,
        bindings,
        locked,
        cache,
        &mut Vec::new(),
    )
}

// Inference mutates the arena (widening and union building allocate), so it cannot
// keep one SemanticQueries alive across its steps: that holds a shared borrow of the
// arena for its whole life. Building one per question keeps the borrow to the call.
// The reason to go through it at all is that candidates are compared against the same
// existing binding over and over (`allSame(1, 2, ..., 8)`), and those repeats were the
// one place the cache never saw.
fn is_subtype_cached(
    arena: &TypeArena,
    cache: &mut SubtypeCache,
    sub: TypeId,
    sup: TypeId,
) -> bool {
    SemanticQueries::new(arena, cache).is_subtype(sub, sup)
}

// A recursive param_type (e.g. `Box<T>`'s own self-referential `next: Box<T>`,
// still holding the bare GenericParameter shape while it's being matched
// against a real argument) can lead back to matching the same (param_type,
// arg_type) pair against itself before the first match finishes. `seen` tracks
// pairs currently being walked; re-entering one contributes nothing further
// worth inferring, so it's simply skipped rather than walked again.
fn infer_type_param_bindings_inner(
    arena: &mut TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
    locked: &[crate::types::TypeParameterId],
    cache: &mut SubtypeCache,
    seen: &mut Vec<(TypeId, TypeId)>,
) {
    let pair = (param_type, arg_type);
    if seen.contains(&pair) {
        return;
    }
    seen.push(pair);
    infer_type_param_bindings_uncached(arena, param_type, arg_type, bindings, locked, cache, seen);
    seen.pop();
}

fn infer_type_param_bindings_uncached(
    arena: &mut TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
    locked: &[crate::types::TypeParameterId],
    cache: &mut SubtypeCache,
    seen: &mut Vec<(TypeId, TypeId)>,
) {
    match arena.get(param_type).clone() {
        Type::GenericParameter(id, _, _) => {
            // A parameter bound by an explicit call-site type argument
            // (identity<string>(x)) is not up for renegotiation by whatever
            // argument happens to line up with it structurally -- explicit
            // wins outright, the same way TypeScript itself treats an
            // explicit type argument as authoritative rather than a hint.
            if locked.contains(&id) {
                return;
            }

            // Widened so identity(5) infers number, matching what a
            // TypeScript author expects from a bare generic call, rather than
            // the checker binding T to the narrower literal type 5.
            let candidate = crate::types::widen(arena, arg_type);

            match bindings.iter_mut().find(|(bound, _)| *bound == id) {
                None => bindings.push((id, candidate)),
                Some((_, existing)) => {
                    if is_subtype_cached(arena, cache, candidate, *existing) {
                        // The existing binding already covers this candidate.
                    } else if is_subtype_cached(arena, cache, *existing, candidate) {
                        *existing = candidate;
                    }
                }
            }
        }
        Type::Array(param_element) => {
            let arg_element = match arena.get(arg_type) {
                Type::Array(element) => *element,
                _ => return,
            };
            infer_type_param_bindings_inner(
                arena,
                param_element,
                arg_element,
                bindings,
                locked,
                cache,
                seen,
            );
        }
        Type::Object(param_object) => {
            let Type::Object(arg_object) = arena.get(arg_type).clone() else {
                return;
            };
            for param_prop in param_object.properties.iter() {
                if let Some(arg_prop) = arg_object
                    .properties
                    .iter()
                    .find(|p| p.name == param_prop.name)
                {
                    infer_type_param_bindings_inner(
                        arena,
                        param_prop.type_id,
                        arg_prop.type_id,
                        bindings,
                        locked,
                        cache,
                        seen,
                    );
                }
            }
        }
        Type::Function(param_fn) => {
            let Type::Function(arg_fn) = arena.get(arg_type).clone() else {
                return;
            };
            for (p, a) in param_fn.params.iter().zip(&arg_fn.params) {
                infer_type_param_bindings_inner(
                    arena, p.type_id, a.type_id, bindings, locked, cache, seen,
                );
            }
            infer_type_param_bindings_inner(
                arena,
                param_fn.return_type,
                arg_fn.return_type,
                bindings,
                locked,
                cache,
                seen,
            );
        }
        // A parameter typed `T | undefined`, `T | null` or the like. TypeScript
        // first sets aside whatever part of the argument a concrete member of the
        // union already accounts for (an `undefined` argument against `T |
        // undefined`), then infers the type parameter from what is left. The
        // leftover members are combined into one candidate, so passing a
        // `number | string` binds T to `number | string` in one step rather than
        // to `number` and then failing on `string`. An `any` or error argument is
        // never set aside, since it is compatible with every concrete member and
        // would otherwise leave T with nothing to infer from.
        Type::Union(param_members) => {
            let arg_members = match arena.get(arg_type) {
                Type::Union(members) => members.clone(),
                _ => vec![arg_type],
            };
            let remaining: Vec<TypeId> = arg_members
                .into_iter()
                .filter(|&member| {
                    if matches!(arena.get(member), Type::Any | Type::Error) {
                        return true;
                    }
                    !param_members.iter().any(|&concrete| {
                        !contains_type_param(arena, concrete)
                            && is_subtype_cached(arena, cache, member, concrete)
                    })
                })
                .collect();
            if remaining.is_empty() {
                return;
            }
            let remainder = if remaining.len() == 1 {
                remaining[0]
            } else {
                arena.alloc_union(remaining)
            };
            for &member in &param_members {
                if contains_type_param(arena, member) {
                    infer_type_param_bindings_inner(
                        arena, member, remainder, bindings, locked, cache, seen,
                    );
                }
            }
        }
        _ => {}
    }
}

// Rebuilds type_id, replacing every GenericParameter occurrence with its bound
// type, or unknown for a parameter nothing ever informed. Bails out immediately
// when type_id contains no type parameters at all, which is every ordinary,
// non-generic type, so this costs nothing beyond one quick tree walk for the
// common case. The function's own stored signature is never mutated; every call
// with different bindings produces a fresh, independent result.
pub(crate) fn substitute_type_params(
    arena: &mut TypeArena,
    type_id: TypeId,
    bindings: &[(crate::types::TypeParameterId, TypeId)],
) -> TypeId {
    substitute_impl(arena, type_id, bindings, false)
}

// Like substitute_type_params, but a parameter with no binding is left as it is
// instead of becoming unknown. A type reference such as `Box<number>` binds only
// the declaration's own parameters, so a method with a parameter of its own
// (`map<U>(f: (x: T) => U): U`) must keep its U to be inferred at the call site.
pub(crate) fn substitute_bound_type_params(
    arena: &mut TypeArena,
    type_id: TypeId,
    bindings: &[(crate::types::TypeParameterId, TypeId)],
) -> TypeId {
    substitute_impl(arena, type_id, bindings, true)
}

fn substitute_impl(
    arena: &mut TypeArena,
    type_id: TypeId,
    bindings: &[(crate::types::TypeParameterId, TypeId)],
    keep_unbound: bool,
) -> TypeId {
    if bindings.is_empty() {
        return type_id;
    }
    let mut outermost_cut = usize::MAX;
    Substitution {
        bindings,
        keep_unbound,
        open: Vec::new(),
        rewritten: FxHashMap::default(),
        cyclic: Vec::new(),
    }
    .rewrite(arena, type_id, &mut outermost_cut)
}

// Scratch state for one substitute_impl call, so that everything learned while
// rewriting one part of a type is reused by the rest of it. The graph is a DAG
// after interning: a subtype reachable along several paths used to be rewritten
// once per path, and each level re-asked "does this contain a parameter" from
// scratch. Now each node is rewritten once (`rewritten`), and the parameter
// question is answered from the arena's own cache (see contains_type_param).
//
// `rewritten` does not outlive the call. A placeholder can be completed by
// TypeArena::set between two calls, which would make a longer-lived rewrite stale.
struct Substitution<'b> {
    bindings: &'b [(crate::types::TypeParameterId, TypeId)],
    keep_unbound: bool,

    // Ids currently being rewritten, outermost first.
    open: Vec<TypeId>,

    rewritten: FxHashMap<TypeId, TypeId>,

    // Ids the walk has found to reach themselves (see rewrite).
    cyclic: Vec<TypeId>,
}

impl Substitution<'_> {
    // A recursive type (Node { next: Node }, or a recursive generic like
    // `Box<T> { value: T, next: Box<T> | null }`) means substituting inside
    // type_id's properties can lead back to substituting type_id itself before the
    // first call returns. Re-entering an id already on the current path is exactly
    // that self-referential edge, and it is what stops the walk from looping.
    //
    // Leaving the edge pointing at the original id is right for a type that has
    // nothing to substitute, and wrong for one that does: `Box<number>.next` would
    // still be the generic `Box<T> | null`, and `box.next.value` would read as T.
    // So the cut only *finds* the cycle (`cyclic`); once the first pass over an
    // object has finished and it turns out to reach itself, rewrite_recursive
    // builds it again with a placeholder of its own, so that the edge closes on the
    // new object instead. Only an Object can close a cycle (see TypeArena), which
    // is why nothing else gets a second pass.
    //
    // `lowest_cut` is the index in `open` of the outermost id any cut below this
    // call pointed at (usize::MAX when none did). A result is memoised only when
    // no cut reached outside the node: a rewrite that left an outer id as-is
    // reflects where the walk entered, and reusing it from another entry point
    // could differ from what a fresh walk would build there.
    fn rewrite(
        &mut self,
        arena: &mut TypeArena,
        type_id: TypeId,
        lowest_cut: &mut usize,
    ) -> TypeId {
        if let Some(&done) = self.rewritten.get(&type_id) {
            return done;
        }
        if !contains_type_param(arena, type_id) {
            return type_id;
        }
        if let Some(position) = self.open.iter().position(|&id| id == type_id) {
            *lowest_cut = (*lowest_cut).min(position);
            if !self.cyclic.contains(&type_id) {
                self.cyclic.push(type_id);
            }
            return type_id;
        }

        let depth = self.open.len();
        self.open.push(type_id);
        let mut cut_below = usize::MAX;
        let mut result = self.rewrite_uncached(arena, type_id, &mut cut_below);
        self.open.pop();

        if self.cyclic.contains(&type_id) && matches!(arena.get(type_id), Type::Object(_)) {
            // The first pass left the self-edge on the original id, so its result
            // is discarded. Cuts to outer nodes are met again on this pass, so the
            // count restarts rather than carrying the first pass's.
            cut_below = usize::MAX;
            result = self.rewrite_recursive(arena, type_id, &mut cut_below);
        }

        if cut_below >= depth {
            self.rewritten.insert(type_id, result);
        } else {
            // rewrite_recursive registers its placeholder before descending; when
            // the result turns out not to be reusable, that entry must go too.
            self.rewritten.remove(&type_id);
        }
        *lowest_cut = (*lowest_cut).min(cut_below);
        result
    }

    // Rebuilds a self-referential object around a placeholder of its own. The
    // placeholder is registered as type_id's rewrite *before* the properties are
    // walked, so the walk finds it where it used to find the cut, and the edge
    // that closed on type_id now closes on the copy. The placeholder is not
    // interned (see TypeArena::alloc_object_placeholder), which is what a
    // recursive shape needs anyway: other types hold its raw id, so it could not
    // be merged with an identical shape afterwards.
    fn rewrite_recursive(
        &mut self,
        arena: &mut TypeArena,
        type_id: TypeId,
        cut: &mut usize,
    ) -> TypeId {
        let Type::Object(object) = arena.get(type_id).clone() else {
            return type_id;
        };
        let placeholder = arena.alloc_object_placeholder();
        self.rewritten.insert(type_id, placeholder);

        let mut properties = Vec::with_capacity(object.properties.len());
        for p in object.properties.iter() {
            properties.push(PropertyEntry {
                name: p.name.clone(),
                type_id: self.rewrite(arena, p.type_id, cut),
                optional: p.optional,
                is_method: p.is_method,
            });
        }
        arena.set(placeholder, Type::Object(ObjectType::new(properties)));
        placeholder
    }

    fn rewrite_uncached(
        &mut self,
        arena: &mut TypeArena,
        type_id: TypeId,
        cut: &mut usize,
    ) -> TypeId {
        match arena.get(type_id).clone() {
            Type::GenericParameter(id, _, _) => {
                let bound = self
                    .bindings
                    .iter()
                    .find(|(bound, _)| *bound == id)
                    .map(|(_, resolved)| *resolved);
                match bound {
                    Some(resolved) => resolved,
                    None if self.keep_unbound => type_id,
                    None => arena.unknown(),
                }
            }
            Type::Array(element) => {
                let substituted = self.rewrite(arena, element, cut);
                arena.alloc(Type::Array(substituted))
            }
            Type::Function(function) => {
                let mut params = Vec::with_capacity(function.params.len());
                for p in &function.params {
                    params.push(crate::types::Param {
                        type_id: self.rewrite(arena, p.type_id, cut),
                        optional: p.optional,
                        rest: p.rest,
                        name: p.name.clone(),
                    });
                }
                let return_type = self.rewrite(arena, function.return_type, cut);
                arena.alloc(Type::Function(FunctionType {
                    params,
                    return_type,
                    is_untyped: function.is_untyped,
                }))
            }
            Type::Object(object) => {
                let mut properties = Vec::with_capacity(object.properties.len());
                for p in object.properties.iter() {
                    properties.push(PropertyEntry {
                        name: p.name.clone(),
                        type_id: self.rewrite(arena, p.type_id, cut),
                        optional: p.optional,
                        is_method: p.is_method,
                    });
                }
                arena.alloc(Type::Object(ObjectType::new(properties)))
            }
            Type::Union(members) => {
                let mut substituted = Vec::with_capacity(members.len());
                for &m in &members {
                    substituted.push(self.rewrite(arena, m, cut));
                }
                arena.alloc_union(substituted)
            }
            _ => type_id,
        }
    }
}

// Collects every distinct type parameter appearing in type_id, then hands them
// back in declaration order rather than tree-walk-encounter order. An explicit
// call-site type argument list (identity<string, number>(x, y)) has no
// declaration span of its own to key a TypeParameterId lookup off of -- it is
// just a positional list -- so the only way to zip it correctly against a
// function's own T, U, ... is to recover the order they were declared in. That
// order survives structurally: push_type_params numbers each parameter by its
// position in the source `<...>` list (see TypeParameterId::parameter_index),
// so sorting by that index reconstructs it even though nothing about a
// FunctionType itself remembers "T came before U".
pub(crate) fn ordered_generic_param_ids(
    arena: &TypeArena,
    type_id: TypeId,
    out: &mut Vec<crate::types::TypeParameterId>,
) {
    // Most callees are not generic. The arena remembers that per type, so the
    // common case costs one lookup instead of a walk (this runs for every call
    // expression, on each parameter and the return type).
    if !contains_type_param(arena, type_id) {
        return;
    }
    ordered_generic_param_ids_inner(arena, type_id, out, &mut Vec::new());
    out.sort_by_key(|id| id.parameter_index());
}

// The walk only accumulates a set of parameters, so a node reached a second time
// (through a self-referential property, or just because interning shares it
// between parents) can never add anything the first visit did not. That is why
// `visited` is never unwound, unlike the path stacks the walks that build a new
// type need: a shared subtype is walked once per call instead of once per
// parent. First-seen order is unchanged, and the sort by parameter_index above
// is what fixes the final order anyway. The `out.contains` check below is a
// separate thing (it dedupes which *parameters* get collected) and does not
// bound the walk.
//
// `visited` is a plain Vec with a linear lookup, not a hash set. Only types that
// really mention a parameter get this far (the check above), and those are
// function signatures and small generic objects, where a handful of comparisons
// is cheaper than hashing and allocating a table on every call.
fn ordered_generic_param_ids_inner(
    arena: &TypeArena,
    type_id: TypeId,
    out: &mut Vec<crate::types::TypeParameterId>,
    visited: &mut Vec<TypeId>,
) {
    if visited.contains(&type_id) {
        return;
    }
    visited.push(type_id);
    match arena.get(type_id) {
        Type::GenericParameter(id, _, _) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Type::Array(element) => ordered_generic_param_ids_inner(arena, *element, out, visited),
        Type::Function(f) => {
            for param in &f.params {
                ordered_generic_param_ids_inner(arena, param.type_id, out, visited);
            }
            ordered_generic_param_ids_inner(arena, f.return_type, out, visited);
        }
        Type::Object(o) => {
            for property in o.properties.iter() {
                ordered_generic_param_ids_inner(arena, property.type_id, out, visited);
            }
        }
        Type::Union(members) => {
            for &member in members {
                ordered_generic_param_ids_inner(arena, member, out, visited);
            }
        }
        _ => {}
    }
}

pub(crate) fn contains_type_param(arena: &TypeArena, type_id: TypeId) -> bool {
    let mut outermost_cut = usize::MAX;
    scan_for_type_param(arena, type_id, &mut Vec::new(), &mut outermost_cut)
}

// The "does this type mention a type parameter" question. Answers are kept in
// the arena (TypeArena::cached_mentions_type_param), not per call: the checker
// asks it for every call expression, mostly about types that never change, and
// interning has turned the type graph into a DAG, so the same `number[]` or
// `{ id: number }` node hangs off many parents. The old version kept only the
// ids on the current path, so a shared node was walked again for every parent.
//
// Caching needs care because of recursive types. A type reachable from itself
// (Node { next: Node }, via namespace::resolve's placeholder backpatch) is cut
// where it re-enters an id that is still open, and that cut answers "false" only
// because the answer is being computed. A node whose walk was cut at an *outer*
// open id has not really been decided: that outer id may reach a parameter
// through some other branch. So `true` is always kept (one parameter anywhere is
// enough), while `false` is kept only when every cut in the walk pointed at the
// node itself or below it, meaning its whole subtree was actually explored.
//
// `open` is the current path, outermost first. `lowest_cut` collects the
// smallest index in `open` that any cut below this call pointed at.
fn scan_for_type_param(
    arena: &TypeArena,
    type_id: TypeId,
    open: &mut Vec<TypeId>,
    lowest_cut: &mut usize,
) -> bool {
    if let Some(known) = arena.cached_mentions_type_param(type_id) {
        return known;
    }
    if let Some(position) = open.iter().position(|&id| id == type_id) {
        *lowest_cut = (*lowest_cut).min(position);
        return false;
    }

    let depth = open.len();
    open.push(type_id);
    let mut cut_below = usize::MAX;
    let found = match arena.get(type_id) {
        Type::GenericParameter(_, _, _) => true,
        Type::Array(element) => scan_for_type_param(arena, *element, open, &mut cut_below),
        Type::Function(f) => {
            f.params
                .iter()
                .any(|p| scan_for_type_param(arena, p.type_id, open, &mut cut_below))
                || scan_for_type_param(arena, f.return_type, open, &mut cut_below)
        }
        Type::Object(o) => o
            .properties
            .iter()
            .any(|p| scan_for_type_param(arena, p.type_id, open, &mut cut_below)),
        Type::Union(members) => members
            .iter()
            .any(|&m| scan_for_type_param(arena, m, open, &mut cut_below)),
        _ => false,
    };
    open.pop();

    if found || cut_below >= depth {
        arena.remember_mentions_type_param(type_id, found);
    }
    *lowest_cut = (*lowest_cut).min(cut_below);
    found
}

// The type an argument at index is checked against. A rest parameter absorbs
// every position past the end of the declared list, and its own declared type is
// the array type, not the per-argument type, so it is unwrapped to the element
// type here.
pub(crate) fn expected_param_type(
    arena: &crate::arena::TypeArena,
    params: &[crate::types::Param],
    index: usize,
) -> Option<TypeId> {
    let param = match params.get(index) {
        Some(param) => param,
        None => params.last().filter(|p| p.rest)?,
    };
    Some(if param.rest {
        match arena.get(param.type_id) {
            Type::Array(element) => *element,
            _ => param.type_id,
        }
    } else {
        param.type_id
    })
}

// Walks type_id the same way contains_type_param does, but collects every
// constrained GenericParameter it finds instead of just checking for their
// presence. Used once per call, after inference finishes, to find which of a
// generic function's own type parameters actually have an `extends` bound to
// check the inferred argument against. A parameter found more than once (T
// appearing in two parameter positions, for instance) is only recorded once,
// since its constraint is the same wherever it appears. The name travels
// alongside the constraint purely so the diagnostic that checks these can name
// the offending type parameter; it plays no role in matching.
pub(crate) fn collect_generic_param_constraints(
    arena: &TypeArena,
    type_id: TypeId,
    constraints: &mut Vec<(crate::types::TypeParameterId, String, TypeId)>,
) {
    // Same early exit as ordered_generic_param_ids: no parameter, no constraints.
    if !contains_type_param(arena, type_id) {
        return;
    }
    collect_generic_param_constraints_inner(arena, type_id, constraints, &mut Vec::new());
}

// Same walk as ordered_generic_param_ids_inner, and the same choice of guard: a
// visited set that is never unwound. The walk only accumulates constraints, and
// a node reached a second time (by a cycle or by plain sharing) would add
// nothing the first visit did not. The constraint-level dedup below is a separate
// concern (which *parameters* get recorded) and does not bound the walk.
fn collect_generic_param_constraints_inner(
    arena: &TypeArena,
    type_id: TypeId,
    constraints: &mut Vec<(crate::types::TypeParameterId, String, TypeId)>,
    visited: &mut Vec<TypeId>,
) {
    if visited.contains(&type_id) {
        return;
    }
    visited.push(type_id);
    match arena.get(type_id) {
        Type::GenericParameter(id, name, Some(constraint)) => {
            if !constraints.iter().any(|(existing, _, _)| existing == id) {
                constraints.push((*id, name.clone(), *constraint));
            }
        }
        Type::GenericParameter(_, _, None) => {}
        Type::Array(element) => {
            collect_generic_param_constraints_inner(arena, *element, constraints, visited)
        }
        Type::Function(f) => {
            for param in &f.params {
                collect_generic_param_constraints_inner(arena, param.type_id, constraints, visited);
            }
            collect_generic_param_constraints_inner(arena, f.return_type, constraints, visited);
        }
        Type::Object(o) => {
            for property in o.properties.iter() {
                collect_generic_param_constraints_inner(
                    arena,
                    property.type_id,
                    constraints,
                    visited,
                );
            }
        }
        Type::Union(members) => {
            for &member in members {
                collect_generic_param_constraints_inner(arena, member, constraints, visited);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypeParameterId;

    // Pins the reason this path was rerouted: a second candidate for an already
    // bound parameter is compared against the existing binding, and that comparison
    // has to land in the shared cache or repeated candidates recompute it forever.
    // Neither direction holds between number and string, so both are asked and the
    // first binding survives; two entries is the proof both went through the cache.
    #[test]
    fn competing_candidates_are_compared_through_the_subtype_cache() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let t = arena.alloc(Type::GenericParameter(id, "T".to_string(), None));
        let (number, string) = (arena.number(), arena.string());
        let mut bindings = Vec::new();
        let mut cache = SubtypeCache::default();

        infer_type_param_bindings(&mut arena, t, number, &mut bindings, &[], &mut cache);
        infer_type_param_bindings(&mut arena, t, string, &mut bindings, &[], &mut cache);

        assert_eq!(bindings, vec![(id, number)]);
        assert_eq!(cache.len(), 2);
    }

    fn property(name: &str, type_id: TypeId) -> PropertyEntry {
        PropertyEntry {
            name: name.into(),
            type_id,
            optional: false,
            is_method: false,
        }
    }

    fn property_type(arena: &TypeArena, object: TypeId, name: &str) -> TypeId {
        let Type::Object(object) = arena.get(object) else {
            panic!("expected an object");
        };
        object
            .properties
            .iter()
            .find(|p| &*p.name == name)
            .map(|p| p.type_id)
            .unwrap_or_else(|| panic!("no property {name}"))
    }

    // `Box<T> { value: T; next: Box<T> | null }` bound at T = number. The self
    // edge used to be cut and left on the generic original, so `next` still read
    // as `Box<T> | null` and `box.next.value` was T. It has to close on the new
    // object instead, and nothing generic may be reachable from the result.
    #[test]
    fn recursive_object_substitution_closes_on_the_copy() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let t = arena.alloc(Type::GenericParameter(id, "T".to_string(), None));
        let number = arena.number();
        let null = arena.null();

        let node = arena.alloc_object_placeholder();
        let next = arena.alloc_union(vec![node, null]);
        arena.set(
            node,
            Type::Object(ObjectType::new(vec![
                property("value", t),
                property("next", next),
            ])),
        );

        let bound = substitute_bound_type_params(&mut arena, node, &[(id, number)]);

        assert_ne!(bound, node);
        assert!(!contains_type_param(&arena, bound));
        assert_eq!(property_type(&arena, bound, "value"), number);

        let next = property_type(&arena, bound, "next");
        let Type::Union(members) = arena.get(next).clone() else {
            panic!("expected a union");
        };
        assert!(members.contains(&bound), "the edge must close on the copy");
        assert!(
            !members.contains(&node),
            "the generic original must not leak"
        );
    }

    // A -> B -> A, both mentioning T. Each copy has to point at the other copy,
    // not back at a generic original.
    #[test]
    fn mutually_recursive_substitution_stays_inside_the_copies() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let t = arena.alloc(Type::GenericParameter(id, "T".to_string(), None));
        let number = arena.number();

        let a = arena.alloc_object_placeholder();
        let b = arena.alloc_object_placeholder();
        arena.set(
            b,
            Type::Object(ObjectType::new(vec![
                property("item", t),
                property("back", a),
            ])),
        );
        arena.set(
            a,
            Type::Object(ObjectType::new(vec![
                property("item", t),
                property("other", b),
            ])),
        );

        let bound = substitute_bound_type_params(&mut arena, a, &[(id, number)]);

        assert!(!contains_type_param(&arena, bound));
        let other = property_type(&arena, bound, "other");
        assert_ne!(other, b);
        assert_eq!(property_type(&arena, other, "item"), number);
        assert_eq!(property_type(&arena, other, "back"), bound);
    }

    // A recursive type with nothing to substitute comes back untouched: no copy,
    // no second pass.
    #[test]
    fn recursive_object_without_type_parameters_is_returned_as_is() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let number = arena.number();
        let null = arena.null();
        let node = arena.alloc_object_placeholder();
        let next = arena.alloc_union(vec![node, null]);
        arena.set(
            node,
            Type::Object(ObjectType::new(vec![
                property("value", number),
                property("next", next),
            ])),
        );

        assert_eq!(
            substitute_bound_type_params(&mut arena, node, &[(id, number)]),
            node
        );
    }
}
