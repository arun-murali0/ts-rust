use oxc_ast::ast::{
    BindingPattern, FormalParameters, PropertyKey, TSMethodSignature, TSMethodSignatureKind,
    TSSignature, TSType, TSTypeAnnotation, TSTypeName,
};
use oxc_span::GetSpan;

use crate::arena::{TypeArena, TypeId};
use crate::namespace::{Resolution, TypeNamespace};
use crate::types::{FunctionType, ObjectType, Param, PropertyEntry, Type, TypeParameterId};

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

            // Only the declaration's own parameters are bound here, so a parameter
            // a member declares for itself (`map<U>(...)`) is kept for inference
            // at the call site instead of becoming unknown.
            Some(crate::semantic::substitute_bound_type_params(
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
        namespace.note_constraint_violation(parameter_name, argument, constraint, span);
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
    for (param, optional) in params.items.iter().zip(optional_flags(params)) {
        let annotation = param.type_annotation.as_ref()?;
        let type_id = resolve_type_annotation(annotation, namespace, arena)?;
        resolved.push(Param {
            type_id,
            optional,
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

// Whether each parameter can be left out of a call. `x?: T` always can. A default
// value (`x: T = v`) can too, but only when no required parameter follows it: in
// `(a = 1, b: number)` a caller has to supply `a` to reach `b`, so tsc counts both
// as required. Walking from the end is what turns "a required parameter follows"
// into one running flag.
//
// A default lives in FormalParameter::initializer. oxc only wraps a pattern in
// BindingPattern::AssignmentPattern for a default nested inside a destructuring
// pattern, so looking at the pattern alone missed every ordinary default and left
// `(name: string, greeting: string = "hi")` demanding both arguments. Both forms
// are honored here.
fn optional_flags(params: &FormalParameters) -> Vec<bool> {
    let mut required_follows = false;
    let mut flags: Vec<bool> = params
        .items
        .iter()
        .rev()
        .map(|param| {
            let has_default = param.initializer.is_some()
                || matches!(param.pattern, BindingPattern::AssignmentPattern(_));
            let optional = param.optional || (has_default && !required_follows);
            if !optional {
                required_follows = true;
            }
            optional
        })
        .collect();
    flags.reverse();
    flags
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
        .zip(optional_flags(params))
        .map(|(param, optional)| {
            let type_id = param
                .type_annotation
                .as_ref()
                .and_then(|annotation| resolve_type_annotation(annotation, namespace, arena))
                .unwrap_or_else(|| arena.any());
            Param {
                type_id,
                optional,
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
    let mut properties: Vec<PropertyEntry> = Vec::with_capacity(members.len());

    for member in members {
        // As with class members in namespace.rs, one member this checker cannot
        // represent, such as a call signature or an index signature inside an
        // interface, makes the whole interface unresolvable rather than silently
        // dropping just that member.
        let entry = match member {
            TSSignature::TSPropertySignature(property) => {
                let PropertyKey::StaticIdentifier(key) = &property.key else {
                    return None;
                };
                let annotation = property.type_annotation.as_ref()?;
                let type_id = resolve_type_annotation(annotation, namespace, arena)?;
                PropertyEntry {
                    name: key.name.to_string().into(),
                    type_id,
                    optional: property.optional,
                    is_method: false,
                }
            }
            TSSignature::TSMethodSignature(method) => {
                resolve_method_signature(method, namespace, arena)?
            }
            _ => return None,
        };

        // ObjectType's merge-join over sorted property lists gives wrong answers
        // when a name appears twice, and two method signatures with one name are an
        // overload set, which has no representation here. Either way the
        // declaration is not modelled, so it stays unresolvable.
        if properties
            .iter()
            .any(|existing| existing.name == entry.name)
        {
            return None;
        }
        properties.push(entry);
    }

    Some(arena.alloc(Type::Object(ObjectType::new(properties))))
}

// A method signature (`get(name: string): T`, `map<U>(f: (x: T) => U): U[]`) is
// a property whose type is a function, flagged as declared with method syntax so
// its parameters are compared bivariantly (see subtyping::property_is_subtype).
//
// Left unresolvable, exactly like a class method, when any part cannot be
// modelled: an accessor (`get x(): T`), a computed or non-identifier key, an
// explicit `this` parameter, or no return type annotation. Parameters without an
// annotation are not one of those: they become `any` and are reported as implicit
// any, the way a function type's parameters are.
//
// The method's own type parameters are in scope only while its signature is
// resolved, so `U` never leaks into the enclosing interface.
fn resolve_method_signature(
    method: &TSMethodSignature,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<PropertyEntry> {
    if !matches!(method.kind, TSMethodSignatureKind::Method)
        || method.computed
        || method.this_param.is_some()
    {
        return None;
    }
    let PropertyKey::StaticIdentifier(key) = &method.key else {
        return None;
    };
    let return_annotation = method.return_type.as_ref()?;

    let scope = namespace.push_decl_type_params(arena, method.type_parameters.as_deref());
    namespace.note_implicit_any_params(&method.params);
    let params = resolve_params_with_any_fallback(&method.params, namespace, arena);
    let return_type = resolve_type_annotation(return_annotation, namespace, arena);
    namespace.pop_type_params(scope);

    let function = arena.alloc(Type::Function(FunctionType {
        params,
        return_type: return_type?,
        is_untyped: false,
    }));
    Some(PropertyEntry {
        name: key.name.to_string().into(),
        type_id: function,
        optional: method.optional,
        is_method: true,
    })
}
