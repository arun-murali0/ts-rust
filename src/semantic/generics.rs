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
    arena: &TypeArena,
    param_type: TypeId,
    arg_type: TypeId,
    bindings: &mut Vec<(crate::types::TypeParameterId, TypeId)>,
) {
    match arena.get(param_type) {
        Type::GenericParameter(id, _, _) => {
            // Widened so identity(5) infers number, matching what a
            // TypeScript author expects from a bare generic call, rather than
            // the checker binding T to the narrower literal type 5.
            let candidate = crate::types::widen(arena, arg_type);

            match bindings.iter_mut().find(|(bound, _)| bound == id) {
                None => bindings.push((*id, candidate)),
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
            if let Type::Array(arg_element) = arena.get(arg_type) {
                infer_type_param_bindings(arena, *param_element, *arg_element, bindings);
            }
        }
        Type::Object(param_object) => {
            if let Type::Object(arg_object) = arena.get(arg_type) {
                for param_prop in &param_object.properties {
                    if let Some(arg_prop) = arg_object
                        .properties
                        .iter()
                        .find(|p| p.name == param_prop.name)
                    {
                        infer_type_param_bindings(
                            arena,
                            param_prop.type_id,
                            arg_prop.type_id,
                            bindings,
                        );
                    }
                }
            }
        }
        Type::Function(param_fn) => {
            if let Type::Function(arg_fn) = arena.get(arg_type) {
                for (p, a) in param_fn.params.iter().zip(&arg_fn.params) {
                    infer_type_param_bindings(arena, p.type_id, a.type_id, bindings);
                }
                infer_type_param_bindings(
                    arena,
                    param_fn.return_type,
                    arg_fn.return_type,
                    bindings,
                );
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
    if bindings.is_empty() || !contains_type_param(arena, type_id) {
        return type_id;
    }

    match arena.get(type_id).clone() {
        Type::GenericParameter(id, _, _) => bindings
            .iter()
            .find(|(bound, _)| *bound == id)
            .map(|(_, resolved)| *resolved)
            .unwrap_or_else(|| arena.unknown()),
        Type::Array(element) => {
            let substituted = substitute_type_params(arena, element, bindings);
            arena.alloc(Type::Array(substituted))
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|p| crate::types::Param {
                    type_id: substitute_type_params(arena, p.type_id, bindings),
                    optional: p.optional,
                    rest: p.rest,
                })
                .collect();
            let return_type = substitute_type_params(arena, function.return_type, bindings);
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
                    type_id: substitute_type_params(arena, p.type_id, bindings),
                    optional: p.optional,
                })
                .collect();
            arena.alloc(Type::Object(ObjectType { properties }))
        }
        Type::Union(members) => {
            let substituted = members
                .iter()
                .map(|&m| substitute_type_params(arena, m, bindings))
                .collect();
            arena.alloc_union(substituted)
        }
        _ => type_id,
    }
}

pub(crate) fn contains_type_param(arena: &TypeArena, type_id: TypeId) -> bool {
    match arena.get(type_id) {
        Type::GenericParameter(_, _, _) => true,
        Type::Array(element) => contains_type_param(arena, *element),
        Type::Function(f) => {
            f.params
                .iter()
                .any(|p| contains_type_param(arena, p.type_id))
                || contains_type_param(arena, f.return_type)
        }
        Type::Object(o) => o
            .properties
            .iter()
            .any(|p| contains_type_param(arena, p.type_id)),
        Type::Union(members) => members.iter().any(|&m| contains_type_param(arena, m)),
        _ => false,
    }
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
    match arena.get(type_id) {
        Type::GenericParameter(id, name, Some(constraint)) => {
            if !constraints.iter().any(|(existing, _, _)| existing == id) {
                constraints.push((*id, name.clone(), *constraint));
            }
        }
        Type::GenericParameter(_, _, None) => {}
        Type::Array(element) => collect_generic_param_constraints(arena, *element, constraints),
        Type::Function(f) => {
            for param in &f.params {
                collect_generic_param_constraints(arena, param.type_id, constraints);
            }
            collect_generic_param_constraints(arena, f.return_type, constraints);
        }
        Type::Object(o) => {
            for property in &o.properties {
                collect_generic_param_constraints(arena, property.type_id, constraints);
            }
        }
        Type::Union(members) => {
            for &member in members {
                collect_generic_param_constraints(arena, member, constraints);
            }
        }
        _ => {}
    }
}
