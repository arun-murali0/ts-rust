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
            namespace.note_implicit_any_params(&func_type.params);
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
            let base = match namespace.resolve(&id.name, arena) {
                Resolution::Resolved(type_id) => type_id,
                Resolution::Circular | Resolution::NotFound => return None,
            };

            // A reference whose type argument count disagrees with its
            // declaration is recorded, not rejected here: resolution carries on
            // with the same lenient treatment below, and the mismatch is reported
            // afterward as a diagnostic (see bridge::check_program). A bare `Box`
            // for `interface Box<T>` is the given == 0 case, reported as a
            // warning rather than an error.
            let given = reference
                .type_arguments
                .as_ref()
                .map_or(0, |arguments| arguments.params.len());
            if let Some(expected) = namespace.declared_type_param_count(&id.name) {
                if expected != given {
                    namespace.note_type_argument_issue(&id.name, expected, given, reference.span);
                }
            }

            // No explicit type arguments (a bare `Box`, or a reference to a
            // non-generic type) -- nothing to substitute.
            let Some(type_arguments) = &reference.type_arguments else {
                return Some(base);
            };

            // Each explicit argument (the `number` in `Box<number>`) is resolved
            // in the *caller's* namespace, not the callee's, exactly like a
            // generic function call's explicit type arguments in calls.rs.
            let explicit: Vec<TypeId> = type_arguments
                .params
                .iter()
                .filter_map(|ty| resolve_ts_type(ty, namespace, arena))
                .collect();
            if explicit.is_empty() {
                return Some(base);
            }

            // base is Box's own cached generic shape -- an Object/Function/etc
            // still containing bare GenericParameter placeholders for T, in the
            // order `interface Box<T, ...>` declared them (see
            // TypeNamespace::resolve). Recovering that order the same way a
            // generic call's explicit type arguments do lets `Box<number>` and
            // `identity<string>(x)` share one substitution mechanism rather than
            // needing two.
            let mut ordered_ids = Vec::new();
            crate::semantic::ordered_generic_param_ids(arena, base, &mut ordered_ids);
            if ordered_ids.is_empty() {
                // Box itself isn't generic (or has no type parameters this
                // checker resolved) -- type arguments given to it are ignored
                // rather than substituted into nothing, the same lenient
                // "erase what can't be honored" stance taken everywhere else in
                // this function.
                return Some(base);
            }

            let bindings: Vec<_> = ordered_ids
                .iter()
                .zip(explicit.iter())
                .map(|(&id, &resolved)| (id, resolved))
                .collect();
            Some(crate::semantic::substitute_type_params(
                arena, base, &bindings,
            ))
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
            // A parameter with a default value (`x = 1`) is omittable at call
            // sites the same way an explicitly optional `x?: T` parameter is,
            // even without its own `?`, matching real TypeScript's arity rules.
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
        // As with class members in namespace.rs, one member this checker cannot
        // represent, such as a method signature or index signature inside an
        // interface, makes the whole interface unresolvable rather than silently
        // dropping just that member.
        let TSSignature::TSPropertySignature(property) = member else {
            return None;
        };
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            return None;
        };
        let annotation = property.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        properties.push(PropertyEntry {
            name: key.name.to_string().into(),
            type_id,
            optional: property.optional,
        });
    }

    Some(arena.alloc(Type::Object(ObjectType::new(properties))))
}
