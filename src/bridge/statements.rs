use oxc_ast::ast::{
    ArrayPattern, BindingPattern, ClassElement, MethodDefinitionKind, ObjectPattern, Program,
    PropertyKey, Statement, VariableDeclarationKind, VariableDeclarator,
};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;
use crate::namespace::Resolution;
use crate::type_annotation::{resolve_function_params, resolve_type_annotation};
use crate::types::{Param, Type};

use super::context::CheckContext;
use super::expressions::{infer_expression_type, infer_member_access_type};
use super::narrow::{narrow_condition, narrow_to_non_nullish};

enum AnnotationOutcome {
    Absent,
    Resolved(crate::arena::TypeId),
    Unresolvable,
}

#[tracing::instrument(skip_all, fields(statement_count = program.body.len()))]
pub fn check_top_level(program: &Program, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) {
    for stmt in &program.body {
        check_statement(stmt, scoping, ctx);
    }
}

pub(super) fn check_statement(stmt: &Statement, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) {
    match stmt {
        Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_) => {}

        Statement::BlockStatement(block) => {
            for inner in &block.body {
                check_statement(inner, scoping, ctx);
            }
        }

        Statement::IfStatement(if_stmt) => {
            infer_expression_type(&if_stmt.test, scoping, ctx);

            let (true_overrides, false_overrides) = narrow_condition(&if_stmt.test, scoping, ctx);
            let outer_narrow = ctx.narrow.clone();

            ctx.narrow.extend(true_overrides);
            check_statement(&if_stmt.consequent, scoping, ctx);
            let consequent_always_exits = statement_always_exits(&if_stmt.consequent);

            match &if_stmt.alternate {
                Some(alternate) => {
                    ctx.narrow = outer_narrow.clone();
                    ctx.narrow.extend(false_overrides);
                    check_statement(alternate, scoping, ctx);
                    ctx.narrow = outer_narrow;
                }
                None => {
                    ctx.narrow = outer_narrow;

                    if consequent_always_exits {
                        ctx.narrow.extend(false_overrides);
                    }
                }
            }
        }

        Statement::VariableDeclaration(decl) => check_variable_declaration(decl, scoping, ctx),

        Statement::FunctionDeclaration(func) => {
            let Some(body) = &func.body else { return };
            let Some(name) = func.id.as_ref() else { return };
            let Some(symbol_id) = name.symbol_id.get() else {
                return;
            };

            let Some(function_type) = ctx.symbols.get(symbol_id) else {
                tracing::trace!(name = %name.name, "function signature not fully annotated, body not checked");
                return;
            };
            let crate::types::Type::Function(function_type) = ctx.arena.get(function_type).clone()
            else {
                return;
            };

            bind_params(&func.params, &function_type.params, scoping, ctx);

            let type_param_scope = ctx.namespace.push_type_params(&mut ctx.arena, func);

            let outer_return_type = ctx.current_return_type.replace(function_type.return_type);
            for body_stmt in &body.statements {
                check_statement(body_stmt, scoping, ctx);
            }
            ctx.current_return_type = outer_return_type;

            ctx.namespace.pop_type_params(type_param_scope);
        }

        Statement::ReturnStatement(ret) => {
            let actual = match &ret.argument {
                Some(expr) => infer_expression_type(expr, scoping, ctx),
                None => ctx.arena.undefined(),
            };

            if let Some(expected) = ctx.current_return_type {
                if !crate::subtyping::is_subtype(&ctx.arena, actual, expected) {
                    ctx.error(
                        "Return type does not match the function's declared return type.",
                        ret.span(),
                    );
                }
            }
        }

        Statement::ExpressionStatement(expr_stmt) => {
            infer_expression_type(&expr_stmt.expression, scoping, ctx);
        }

        Statement::ClassDeclaration(class) => {
            let Some(name) = class.id.as_ref() else {
                return;
            };
            let instance_type = match ctx.namespace.resolve(&name.name, &mut ctx.arena) {
                Resolution::Resolved(type_id) => type_id,
                Resolution::Circular | Resolution::NotFound => {
                    ctx.warning(
                        format!(
                            "Class '{}' uses a shape not yet checked by ts-rust: an unresolvable \
                             field or method, or a superclass that isn't a plain class name.",
                            name.name
                        ),
                        class.span(),
                    );
                    return;
                }
            };
            let Type::Object(instance_object) = ctx.arena.get(instance_type).clone() else {
                return;
            };

            let outer_class_instance = ctx.current_class_instance.replace(instance_type);

            if let Some(ctor) = super::declare::find_constructor(class) {
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

                        let Some(method_property) = instance_object
                            .properties
                            .iter()
                            .find(|p| p.name == key.name.to_string())
                        else {
                            continue;
                        };
                        let Type::Function(method_type) =
                            ctx.arena.get(method_property.type_id).clone()
                        else {
                            continue;
                        };

                        bind_params(&method.value.params, &method_type.params, scoping, ctx);
                        let outer_return_type =
                            ctx.current_return_type.replace(method_type.return_type);
                        for body_stmt in &method_body.statements {
                            check_statement(body_stmt, scoping, ctx);
                        }
                        ctx.current_return_type = outer_return_type;
                    }

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

                        let return_type = method.value.return_type.as_ref().and_then(|rt| {
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
                            "Static getter/setter is not yet checked by ts-rust.",
                            method.span(),
                        );
                    }

                    ClassElement::PropertyDefinition(prop) if prop.r#static => {
                        let Some(initializer) = &prop.value else {
                            continue;
                        };
                        let actual = infer_expression_type(initializer, scoping, ctx);
                        if let Some(annotation) = &prop.type_annotation {
                            if let Some(declared) = resolve_type_annotation(
                                annotation,
                                &mut ctx.namespace,
                                &mut ctx.arena,
                            ) {
                                if !crate::subtyping::is_subtype(&ctx.arena, actual, declared) {
                                    ctx.error("Static field initializer is not assignable to its declared type.", initializer.span());
                                }
                            }
                        }
                    }

                    _ => {}
                }
            }

            ctx.current_class_instance = outer_class_instance;
        }

        Statement::WhileStatement(while_stmt) => {
            infer_expression_type(&while_stmt.test, scoping, ctx);
            check_statement(&while_stmt.body, scoping, ctx);
        }

        Statement::ForStatement(for_stmt) => {
            if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &for_stmt.init
            {
                check_variable_declaration(decl, scoping, ctx);
            }

            if let Some(test) = &for_stmt.test {
                infer_expression_type(test, scoping, ctx);
            }
            if let Some(update) = &for_stmt.update {
                infer_expression_type(update, scoping, ctx);
            }
            check_statement(&for_stmt.body, scoping, ctx);
        }

        Statement::SwitchStatement(switch_stmt) => {
            infer_expression_type(&switch_stmt.discriminant, scoping, ctx);
            for case in &switch_stmt.cases {
                if let Some(test) = &case.test {
                    infer_expression_type(test, scoping, ctx);
                }
                for stmt in &case.consequent {
                    check_statement(stmt, scoping, ctx);
                }
            }
        }

        other => push_unsupported(other, ctx),
    }
}

