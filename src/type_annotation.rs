use oxc_ast::ast::{
    BindingPattern, FormalParameters, PropertyKey, TSSignature, TSType, TSTypeAnnotation,
    TSTypeName,
};

use crate::arena::{TypeArena, TypeId};
use crate::namespace::{Resolution, TypeNamespace};
use crate::types::{ObjectType, Param, PropertyEntry, Type};

pub fn resolve_type_annotation(
    annotation: &TSTypeAnnotation,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<TypeId> {
    resolve_ts_type(&annotation.type_annotation, namespace, arena)
}

pub fn resolve_ts_type(
    ty: &TSType,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<TypeId> {
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

        TSType::TSTypeLiteral(literal) => {
            resolve_object_members(&literal.members, namespace, arena)
        }

        TSType::TSFunctionType(func_type) => {
            let params = resolve_params_with_any_fallback(&func_type.params, namespace, arena);
            let return_type = resolve_type_annotation(&func_type.return_type, namespace, arena)?;
            Some(arena.alloc(Type::Function(crate::types::FunctionType {
                params,
                return_type,
                is_untyped: false,
            })))
        }

        TSType::TSLiteralType(literal) => resolve_literal_type(&literal.literal, arena),

        TSType::TSTypeReference(reference) => {
            let TSTypeName::IdentifierReference(id) = &reference.type_name else {
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

fn resolve_literal_type(
    literal: &oxc_ast::ast::TSLiteral,
    arena: &mut TypeArena,
) -> Option<TypeId> {
    use oxc_ast::ast::TSLiteral;
    match literal {
        TSLiteral::StringLiteral(s) => Some(arena.alloc(Type::StringLiteral(s.value.to_string()))),
        TSLiteral::NumericLiteral(n) => Some(arena.alloc(Type::NumberLiteral(n.value))),
        TSLiteral::BooleanLiteral(b) => Some(arena.alloc(Type::BooleanLiteral(b.value))),

        _ => None,
    }
}

pub fn resolve_function_params(
    params: &FormalParameters,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<Vec<Param>> {
    let mut resolved = Vec::with_capacity(params.items.len() + 1);
    for param in &params.items {
        let annotation = param.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        resolved.push(Param {
            type_id,
            optional: param.optional
                || matches!(param.pattern, BindingPattern::AssignmentPattern(_)),
            rest: false,
        });
    }

    if let Some(rest) = &params.rest {
        let annotation = rest.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        resolved.push(Param {
            type_id,
            optional: false,
            rest: true,
        });
    }

    Some(resolved)
}

pub fn resolve_params_with_any_fallback(
    params: &FormalParameters,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Vec<Param> {
    let mut resolved: Vec<Param> = params
        .items
        .iter()
        .map(|param| {
            let type_id = param
                .type_annotation
                .as_ref()
                .and_then(|annotation| resolve_type_annotation(annotation, namespace, arena))
                .unwrap_or_else(|| arena.any());
            Param {
                type_id,
                optional: param.optional
                    || matches!(param.pattern, BindingPattern::AssignmentPattern(_)),
                rest: false,
            }
        })
        .collect();

    if let Some(rest) = &params.rest {
        let type_id = rest
            .type_annotation
            .as_ref()
            .and_then(|annotation| resolve_type_annotation(annotation, namespace, arena))
            .unwrap_or_else(|| {
                let any = arena.any();
                arena.alloc(Type::Array(any))
            });
        resolved.push(Param {
            type_id,
            optional: false,
            rest: true,
        });
    }

    resolved
}

pub fn resolve_object_members(
    members: &[TSSignature],
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<TypeId> {
    let mut properties = Vec::with_capacity(members.len());

    for member in members {
        let TSSignature::TSPropertySignature(property) = member else {
            return None;
        };
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            return None;
        };
        let annotation = property.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        properties.push(PropertyEntry {
            name: key.name.to_string(),
            type_id,
            optional: property.optional,
        });
    }

    properties.sort_by(|a, b| a.name.cmp(&b.name));
    Some(arena.alloc(Type::Object(ObjectType { properties })))
}
