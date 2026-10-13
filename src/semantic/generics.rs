use crate::arena::{TypeArena, TypeId};
use crate::fxhash::{FxBuildHasher, FxHashMap};
use crate::semantic::queries::{RelationCache, SemanticQueries};
use crate::types::{FunctionType, ObjectType, PropertyEntry, Type};
use smallvec::SmallVec;
use std::collections::HashSet;

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
    cache: &mut RelationCache,
) {
    infer_type_param_bindings_inner(
        arena,
        param_type,
        arg_type,
        bindings,
        locked,
        cache,
        &mut SeenPairs::default(),
    )
}

// Inference mutates the arena (widening and union building allocate), so it cannot
// keep one SemanticQueries alive across its steps: that holds a shared borrow of the
// arena for its whole life. Building one per question keeps the borrow to the call.
// The reason to go through it at all is that candidates are compared against the same
// existing binding over and over (`allSame(1, 2, ..., 8)`), and those repeats were the
// one place the cache never saw.
fn is_subtype_via_cache(
    arena: &TypeArena,
    cache: &mut RelationCache,
    sub: TypeId,
    sup: TypeId,
) -> bool {
    SemanticQueries::new(arena, cache).is_subtype(sub, sup)
}

// Whether a bound asks for narrow types: it is, or contains, a primitive or a literal.
fn bound_keeps_literals(arena: &TypeArena, bound: TypeId) -> bool {
    let is_primitive_or_literal = |id: TypeId| {
        matches!(
            arena.get(id),
            Type::String
                | Type::Number
                | Type::Boolean
                | Type::StringLiteral(_)
                | Type::NumberLiteral(_)
                | Type::BooleanLiteral(_)
        )
    };
    match arena.get(bound) {
        Type::Union(members) => members
            .iter()
            .any(|&member| is_primitive_or_literal(member)),
        _ => is_primitive_or_literal(bound),
    }
}

// Which primitive a literal, or a union made only of literals, belongs to. None for
// anything else, and for a union that mixes primitives.
fn literal_base(arena: &TypeArena, id: TypeId) -> Option<u8> {
    let base_of = |id: TypeId| match arena.get(id) {
        Type::StringLiteral(_) => Some(0),
        Type::NumberLiteral(_) => Some(1),
        Type::BooleanLiteral(_) => Some(2),
        _ => None,
    };
    match arena.get(id) {
        Type::Union(members) => {
            let mut bases = members.iter().map(|&member| base_of(member));
            let first = bases.next()??;
            bases.all(|base| base == Some(first)).then_some(first)
        }
        _ => base_of(id),
    }
}

fn same_literal_base(arena: &TypeArena, a: TypeId, b: TypeId) -> bool {
    matches!((literal_base(arena, a), literal_base(arena, b)), (Some(x), Some(y)) if x == y)
}

// The (param_type, arg_type) pairs one inference call has already matched, or is
// matching right now.
type SeenPairs = HashSet<(TypeId, TypeId), FxBuildHasher>;

// A recursive param_type (e.g. `Box<T>`'s own self-referential `next: Box<T>`,
// still holding the bare GenericParameter shape while it's being matched
// against a real argument) can lead back to matching the same (param_type,
// arg_type) pair against itself before the first match finishes. `seen` holds the
// pairs this call has started on; reaching one again contributes nothing further
// worth inferring, so it's simply skipped rather than walked again.
//
// Problem: `seen` used to be a stack, and a pair was removed again as soon as its
// match finished. That only stops a pair from containing itself; the same pair
// reached by a different route was matched in full again. With classes that point
// at each other (`class C0 { p1: C1; p2: C2; ... }`, as in TypeScript's
// resolvingClassDeclarationWhenInBaseTypeResolution) the number of routes grows
// exponentially, and a call such as `use(c0)` for a parameter of type C0 never
// finished, even though nothing in it is generic.
// Picked: keep every pair for the rest of the call. A pair matched a second time
// would offer the same candidate to the same parameters, and offering a candidate
// the binding already covers changes nothing, so the only thing skipped is repeated
// work. tsc keeps its own visited pairs for the whole inference in the same way.
// Cost: if a binding is widened after a pair was matched, that pair is not offered
// again against the wider binding. Both the first match and a repeat are compared
// with the binding as it stands, so a candidate that was ignored once for being
// unrelated to the binding can no longer be picked up by a later route to it.
fn infer_type_param_bindings_inner(
    arena: &mut TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
    locked: &[crate::types::TypeParameterId],
    cache: &mut RelationCache,
    seen: &mut SeenPairs,
) {
    if !seen.insert((param_type, arg_type)) {
        return;
    }
    infer_type_param_bindings_uncached(arena, param_type, arg_type, bindings, locked, cache, seen);
}

