use oxc_ast::ast::{ClassElement, MethodDefinitionKind, PropertyKey};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::namespace::Resolution;
use crate::type_annotation::{resolve_function_params, resolve_type_annotation};
use crate::types::Type;

use super::super::context::CheckContext;
use super::super::expressions::infer_expression_type;
use super::support::report_implicit_any_params;
use super::{bind_params, check_statement};

pub(super) fn check_class_declaration(
    class: &oxc_ast::ast::Class<'_>,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    // Reported up front, before anything below can bail out, so a class this
    // checker treats as wholly unsupported still gets its untyped parameters
    // flagged the way tsc would.
    for element in &class.body.body {
        if let ClassElement::MethodDefinition(method) = element {
            report_implicit_any_params(&method.value.params, ctx);
        }
    }

    let Some(name) = class.id.as_ref() else {
        return;
    };
    // The class's already-resolved instance shape, built once during the declare
    // pass (see resolve_class in namespace.rs). Instance methods below reuse the
    // signatures baked into this shape rather than re-resolving them, the same
    // pattern check_function_declaration uses for plain functions. Generic
    // classes (`class Box<T>`) are not supported, so there is no type-parameter
    // scope to push here the way check_function_declaration pushes one for `T`.
    let instance_type = match ctx.namespace.resolve(&name.name, &mut ctx.arena) {
        Resolution::Resolved(type_id) => type_id,
        Resolution::Circular | Resolution::NotFound => {
            ctx.warning(
                crate::diagnostic_messages::messages::unimplemented_class_shape(&name.name),
                class.span(),
            );
            return;
        }
    };
    let Type::Object(instance_object) = ctx.arena.get(instance_type).clone() else {
        return;
    };

    let outer_class_instance = ctx.current_class_instance.replace(instance_type);

    // The constructor is not part of the instance's own structural type (nothing
    // outside the class calls it as `instance.constructor(...)`), so unlike an
    // instance method below, its parameter types are resolved fresh here rather
    // than reused from instance_object.
    if let Some(ctor) = super::super::declare::find_constructor(class) {
        if let Some(ctor_body) = &ctor.body {
            if let Some(params) =
                resolve_function_params(&ctor.params, &mut ctx.namespace, &mut ctx.arena)
            {
                bind_params(&ctor.params, &params, scoping, ctx);
                for body_stmt in &ctor_body.statements {
                    check_statement(body_stmt, scoping, ctx);
                }
            }
        }
    }

    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method)
                if !method.r#static && method.kind == MethodDefinitionKind::Method =>
            {
                let PropertyKey::StaticIdentifier(key) = &method.key else {
                    continue;
                };
                let Some(method_body) = &method.value.body else {
                    continue;
                };

                // Reuses the method's signature from instance_object, computed
                // once during resolve_class, rather than re-resolving it here.
                let Some(method_property) = instance_object
                    .properties
                    .iter()
                    .find(|p| p.name.as_ref() == key.name.as_str())
                else {
                    continue;
                };
                let Type::Function(method_type) = ctx.arena.get(method_property.type_id).clone()
                else {
                    continue;
                };

                bind_params(&method.value.params, &method_type.params, scoping, ctx);
                let outer_return_type = ctx.current_return_type.replace(method_type.return_type);
                for body_stmt in &method_body.statements {
                    check_statement(body_stmt, scoping, ctx);
                }
                ctx.current_return_type = outer_return_type;
            }

            // A static method lives on the class itself, not on instances, so it
            // has no entry in instance_object the way an instance method does,
            // and its signature is resolved fresh here instead.
            ClassElement::MethodDefinition(method)
                if method.r#static && method.kind == MethodDefinitionKind::Method =>
            {
                let Some(method_body) = &method.value.body else {
                    continue;
                };

                let (params, _is_untyped) = match resolve_function_params(
                    &method.value.params,
                    &mut ctx.namespace,
                    &mut ctx.arena,
                ) {
                    Some(params) => (params, false),
                    None => (Vec::new(), true),
                };
                bind_params(&method.value.params, &params, scoping, ctx);

                let return_type =
                    method.value.return_type.as_ref().and_then(|rt| {
                        resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena)
                    });
                let outer_return_type =
                    std::mem::replace(&mut ctx.current_return_type, return_type);
                for body_stmt in &method_body.statements {
                    check_statement(body_stmt, scoping, ctx);
                }
                ctx.current_return_type = outer_return_type;
            }

            ClassElement::MethodDefinition(method) if method.r#static => {
                ctx.warning(
                    crate::diagnostic_messages::messages::unimplemented_static_accessor(),
                    method.span(),
                );
            }

            ClassElement::PropertyDefinition(prop) if prop.r#static => {
                let Some(initializer) = &prop.value else {
                    continue;
                };
                let actual = infer_expression_type(initializer, scoping, ctx);
                if let Some(annotation) = &prop.type_annotation {
                    if let Some(declared) =
                        resolve_type_annotation(annotation, &mut ctx.namespace, &mut ctx.arena)
                    {
                        if !ctx.semantic().is_assignable(actual, declared) {
                            ctx.error(
                                crate::diagnostic_messages::messages::static_field_initializer_mismatch(),
                                initializer.span(),
                            );
                        }
                    }
                }
            }

            _ => {}
        }
    }

    ctx.current_class_instance = outer_class_instance;
}
