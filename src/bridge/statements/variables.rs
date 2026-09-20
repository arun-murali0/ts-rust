use oxc_ast::ast::{BindingPattern, Expression, VariableDeclarationKind, VariableDeclarator};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;
use crate::type_annotation::resolve_type_annotation;

use super::super::context::CheckContext;
use super::super::expressions::{check_excess_properties, infer_expression_type};
use super::bind_pattern;
use super::support::{find_unresolved_type_name, report_implicit_any_params};

/// What we know about a declarator's declared type after attempting to
/// resolve its type annotation (if any). Kept separate from the inferred
/// type of its initializer so the two can be compared and merged explicitly
/// per declarator kind, rather than collapsing "no annotation" and
/// "annotation present but unresolvable" into the same case.
enum AnnotationOutcome {
    /// No type annotation was written on this declarator.
    Absent,
    /// A type annotation was written and resolved successfully.
    Resolved(TypeId),
    /// A type annotation was written but could not be resolved (unknown
    /// name, or a definition ts-rust doesn't fully understand yet).
    Unresolvable,
}

pub(super) fn check_variable_declaration(
    decl: &oxc_ast::ast::VariableDeclaration,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    for declarator in &decl.declarations {
        match &declarator.id {
            BindingPattern::BindingIdentifier(id) => {
                check_identifier_declarator(decl.kind, declarator, id, scoping, ctx);
            }
            BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => {
                check_destructured_declarator(decl.kind, declarator, scoping, ctx);
            }

            BindingPattern::AssignmentPattern(_) => {}
        }
    }
}

fn check_identifier_declarator(
    decl_kind: VariableDeclarationKind,
    declarator: &VariableDeclarator,
    id: &oxc_ast::ast::BindingIdentifier,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    let annotation_outcome = match &declarator.type_annotation {
        None => AnnotationOutcome::Absent,
        Some(annotation) => {
            match resolve_type_annotation(annotation, &mut ctx.namespace, &mut ctx.arena) {
                Some(type_id) => AnnotationOutcome::Resolved(type_id),
                None => AnnotationOutcome::Unresolvable,
            }
        }
    };

    // With no annotation on the variable there is no contextual type for the
    // function on the right, so any untyped parameter of it is a genuine
    // implicit any. Reported before the body is inferred so the diagnostics come
    // out in source order.
    if declarator.type_annotation.is_none() {
        match &declarator.init {
            Some(Expression::ArrowFunctionExpression(arrow)) => {
                report_implicit_any_params(&arrow.params, ctx);
            }
            Some(Expression::FunctionExpression(func)) => {
                report_implicit_any_params(&func.params, ctx);
            }
            _ => {}
        }
    }

    let inferred_type = declarator
        .init
        .as_ref()
        .map(|init| infer_expression_type(init, scoping, ctx));

    match (annotation_outcome, inferred_type) {
        (AnnotationOutcome::Unresolvable, _) => {
            let unknown_name = declarator.type_annotation.as_ref().and_then(|annotation| {
                find_unresolved_type_name(&annotation.type_annotation, scoping, &ctx.namespace)
            });
            match unknown_name {
                Some((name, span)) => ctx.error(
                    crate::diagnostic_messages::messages::unresolved_identifier(&name),
                    span,
                ),
                None => ctx.warning(
                    crate::diagnostic_messages::messages::unresolvable_type_annotation(&id.name),
                    declarator.span(),
                ),
            }
        }

        (AnnotationOutcome::Resolved(declared), Some(actual)) => {
            if !ctx.semantic().is_assignable(actual, declared) {
                ctx.error(
                    crate::diagnostic_messages::messages::declared_type_mismatch(&id.name),
                    declarator
                        .init
                        .as_ref()
                        .map_or(declarator.span(), GetSpan::span),
                );
            } else if let Some(init) = &declarator.init {
                check_excess_properties(init, declared, ctx);
            }

            if let Some(symbol_id) = id.symbol_id.get() {
                ctx.symbols.declare(symbol_id, declared);
            }
        }

        (AnnotationOutcome::Absent, Some(actual)) => {
            // const x = "a" keeps the literal type "a", since a const binding can
            // never be reassigned to a different string. let x = "a" widens to
            // string, since a later `x = "b"` would otherwise be rejected by a
            // type it was never meant to be pinned to.
            let registered_type = if decl_kind == VariableDeclarationKind::Const {
                actual
            } else {
                crate::types::widen(&ctx.arena, actual)
            };
            if let Some(symbol_id) = id.symbol_id.get() {
                ctx.symbols.declare(symbol_id, registered_type);
            }
        }

        (AnnotationOutcome::Resolved(declared), None) => {
            if let Some(symbol_id) = id.symbol_id.get() {
                ctx.symbols.declare(symbol_id, declared);
            }
        }
        (AnnotationOutcome::Absent, None) => {}
    }
}

// Handles const {x} = y and let [a, b] = y separately from the plain identifier
// case above, since a destructured declarator has no single name to report a
// mismatch against; each nested property or element is checked individually by
// bind_pattern once the overall source_type is known. This function's only job
// is resolving that one source_type.
fn check_destructured_declarator(
    decl_kind: VariableDeclarationKind,
    declarator: &VariableDeclarator,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    let annotation_outcome = match &declarator.type_annotation {
        None => AnnotationOutcome::Absent,
        Some(annotation) => {
            match resolve_type_annotation(annotation, &mut ctx.namespace, &mut ctx.arena) {
                Some(type_id) => AnnotationOutcome::Resolved(type_id),
                None => AnnotationOutcome::Unresolvable,
            }
        }
    };

    let inferred_type = declarator
        .init
        .as_ref()
        .map(|init| infer_expression_type(init, scoping, ctx));

    let source_type = match (annotation_outcome, inferred_type) {
        (AnnotationOutcome::Unresolvable, _) => {
            let unknown_name = declarator.type_annotation.as_ref().and_then(|annotation| {
                find_unresolved_type_name(&annotation.type_annotation, scoping, &ctx.namespace)
            });
            match unknown_name {
                Some((name, span)) => ctx.error(
                    crate::diagnostic_messages::messages::unresolved_identifier(&name),
                    span,
                ),
                None => ctx.warning(
                    crate::diagnostic_messages::messages::unresolvable_destructuring_type_annotation(),
                    declarator.span(),
                ),
            }
            return;
        }
        (AnnotationOutcome::Resolved(declared), Some(actual)) => {
            if !ctx.semantic().is_assignable(actual, declared) {
                ctx.error(
                    crate::diagnostic_messages::messages::destructuring_pattern_type_mismatch(),
                    declarator
                        .init
                        .as_ref()
                        .map_or(declarator.span(), GetSpan::span),
                );
            }
            declared
        }
        (AnnotationOutcome::Resolved(declared), None) => declared,
        (AnnotationOutcome::Absent, Some(actual)) => {
            if decl_kind == VariableDeclarationKind::Const {
                actual
            } else {
                crate::types::widen(&ctx.arena, actual)
            }
        }
        (AnnotationOutcome::Absent, None) => return,
    };

    bind_pattern(&declarator.id, source_type, scoping, ctx);
}