fn infer_type_param_bindings_uncached(
    arena: &mut TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
    locked: &[crate::types::TypeParameterId],
    cache: &mut RelationCache,
    seen: &mut SeenPairs,
) {
    match arena.get(param_type).clone() {
        Type::GenericParameter(id, _, bound) => {
            // A parameter bound by an explicit call-site type argument
            // (identity<string>(x)) is not up for renegotiation by whatever
            // argument happens to line up with it structurally; explicit
            // wins outright, the same way TypeScript itself treats an
            // explicit type argument as authoritative rather than a hint.
            if locked.contains(&id) {
                return;
            }

            // A literal argument is widened, so identity(5) infers number and not
            // the literal 5, which is what a bare generic call is expected to give.
            // Unless the parameter's bound mentions a primitive or literal type
            // (`T extends "a" | "b"`, `T extends number`): the bound is then asking
            // for the narrow type, widening would break it (`pick("a")` would bind T
            // to string, which does not satisfy `"a" | "b"`), and TypeScript keeps the
            // literal in exactly this case.
            let keeps_literals = bound.is_some_and(|bound| bound_keeps_literals(arena, bound));
            let candidate = if keeps_literals {
                arg_type
            } else {
                crate::types::widen(arena, arg_type)
            };

            match bindings.iter_mut().find(|(bound, _)| *bound == id) {
                None => bindings.push((id, candidate)),
                Some((_, existing)) => {
                    if is_subtype_via_cache(arena, cache, candidate, *existing) {
                        // The existing binding already covers this candidate.
                    } else if is_subtype_via_cache(arena, cache, *existing, candidate) {
                        *existing = candidate;
                    } else if keeps_literals && same_literal_base(arena, *existing, candidate) {
                        // `pair(1, 2)` for `T extends number`: neither literal covers
                        // the other, but both are numbers, so T is 1 | 2. Literals of
                        // different primitives (`pair("a", 1)`) are not combined; the
                        // first binding stays and the second argument is reported.
                        *existing = arena.alloc_union(vec![*existing, candidate]);
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
        // A parameter typed `T & { id: number }`. tsc infers into each member from the
        // argument as a whole, so `T` here is the argument's own type, and a member
        // with no type parameter in it has nothing to say.
        Type::Intersection(param_members) => {
            for member in param_members {
                if contains_type_param(arena, member) {
                    infer_type_param_bindings_inner(
                        arena, member, arg_type, bindings, locked, cache, seen,
                    );
                }
            }
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
                            && is_subtype_via_cache(arena, cache, member, concrete)
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
    // Nothing bound, nothing to rewrite: the id comes back as it is and no scratch state
    // is built.
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
// after interning, so a subtype reachable along several paths is rewritten once
// (`rewritten`) instead of once per path, and "does this contain a parameter" is
// answered from the arena's own cache (see contains_type_param) instead of a fresh
// walk at every level.
//
// `rewritten` does not outlive the call. A declaration's Ref can be resolved by
// TypeArena::resolve_ref between two calls, which would make a longer-lived rewrite
// stale.
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
    // builds it again with a Ref of its own, so that the edge closes on the new object
    // instead. Only an Object can close a cycle (see TypeArena), which
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
            // rewrite_recursive registers its Ref before descending; when
            // the result turns out not to be reusable, that entry must go too.
            self.rewritten.remove(&type_id);
        }
        *lowest_cut = (*lowest_cut).min(cut_below);
        result
    }

    // Rebuilds a self-referential object around a Ref of its own. The Ref is
    // registered as type_id's rewrite *before* the properties are walked, so the walk
    // finds it where the first pass found the cut, and the edge that closed on
    // type_id closes on the copy. The Ref is its own id (see TypeArena::alloc_ref),
    // which is what a recursive shape needs: other types hold that id, so it must
    // never be merged with another declaration. Its body is an ordinary Object.
    fn rewrite_recursive(
        &mut self,
        arena: &mut TypeArena,
        type_id: TypeId,
        cut: &mut usize,
    ) -> TypeId {
        let Type::Object(object) = arena.get(type_id).clone() else {
            return type_id;
        };
        let copy = arena.alloc_ref();
        self.rewritten.insert(type_id, copy);

        let mut properties = Vec::with_capacity(object.properties.len());
        for p in object.properties.iter() {
            properties.push(PropertyEntry {
                name: p.name.clone(),
                type_id: self.rewrite(arena, p.type_id, cut),
                optional: p.optional,
                is_method: p.is_method,
            });
        }
        let body = arena.alloc(Type::Object(ObjectType::new(properties)));
        arena.resolve_ref(copy, body);
        copy
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
            // Rebuilt through alloc_intersection, not allocated as it stands: replacing a
            // parameter can create something that reduces (`T & number` with `T := string`
            // is `never`), and a node that skipped the reduction would not be canonical.
            // Distributing over a union that is too large cannot be reported from here,
            // so it becomes the error type, which is already reported where it was written.
            Type::Intersection(members) => {
                let mut substituted = Vec::with_capacity(members.len());
                for &m in &members {
                    substituted.push(self.rewrite(arena, m, cut));
                }
                arena
                    .alloc_intersection(substituted)
                    .unwrap_or_else(|_| arena.error())
            }
            _ => type_id,
        }
    }
}

// Collects every distinct type parameter appearing in type_id, then hands them
// back in declaration order rather than tree-walk-encounter order. An explicit
// call-site type argument list (identity<string, number>(x, y)) has no
// declaration span of its own to key a TypeParameterId lookup off of (it is
// just a positional list), so the only way to zip it correctly against a
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
    ordered_generic_param_ids_inner(arena, type_id, out, &mut SmallVec::new());
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
    visited: &mut SmallVec<[TypeId; 16]>,
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
        Type::Union(members) | Type::Intersection(members) => {
            for &member in members {
                ordered_generic_param_ids_inner(arena, member, out, visited);
            }
        }
        _ => {}
    }
}

pub(crate) fn contains_type_param(arena: &TypeArena, type_id: TypeId) -> bool {
    if let Some(known) = arena.cached_mentions_type_param(type_id) {
        return known;
    }
    let mut visited: HashSet<TypeId, FxBuildHasher> = HashSet::default();
    let found = scan_for_type_param(arena, type_id, &mut visited);
    if !found {
        // The walk reached every type reachable from type_id and none of them is a
        // parameter, so the same is true of each type it passed through: whatever
        // one of them reaches, type_id reaches too. All of them are remembered in
        // one go, which is what keeps the next question about any of them to a lookup.
        for id in visited {
            arena.remember_mentions_type_param(id, false);
        }
    }
    found
}

// The "does this type mention a type parameter" question. Answers are kept in
// the arena (TypeArena::cached_mentions_type_param), not per call: the checker
// asks it for every call expression, mostly about types that never change, and
// interning has turned the type graph into a DAG, so the same `number[]` or
// `{ id: number }` node hangs off many parents.
//
// Problem: the walk used to keep only the ids on its current path, and to
// remember a `false` only for a node whose walk had not been cut short by a
// cycle. In a graph where classes refer to each other (Node { next: Node }, through
// the Ref namespace::resolve gives its declaration), almost every node is cut by
// some cycle, so almost nothing was remembered and a node shared by many parents
// was walked again for every path that led to it. The number of paths grows
// exponentially with the number of classes: TypeScript's own
// resolvingClassDeclarationWhenInBaseTypeResolution test, about 50 generic classes
// that refer to each other across namespaces, never finished.
// Picked: one visited set for the whole question. A node already visited is not
// walked again, so the walk is linear in the types reachable from the start.
// Returning `false` for it is right whatever is still open above it: the question
// is only whether some parameter is reachable, so if there is one, it will be
// found by the walk that is still running, and the `true` it produces travels up
// through every open node (and is remembered for each) without looking at the
// visited node's answer again.
// Cost: a `true` is only remembered for the nodes on the path to the parameter, not
// for the other nodes the walk visited first; asking about those again walks them
// again, once. A `false` is remembered for every visited node, see
// contains_type_param.
fn scan_for_type_param(
    arena: &TypeArena,
    type_id: TypeId,
    visited: &mut HashSet<TypeId, FxBuildHasher>,
) -> bool {
    if let Some(known) = arena.cached_mentions_type_param(type_id) {
        return known;
    }
    if !visited.insert(type_id) {
        return false;
    }

    let found = match arena.get(type_id) {
        Type::GenericParameter(_, _, _) => true,
        Type::Array(element) => scan_for_type_param(arena, *element, visited),
        Type::Function(f) => {
            f.params
                .iter()
                .any(|p| scan_for_type_param(arena, p.type_id, visited))
                || scan_for_type_param(arena, f.return_type, visited)
        }
        Type::Object(o) => o
            .properties
            .iter()
            .any(|p| scan_for_type_param(arena, p.type_id, visited)),
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .any(|&m| scan_for_type_param(arena, m, visited)),
        _ => false,
    };

    if found {
        arena.remember_mentions_type_param(type_id, true);
    }
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
    collect_generic_param_constraints_inner(arena, type_id, constraints, &mut SmallVec::new());
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
    visited: &mut SmallVec<[TypeId; 16]>,
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
        Type::Union(members) | Type::Intersection(members) => {
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
    fn competing_candidates_are_compared_through_the_relation_cache() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let t = arena.alloc(Type::GenericParameter(id, "T".to_string(), None));
        let (number, string) = (arena.number(), arena.string());
        let mut bindings = Vec::new();
        let mut cache = RelationCache::default();

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
    // edge must close on the new object, not stay on the generic original:
    // otherwise `next` would still read as `Box<T> | null` and `box.next.value`
    // would be T. Nothing generic may be reachable from the result.
    #[test]
    fn recursive_object_substitution_closes_on_the_copy() {
        let mut arena = TypeArena::new();
        let id = TypeParameterId::new(0, 0);
        let t = arena.alloc(Type::GenericParameter(id, "T".to_string(), None));
        let number = arena.number();
        let null = arena.null();

        let node = arena.alloc_ref();
        let next = arena.alloc_union(vec![node, null]);
        let body = arena.alloc(Type::Object(ObjectType::new(vec![
            property("value", t),
            property("next", next),
        ])));
        arena.resolve_ref(node, body);

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

        let a = arena.alloc_ref();
        let b = arena.alloc_ref();
        let b_body = arena.alloc(Type::Object(ObjectType::new(vec![
            property("item", t),
            property("back", a),
        ])));
        arena.resolve_ref(b, b_body);
        let a_body = arena.alloc(Type::Object(ObjectType::new(vec![
            property("item", t),
            property("other", b),
        ])));
        arena.resolve_ref(a, a_body);

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
        let node = arena.alloc_ref();
        let next = arena.alloc_union(vec![node, null]);
        let body = arena.alloc(Type::Object(ObjectType::new(vec![
            property("value", number),
            property("next", next),
        ])));
        arena.resolve_ref(node, body);

        assert_eq!(
            substitute_bound_type_params(&mut arena, node, &[(id, number)]),
            node
        );
    }
}
