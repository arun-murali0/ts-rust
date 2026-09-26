use crate::arena::{TypeArena, TypeId};
use crate::subtyping::is_subtype;
use crate::types::{FunctionType, ObjectType, PropertyEntry, Type};

// Structural inference: walks a generic function's declared parameter type
// alongside a call's actual argument type in lockstep, recording a binding the
// first time it reaches a GenericParameter leaf. Only argument-driven inference
// is supported; there is no explicit call-site syntax like identity<string>(x).
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
) {
    infer_type_param_bindings_inner(
        arena,
        param_type,
        arg_type,
        bindings,
        locked,
        &mut Vec::new(),
    )
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
    seen: &mut Vec<(TypeId, TypeId)>,
) {
    let pair = (param_type, arg_type);
    if seen.contains(&pair) {
        return;
    }
    seen.push(pair);
    infer_type_param_bindings_uncached(arena, param_type, arg_type, bindings, locked, seen);
    seen.pop();
}

fn infer_type_param_bindings_uncached(
    arena: &mut TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
    locked: &[crate::types::TypeParameterId],
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
                    if is_subtype(arena, candidate, *existing) {
                        // The existing binding already covers this candidate.
                    } else if is_subtype(arena, *existing, candidate) {
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
                seen,
            );
        }
        Type::Object(param_object) => {
            let Type::Object(arg_object) = arena.get(arg_type).clone() else {
                return;
            };
            for param_prop in &param_object.properties {
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
                    arena, p.type_id, a.type_id, bindings, locked, seen,
                );
            }
            infer_type_param_bindings_inner(
                arena,
                param_fn.return_type,
                arg_fn.return_type,
                bindings,
                locked,
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
                        !contains_type_param(arena, concrete) && is_subtype(arena, member, concrete)
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
                        arena, member, remainder, bindings, locked, seen,
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
    substitute_impl_inner(arena, type_id, bindings, keep_unbound, &mut Vec::new())
}

// A recursive type (Node { next: Node }, or a recursive generic like
// `Box<T> { value: T, next: Box<T> }`) means substituting inside type_id's
// properties can lead back to substituting type_id itself before the first
// call returns. Re-entering an id already on the current path is exactly the
// self-referential edge -- leaving it as type_id unchanged is correct there,
// since that edge and the type being substituted share the same identity by
// construction (namespace::resolve's placeholder backpatch).
fn substitute_impl_inner(
    arena: &mut TypeArena,
    type_id: TypeId,
    bindings: &[(crate::types::TypeParameterId, TypeId)],
    keep_unbound: bool,
    seen: &mut Vec<TypeId>,
) -> TypeId {
    if bindings.is_empty() || !contains_type_param(arena, type_id) {
        return type_id;
    }
    if seen.contains(&type_id) {
        return type_id;
    }
    seen.push(type_id);
    let result = substitute_impl_uncached(arena, type_id, bindings, keep_unbound, seen);
    seen.pop();
    result
}

fn substitute_impl_uncached(
    arena: &mut TypeArena,
    type_id: TypeId,
    bindings: &[(crate::types::TypeParameterId, TypeId)],
    keep_unbound: bool,
    seen: &mut Vec<TypeId>,
) -> TypeId {
    match arena.get(type_id).clone() {
        Type::GenericParameter(id, _, _) => {
            let bound = bindings
                .iter()
                .find(|(bound, _)| *bound == id)
                .map(|(_, resolved)| *resolved);
            match bound {
                Some(resolved) => resolved,
                None if keep_unbound => type_id,
                None => arena.unknown(),
            }
        }
        Type::Array(element) => {
            let substituted = substitute_impl_inner(arena, element, bindings, keep_unbound, seen);
            arena.alloc(Type::Array(substituted))
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|p| crate::types::Param {
                    type_id: substitute_impl_inner(arena, p.type_id, bindings, keep_unbound, seen),
                    optional: p.optional,
                    rest: p.rest,
                    name: p.name.clone(),
                })
                .collect();
            let return_type =
                substitute_impl_inner(arena, function.return_type, bindings, keep_unbound, seen);
            arena.alloc(Type::Function(FunctionType {
                params,
                return_type,
                is_untyped: function.is_untyped,
            }))
        }
        Type::Object(object) => {
            let properties = object
                .properties
                .iter()
                .map(|p| PropertyEntry {
                    name: p.name.clone(),
                    type_id: substitute_impl_inner(arena, p.type_id, bindings, keep_unbound, seen),
                    optional: p.optional,
                    is_method: p.is_method,
                })
                .collect();
            arena.alloc(Type::Object(ObjectType::new(properties)))
        }
        Type::Union(members) => {
            let substituted = members
                .iter()
                .map(|&m| substitute_impl_inner(arena, m, bindings, keep_unbound, seen))
                .collect();
            arena.alloc_union(substituted)
        }
        _ => type_id,
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
    ordered_generic_param_ids_inner(arena, type_id, out, &mut Vec::new());
    out.sort_by_key(|id| id.parameter_index());
}