fn check_variable_declaration(
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

    let inferred_type = declarator
        .init
        .as_ref()
        .map(|init| infer_expression_type(init, scoping, ctx));

    match (annotation_outcome, inferred_type) {
        (AnnotationOutcome::Unresolvable, _) => {
            ctx.warning(
                format!(
                    "Type annotation for '{}' could not be resolved (unknown name, \
                     or its definition isn't fully understood by ts-rust yet).",
                    id.name
                ),
                declarator.span(),
            );
        }

        (AnnotationOutcome::Resolved(declared), Some(actual)) => {
            if !crate::subtyping::is_subtype(&ctx.arena, actual, declared) {
                ctx.error(
                    format!(
                        "Type mismatch: value is not assignable to declared type of '{}'.",
                        id.name
                    ),
                    declarator
                        .init
                        .as_ref()
                        .map_or(declarator.span(), GetSpan::span),
                );
            }

            if let Some(symbol_id) = id.symbol_id.get() {
                ctx.symbols.declare(symbol_id, declared);
            }
        }

        (AnnotationOutcome::Absent, Some(actual)) => {
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
            ctx.warning(
                "Type annotation for this destructuring pattern could not be resolved.",
                declarator.span(),
            );
            return;
        }
        (AnnotationOutcome::Resolved(declared), Some(actual)) => {
            if !crate::subtyping::is_subtype(&ctx.arena, actual, declared) {
                ctx.error(
                    "Type mismatch: value is not assignable to the destructuring pattern's declared type.",
                    declarator.init.as_ref().map_or(declarator.span(), GetSpan::span),
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

fn bind_pattern(
    pattern: &BindingPattern,
    type_id: TypeId,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            if let Some(symbol_id) = id.symbol_id.get() {
                ctx.symbols.declare(symbol_id, type_id);
            }
        }
        BindingPattern::ObjectPattern(object) => bind_object_pattern(object, type_id, scoping, ctx),
        BindingPattern::ArrayPattern(array) => bind_array_pattern(array, type_id, scoping, ctx),
        BindingPattern::AssignmentPattern(assignment) => {
            let default_type = infer_expression_type(&assignment.right, scoping, ctx);
            let default_type = crate::types::widen(&ctx.arena, default_type);

            let non_nullish = narrow_to_non_nullish(&mut ctx.arena, type_id);
            let effective = ctx.arena.alloc_union(vec![non_nullish, default_type]);
            bind_pattern(&assignment.left, effective, scoping, ctx);
        }
    }
}

fn bind_object_pattern(
    object: &ObjectPattern,
    type_id: TypeId,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    for property in &object.properties {
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            ctx.warning(
                "Computed or non-identifier destructuring keys are not yet checked by ts-rust.",
                property.span(),
            );
            continue;
        };
        let property_type = infer_member_access_type(type_id, &key.name, property.span(), ctx);
        bind_pattern(&property.value, property_type, scoping, ctx);
    }

    if let Some(rest) = &object.rest {
        ctx.warning(
            "Rest destructuring (`...rest`) does not yet compute a precise type; \
             the binding is not checked by ts-rust.",
            rest.span(),
        );
        bind_pattern(&rest.argument, ctx.arena.error(), scoping, ctx);
    }
}

fn bind_array_pattern(
    array: &ArrayPattern,
    type_id: TypeId,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    let element_type = match ctx.arena.get(type_id) {
        Type::Array(element) => *element,
        Type::Any | Type::Error => type_id,
        _ => {
            ctx.error("Array destructuring requires an array type.", array.span());
            ctx.arena.error()
        }
    };

    for element_pattern in array.elements.iter().flatten() {
        bind_pattern(element_pattern, element_type, scoping, ctx);
    }

    if let Some(rest) = &array.rest {
        let rest_type = ctx.arena.alloc(Type::Array(element_type));
        bind_pattern(&rest.argument, rest_type, scoping, ctx);
    }
}

pub(super) fn bind_params(
    params: &oxc_ast::ast::FormalParameters<'_>,
    param_types: &[Param],
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    for (param, declared) in params.items.iter().zip(param_types) {
        bind_pattern(&param.pattern, declared.type_id, scoping, ctx);
    }

    if let Some(rest) = &params.rest {
        if let Some(rest_param) = param_types.last().filter(|p| p.rest) {
            bind_pattern(&rest.rest.argument, rest_param.type_id, scoping, ctx);
        }
    }
}

fn push_unsupported(stmt: &Statement, ctx: &mut CheckContext<'_, '_>) {
    let kind = stmt_kind_name(stmt);
    tracing::trace!(kind, "unsupported statement kind");
    ctx.warning(
        format!("This statement kind is not yet checked by ts-rust: {kind}."),
        stmt.span(),
    );
}

fn stmt_kind_name(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::ImportDeclaration(_) => "ImportDeclaration",
        _ => "Other",
    }
}

fn statement_always_exits(stmt: &Statement) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.last().is_some_and(statement_always_exits),
        Statement::IfStatement(if_stmt) => match &if_stmt.alternate {
            Some(alternate) => {
                statement_always_exits(&if_stmt.consequent) && statement_always_exits(alternate)
            }
            None => false,
        },
        _ => false,
    }
}
