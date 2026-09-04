//! Pass 2 ("check"): walks statement bodies now that pass 1 has hoisted
//! top-level signatures. Identifiers resolve by SymbolId through
//! symbol_map.rs rather than a hand-rolled scope chain.
//!
//! Function parameters are declared into the symbol map right here,
//! immediately before a function's body is walked. Pass 1 only builds the
//! function's signature (see bridge/declare.rs); it never touches the
//! individual parameter bindings, since those only matter once we're
//! inside the body checking references to them.

use oxc_ast::ast::{
    BindingPattern, ClassElement, MethodDefinitionKind, Program, PropertyKey, Statement, VariableDeclarationKind,
};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::namespace::Resolution;
use crate::type_annotation::{resolve_function_params, resolve_type_annotation};
use crate::types::Type;

use super::context::CheckContext;
use super::expressions::infer_expression_type;
use super::narrow::narrow_condition;

#[tracing::instrument(skip_all, fields(statement_count = program.body.len()))]
pub fn check_top_level(program: &Program, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) {
    for stmt in &program.body {
        check_statement(stmt, scoping, ctx);
    }
}

fn check_statement(stmt: &Statement, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) {
    match stmt {
        // A type alias or interface only ever contributes a type, and
        // that work happens once in declare.rs. There is nothing left to
        // walk here.
        Statement::TSTypeAliasDeclaration(_) | Statement::TSInterfaceDeclaration(_) => {}

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
            ctx.narrow = outer_narrow.clone();

            if let Some(alternate) = &if_stmt.alternate {
                ctx.narrow.extend(false_overrides);
                check_statement(alternate, scoping, ctx);
                ctx.narrow = outer_narrow;
            }
        }

        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    tracing::trace!("destructuring pattern not yet checked");
                    continue;
                };

                let annotation_type = declarator
                    .type_annotation
                    .as_ref()
                    .and_then(|a| resolve_type_annotation(a, &mut ctx.namespace, &mut ctx.arena));

                let inferred_type = declarator.init.as_ref().map(|init| infer_expression_type(init, scoping, ctx));

                match (annotation_type, inferred_type) {
                    (Some(declared), Some(actual)) => {
                        if !crate::subtyping::is_subtype(&ctx.arena, actual, declared) {
                            ctx.error(
                                format!(
                                    "Type mismatch: value is not assignable to declared type of '{}'.",
                                    id.name
                                ),
                                declarator.init.as_ref().map_or(declarator.span(), GetSpan::span),
                            );
                        }
                        // Register the declared type regardless of whether
                        // the check above passed. Without this, a later
                        // reference to `id` in the same body resolves fine
                        // via oxc_semantic but finds nothing in
                        // ctx.symbols, silently falls back to the error
                        // sentinel, and then passes every subtype check
                        // against it since Error is universally compatible
                        // in is_subtype. That turns a real type error into
                        // a false negative instead of a diagnostic.
                        if let Some(symbol_id) = id.symbol_id.get() {
                            ctx.symbols.declare(symbol_id, declared);
                        }
                    }
                    // No annotation: register the inferred type so later
                    // statements can resolve this binding. `const` keeps
                    // the precise literal type (`const x = 5` gives `x`
                    // type `5`); `let`/`var` widen to the base primitive,
                    // since the value can change later.
                    (None, Some(actual)) => {
                        let registered_type = if decl.kind == VariableDeclarationKind::Const {
                            actual
                        } else {
                            crate::types::widen(&ctx.arena, actual)
                        };
                        if let Some(symbol_id) = id.symbol_id.get() {
                            ctx.symbols.declare(symbol_id, registered_type);
                        }
                    }
                    // Annotated with no initializer (`let x: number;`).
                    // Same registration gap as above: nothing to check
                    // yet, but the binding still needs a type on record
                    // before any later reference to it is checked.
                    (Some(declared), None) => {
                        if let Some(symbol_id) = id.symbol_id.get() {
                            ctx.symbols.declare(symbol_id, declared);
                        }
                    }
                    (None, None) => {}
                }
            }
        }

        Statement::FunctionDeclaration(func) => {
            let Some(body) = &func.body else { return };
            let Some(name) = func.id.as_ref() else { return };
            let Some(symbol_id) = name.symbol_id.get() else { return };

            let Some(function_type) = ctx.symbols.get(symbol_id) else {
                tracing::trace!(name = %name.name, "function signature not fully annotated, body not checked");
                return;
            };
            let crate::types::Type::Function(function_type) = ctx.arena.get(function_type).clone() else { return };

            // Bind each parameter to its declared type before the body is
            // walked. oxc_semantic already resolved every reference to a
            // parameter inside the body to this same SymbolId; without
            // this step those references would resolve fine but carry no
            // type, silently degrading to the error sentinel.
            bind_params(&func.params.items, &function_type.params, ctx);

            let outer_return_type = ctx.current_return_type.replace(function_type.return_type);
            for body_stmt in &body.statements {
                check_statement(body_stmt, scoping, ctx);
            }
            ctx.current_return_type = outer_return_type;
        }

        Statement::ReturnStatement(ret) => {
            let actual = match &ret.argument {
                Some(expr) => infer_expression_type(expr, scoping, ctx),
                None => ctx.arena.undefined(),
            };
            // `current_return_type` is only unset if a `return` appears
            // outside any function body we're checking — malformed input
            // the parser would already have rejected, so nothing to flag.
            if let Some(expected) = ctx.current_return_type {
                if !crate::subtyping::is_subtype(&ctx.arena, actual, expected) {
                    ctx.error("Return type does not match the function's declared return type.", ret.span());
                }
            }
        }

        Statement::ExpressionStatement(expr_stmt) => {
            infer_expression_type(&expr_stmt.expression, scoping, ctx);
        }

        Statement::ClassDeclaration(class) => {
            let Some(name) = class.id.as_ref() else { return };
            let instance_type = match ctx.namespace.resolve(&name.name, &mut ctx.arena) {
                Resolution::Resolved(type_id) => type_id,
                _ => return,
            };
            let Type::Object(instance_object) = ctx.arena.get(instance_type).clone() else { return };

            let outer_class_instance = ctx.current_class_instance.replace(instance_type);

            // Constructor body, if there is one and its params are fully
            // annotated.
            if let Some(ctor) = super::declare::find_constructor(class) {
                if let Some(ctor_body) = &ctor.body {
                    if let Some(params) = resolve_function_params(ctor, &mut ctx.namespace, &mut ctx.arena) {
                        bind_params(&ctor.params.items, &params, ctx);
                        for body_stmt in &ctor_body.statements {
                            check_statement(body_stmt, scoping, ctx);
                        }
                    }
                }
            }

            for element in &class.body.body {
                let ClassElement::MethodDefinition(method) = element else { continue };
                if method.r#static || method.kind != MethodDefinitionKind::Method {
                    continue;
                }
                let PropertyKey::StaticIdentifier(key) = &method.key else { continue };
                let Some(method_body) = &method.value.body else { continue };

                // The method's type was already built once while resolving
                // the class's instance type; reuse it rather than
                // re-resolving its signature here.
                let Some(method_property) =
                    instance_object.properties.iter().find(|p| p.name == key.name.to_string())
                else {
                    continue;
                };
                let Type::Function(method_type) = ctx.arena.get(method_property.type_id).clone() else { continue };

                bind_params(&method.value.params.items, &method_type.params, ctx);
                let outer_return_type = ctx.current_return_type.replace(method_type.return_type);
                for body_stmt in &method_body.statements {
                    check_statement(body_stmt, scoping, ctx);
                }
                ctx.current_return_type = outer_return_type;
            }

            ctx.current_class_instance = outer_class_instance;
        }

        other => push_unsupported(other, ctx),
    }
}

fn bind_params(
    params: &[oxc_ast::ast::FormalParameter<'_>],
    param_types: &[crate::arena::TypeId],
    ctx: &mut CheckContext<'_, '_>,
) {
    for (param, &param_type) in params.iter().zip(param_types) {
        if let BindingPattern::BindingIdentifier(param_id) = &param.pattern {
            if let Some(param_symbol_id) = param_id.symbol_id.get() {
                ctx.symbols.declare(param_symbol_id, param_type);
            }
        }
    }
}

fn push_unsupported(stmt: &Statement, ctx: &mut CheckContext<'_, '_>) {
    let kind = stmt_kind_name(stmt);
    tracing::trace!(kind, "unsupported statement kind");
    ctx.warning(format!("This statement kind is not yet checked by ts-rust: {kind}."), stmt.span());
}

fn stmt_kind_name(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::ForStatement(_) => "ForStatement",
        Statement::WhileStatement(_) => "WhileStatement",
        Statement::ImportDeclaration(_) => "ImportDeclaration",
        _ => "Other",
    }
}