// Same recursive-type hazard as everything else here: a self-referential
// property can revisit the same TypeId before the first visit returns.
// `seen` guards the walk itself; the `out.contains` check right below is a
// separate thing (it dedupes which *parameters* get collected) and does not,
// by itself, stop the walk from looping.
fn ordered_generic_param_ids_inner(
    arena: &TypeArena,
    type_id: TypeId,
    out: &mut Vec<crate::types::TypeParameterId>,
    seen: &mut Vec<TypeId>,
) {
    if seen.contains(&type_id) {
        return;
    }
    seen.push(type_id);
    match arena.get(type_id) {
        Type::GenericParameter(id, _, _) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Type::Array(element) => ordered_generic_param_ids_inner(arena, *element, out, seen),
        Type::Function(f) => {
            for param in &f.params {
                ordered_generic_param_ids_inner(arena, param.type_id, out, seen);
            }
            ordered_generic_param_ids_inner(arena, f.return_type, out, seen);
        }
        Type::Object(o) => {
            for property in &o.properties {
                ordered_generic_param_ids_inner(arena, property.type_id, out, seen);
            }
        }
        Type::Union(members) => {
            for &member in members {
                ordered_generic_param_ids_inner(arena, member, out, seen);
            }
        }
        _ => {}
    }
    seen.pop();
}

pub(crate) fn contains_type_param(arena: &TypeArena, type_id: TypeId) -> bool {
    contains_type_param_inner(arena, type_id, &mut Vec::new())
}

// A recursive type (Node { next: Node }, via namespace::resolve's placeholder
// backpatch) means type_id can be reachable from itself. Without tracking
// what's currently being visited, `next: Node` recurses into the same TypeId
// forever. `seen` only needs to hold ids currently on the call stack -- an id
// we've already finished with and returned from is fine to visit again if it
// shows up somewhere unrelated; it's only re-entering one we haven't
// unwound from yet that loops. A cycle contributes nothing new on its own,
// so re-entering one is treated as "no type parameter found this way".
fn contains_type_param_inner(arena: &TypeArena, type_id: TypeId, seen: &mut Vec<TypeId>) -> bool {
    if seen.contains(&type_id) {
        return false;
    }
    seen.push(type_id);
    let result = match arena.get(type_id) {
        Type::GenericParameter(_, _, _) => true,
        Type::Array(element) => contains_type_param_inner(arena, *element, seen),
        Type::Function(f) => {
            f.params
                .iter()
                .any(|p| contains_type_param_inner(arena, p.type_id, seen))
                || contains_type_param_inner(arena, f.return_type, seen)
        }
        Type::Object(o) => o
            .properties
            .iter()
            .any(|p| contains_type_param_inner(arena, p.type_id, seen)),
        Type::Union(members) => members
            .iter()
            .any(|&m| contains_type_param_inner(arena, m, seen)),
        _ => false,
    };
    seen.pop();
    result
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
    collect_generic_param_constraints_inner(arena, type_id, constraints, &mut Vec::new());
}

// Same walk-guard reasoning as ordered_generic_param_ids_inner: `seen` stops
// the traversal from looping through a recursive type; the constraint-level
// `constraints.iter().any(...)` dedup below is a separate concern (which
// *parameters* get recorded) and does not by itself bound the recursion.
fn collect_generic_param_constraints_inner(
    arena: &TypeArena,
    type_id: TypeId,
    constraints: &mut Vec<(crate::types::TypeParameterId, String, TypeId)>,
    seen: &mut Vec<TypeId>,
) {
    if seen.contains(&type_id) {
        return;
    }
    seen.push(type_id);
    match arena.get(type_id) {
        Type::GenericParameter(id, name, Some(constraint)) => {
            if !constraints.iter().any(|(existing, _, _)| existing == id) {
                constraints.push((*id, name.clone(), *constraint));
            }
        }
        Type::GenericParameter(_, _, None) => {}
        Type::Array(element) => {
            collect_generic_param_constraints_inner(arena, *element, constraints, seen)
        }
        Type::Function(f) => {
            for param in &f.params {
                collect_generic_param_constraints_inner(arena, param.type_id, constraints, seen);
            }
            collect_generic_param_constraints_inner(arena, f.return_type, constraints, seen);
        }
        Type::Object(o) => {
            for property in &o.properties {
                collect_generic_param_constraints_inner(arena, property.type_id, constraints, seen);
            }
        }
        Type::Union(members) => {
            for &member in members {
                collect_generic_param_constraints_inner(arena, member, constraints, seen);
            }
        }
        _ => {}
    }
    seen.pop();
}
