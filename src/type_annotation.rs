//! Resolves a TypeScript type annotation (`TSType`) to a `TypeId`. Lives at
//! the top level rather than under `bridge/` because both `bridge/` and
//! `namespace.rs` call into it. A named type reference (`Foo` in
//! `x: Foo`) has to look itself up in the namespace, which can in turn
//! call back in here to resolve an interface's property types.

use oxc_ast::ast::{PropertyKey, TSSignature, TSType, TSTypeAnnotation, TSTypeName};

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
/// function.
pub fn resolve_function_params(
    func: &oxc_ast::ast::Function,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<Vec<TypeId>> {
    let mut params = Vec::with_capacity(func.params.items.len());
    for param in &func.params.items {
        let annotation = param.type_annotation.as_ref()?;
        params.push(resolve_type_annotation(annotation, namespace, arena)?);
    }
    Some(params)
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
