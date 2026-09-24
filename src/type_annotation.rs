use oxc_ast::ast::{
    BindingPattern, FormalParameters, PropertyKey, TSSignature, TSType, TSTypeAnnotation,
    TSTypeName,
};
use oxc_span::GetSpan;

use crate::arena::{TypeArena, TypeId};
use crate::namespace::{Resolution, TypeNamespace};
use crate::types::{ObjectType, Param, PropertyEntry, Type, TypeParameterId};

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
            // leniently below, and the mismatch is reported afterward (see
            // bridge::check_program). Parameters with a default may be omitted,
            // so the valid range is required..=total, as in tsc.
            let given = reference
                .type_arguments
                .as_ref()
                .map_or(0, |arguments| arguments.params.len());
            let arity = namespace.declared_type_param_arity(&id.name);
            if let Some((required, total)) = arity {
                if given < required || given > total {
                    namespace.note_type_argument_issue(
                        &id.name,
                        required,
                        total,
                        given,
                        reference.span,
                    );
                }
            }

            // Not generic, or a declaration this checker does not model (a
            // class): nothing to substitute.
            let Some(decl) = namespace.declared_type_param_decl(&id.name) else {
                return Some(base);
            };

            // A bare `Box` for `interface Box<T>` was reported above. As in tsc
            // the reference is then the error type, which is compatible with
            // everything, so the missing arguments cannot cascade into a second,
            // unrelated diagnostic and no bare placeholder leaks out.
            if given == 0 && arity.is_some_and(|(required, _)| required > 0) {
                return Some(arena.error());
            }

            // base is the declaration's own cached generic shape, still holding
            // bare GenericParameter placeholders. One binding per *declared*
            // parameter, in declaration order, keyed by the same TypeParameterId
            // push_decl_type_params uses, so positions cannot drift: a parameter
            // the body never mentions still owns its slot, and an argument that
            // cannot be resolved becomes the error type in place instead of
            // being dropped and shifting every later argument left. Each
            // explicit argument is resolved in the *caller's* namespace, exactly
            // like a generic call's explicit type arguments in calls.rs.
            // Arguments beyond the declared count are ignored; an omitted one
            // takes its declared default, or the error type if it has none.
            let mut bindings: Vec<(TypeParameterId, TypeId)> =
                Vec::with_capacity(decl.params.len());
            for (index, param) in decl.params.iter().enumerate() {
                let parameter_id = TypeParameterId::new(param.span().start, index as u32);
                let bound = match reference
                    .type_arguments
                    .as_ref()
                    .and_then(|arguments| arguments.params.get(index))
                {
                    Some(argument) => {
                        let resolved = resolve_ts_type(argument, namespace, arena)
                            .unwrap_or_else(|| arena.error());
                        check_type_argument_constraint(
                            namespace,
                            arena,
                            (parameter_id, param.name.name.as_str()),
                            resolved,
                            &bindings,
                            argument.span(),
                        );
                        resolved
                    }
                    None => namespace
                        .resolve_type_param_default(arena, decl, index)
                        .map(|default| {
                            crate::semantic::substitute_type_params(arena, default, &bindings)
                        })
                        .unwrap_or_else(|| arena.error()),
                };
                bindings.push((parameter_id, bound));
            }

            Some(crate::semantic::substitute_type_params(
                arena, base, &bindings,
            ))
        }

        _ => None,
    }
}

// Checks one explicit type argument against the `extends` bound of the parameter
// it was given to, the way tsc does (TS2344), and records a violation for
// bridge::check_program to report. The bound may mention earlier parameters
// (`<T, U extends T>`), so it is substituted with the arguments bound so far.
//
// Skipped when either side still contains a type parameter: this checker's
// subtyping does not look through a parameter's own bound, so comparing them here
// could report an error tsc would not. Such an argument is checked where the
// enclosing generic is instantiated instead. An argument that failed to resolve
// is the error type, which is compatible with everything.
fn check_type_argument_constraint(
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
    parameter: (TypeParameterId, &str),
    argument: TypeId,
    bindings: &[(TypeParameterId, TypeId)],
    span: oxc_span::Span,
) {
    let (parameter_id, parameter_name) = parameter;
    let Some(constraint) = namespace.type_param_constraint(arena, parameter_id) else {
        return;
    };
    let constraint = crate::semantic::substitute_type_params(arena, constraint, bindings);
    if crate::semantic::contains_type_param(arena, argument)
        || crate::semantic::contains_type_param(arena, constraint)
    {
        return;
    }
    if !crate::subtyping::is_subtype(arena, argument, constraint) {
        namespace.note_constraint_violation(parameter_name, span);
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
            name: binding_name(&param.pattern),
        });
    }

    if let Some(rest) = &params.rest {
        let annotation = rest.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        resolved.push(Param {
            type_id,
            optional: false,
            rest: true,
            name: binding_name(&rest.rest.argument),
        });
    }

    Some(resolved)
}

// None for a destructured pattern, which has no single name -- treated as
// unnamed rather than guessed.
fn binding_name(pattern: &BindingPattern) -> Option<std::rc::Rc<str>> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str().into()),
        BindingPattern::AssignmentPattern(assignment) => binding_name(&assignment.left),
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => None,
    }
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
                name: binding_name(&param.pattern),
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
            name: binding_name(&rest.rest.argument),
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
