//! Resolves a TypeScript type annotation (`TSType`) to a `TypeId`. Lives at
//! the top level rather than under `bridge/` because both `bridge/` and
//! `namespace.rs` call into it. A named type reference (`Foo` in
//! `x: Foo`) has to look itself up in the namespace, which can in turn
//! call back in here to resolve an interface's property types.

use oxc_ast::ast::{FormalParameters, PropertyKey, TSSignature, TSType, TSTypeAnnotation, TSTypeName};

use crate::arena::{TypeArena, TypeId};
use crate::namespace::{Resolution, TypeNamespace};
use crate::types::{ObjectType, PropertyEntry, Type};

pub fn resolve_type_annotation(
    annotation: &TSTypeAnnotation,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<TypeId> {
    resolve_ts_type(&annotation.type_annotation, namespace, arena)
}

pub fn resolve_ts_type(ty: &TSType, namespace: &mut TypeNamespace, arena: &mut TypeArena) -> Option<TypeId> {
    match ty {
        TSType::TSNumberKeyword(_) => Some(arena.number()),
        TSType::TSStringKeyword(_) => Some(arena.string()),
        TSType::TSBooleanKeyword(_) => Some(arena.boolean()),
        TSType::TSNullKeyword(_) => Some(arena.null()),
        TSType::TSUndefinedKeyword(_) => Some(arena.undefined()),
        TSType::TSAnyKeyword(_) => Some(arena.any()),
        TSType::TSUnknownKeyword(_) => Some(arena.unknown()),

        TSType::TSArrayType(array) => {
            let element = resolve_ts_type(&array.element_type, namespace, arena)?;
            Some(arena.alloc(Type::Array(element)))
        }

        TSType::TSUnionType(union) => {
            let mut members = Vec::with_capacity(union.types.len());
            for member in &union.types {
                members.push(resolve_ts_type(member, namespace, arena)?);
            }
            Some(arena.alloc_union(members))
        }

        TSType::TSTypeLiteral(literal) => resolve_object_members(&literal.members, namespace, arena),

        TSType::TSFunctionType(func_type) => {
            // Uses the any-fallback resolver, not the strict
            // resolve_function_params below: `(x) => number` written as
            // a type (e.g. a callback parameter's own annotation) is
            // exactly as legal in real TS as an untyped arrow-function
            // *value* parameter, and should default to `any` the same
            // way rather than making the whole annotation unresolvable.
            // Previously this used resolve_function_params, which failed
            // the entire TSFunctionType if even one of its own params
            // lacked an annotation — inconsistent with how an actual
            // arrow-function value handles the identical gap.
            let params = resolve_params_with_any_fallback(&func_type.params, namespace, arena);
            let return_type = resolve_type_annotation(&func_type.return_type, namespace, arena)?;
            Some(arena.alloc(Type::Function(crate::types::FunctionType { params, return_type, is_untyped: false })))
        }

        TSType::TSLiteralType(literal) => resolve_literal_type(&literal.literal, arena),

        TSType::TSTypeReference(reference) => {
            let TSTypeName::IdentifierReference(id) = &reference.type_name else {
                // Qualified names (`Namespace.Type`) aren't resolved in V2.
                return None;
            };
            match namespace.resolve(&id.name, arena) {
                Resolution::Resolved(type_id) => Some(type_id),
                Resolution::Circular | Resolution::NotFound => None,
            }
        }

        _ => None,
    }
}

fn resolve_literal_type(literal: &oxc_ast::ast::TSLiteral, arena: &mut TypeArena) -> Option<TypeId> {
    use oxc_ast::ast::TSLiteral;
    match literal {
        TSLiteral::StringLiteral(s) => Some(arena.alloc(Type::StringLiteral(s.value.to_string()))),
        TSLiteral::NumericLiteral(n) => Some(arena.alloc(Type::NumberLiteral(n.value))),
        TSLiteral::BooleanLiteral(b) => Some(arena.alloc(Type::BooleanLiteral(b.value))),
        // Negative number literals (`-1`), BigInt, and template literal
        // types aren't handled yet.
        _ => None,
    }
}

/// Resolves every parameter's type annotation, in order. Returns `None`
/// (skip the whole function/method rather than guess) if even one
/// parameter has no annotation, matching how declare.rs treats a plain
/// function. Takes the parameter list directly, not a whole `&Function`,
/// since a `TSFunctionType` (a function used as a *type*, e.g. the
/// annotation `(x: number) => number`) has the same `FormalParameters`
/// shape as a real function but isn't a `Function` node itself.
pub fn resolve_function_params(
    params: &FormalParameters,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<Vec<TypeId>> {
    let mut resolved = Vec::with_capacity(params.items.len());
    for param in &params.items {
        let annotation = param.type_annotation.as_ref()?;
        resolved.push(resolve_type_annotation(annotation, namespace, arena)?);
    }
    Some(resolved)
}

/// A parameter with no annotation gets `any` rather than being treated as
/// a reason to reject the whole parameter list, unlike
/// `resolve_function_params` above. That strict all-or-nothing behavior
/// is right for a top-level `function` declaration or a class
/// member (see `declare.rs` and `namespace.rs`'s resolve_class): nothing
/// calls a function before pass 2 has fully hoisted it, so there's no
/// harm in simply not registering a partially-annotated one. An arrow
/// function or function expression, though — or a `TSFunctionType`
/// written as an annotation, e.g. a callback parameter's own type
/// `(x) => number` — IS the value (or governs the value) at the point
/// it's written; callbacks like `arr.map(x => x + 1)` are exactly this
/// shape and are extremely common with no annotation on `x` at all.
/// Falling back to `any` per parameter is what lets the function still
/// get a real `Type::Function` instead of degrading to the error
/// sentinel for the single most common real-world use of a function
/// value.
pub fn resolve_params_with_any_fallback(
    params: &FormalParameters,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Vec<TypeId> {
    params
        .items
        .iter()
        .map(|param| {
            param
                .type_annotation
                .as_ref()
                .and_then(|annotation| resolve_type_annotation(annotation, namespace, arena))
                .unwrap_or_else(|| arena.any())
        })
        .collect()
}

/// Builds an object type from a property list, sorting by name so
/// `subtyping.rs` can compare objects with a merge-join. Returns `None`
/// (rather than a partial object) if any member isn't a plain, understood
/// property signature. A method or index signature makes the whole type
/// not-yet-representable, and a half-built object type would be worse than
/// an honest "unresolved."
pub fn resolve_object_members(
    members: &[TSSignature],
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<TypeId> {
    let mut properties = Vec::with_capacity(members.len());

    for member in members {
        let TSSignature::TSPropertySignature(property) = member else { return None };
        let PropertyKey::StaticIdentifier(key) = &property.key else { return None };
        let annotation = property.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        properties.push(PropertyEntry { name: key.name.to_string(), type_id, optional: property.optional });
    }

    properties.sort_by(|a, b| a.name.cmp(&b.name));
    Some(arena.alloc(Type::Object(ObjectType { properties })))
}
