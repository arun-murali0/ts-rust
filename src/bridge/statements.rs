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

/// Distinguishes "no type annotation was written" from "one was written
/// but couldn't be resolved," which used to collapse into the same
/// `None` and get treated identically. See the `VariableDeclaration` arm
/// below for what that silently broke.
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

                // Three distinct outcomes, not two: no annotation was
                // written at all, an annotation was written and resolved,
                // or an annotation was written but couldn't be resolved
                // (an unknown name, or a name whose own definition isn't
                // fully understood, e.g. a class with an unresolvable
                // member). Collapsing the last two into one `None` used
                // to mean an unresolvable annotation was silently treated
                // exactly like no annotation at all: the variable's
                // inferred type got registered with zero diagnostic
                // anywhere, and `let bad: TypoedName = value;` produced
                // no signal that `TypoedName` was never found.
                let annotation_outcome = match &declarator.type_annotation {
                    None => AnnotationOutcome::Absent,
                    Some(annotation) => {
                        match resolve_type_annotation(annotation, &mut ctx.namespace, &mut ctx.arena) {
                            Some(type_id) => AnnotationOutcome::Resolved(type_id),
                            None => AnnotationOutcome::Unresolvable,
                        }
                    }
                };

                let inferred_type = declarator.init.as_ref().map(|init| infer_expression_type(init, scoping, ctx));

                match (annotation_outcome, inferred_type) {
                    // The annotation itself couldn't be resolved. The
                    // initializer expression was still walked above (so a
                    // real error inside it, e.g. `1 + "x"`, is still
                    // caught), but there's nothing sound to compare it
                    // against, and guessing a type here would be worse
                    // than an honest gap. `id` is deliberately left
                    // unregistered: a later reference to it hits the
                    // existing "resolved symbol has no registered type"
                    // warning in expressions.rs rather than silently
                    // succeeding. This can double up with a warning
                    // already raised at the failing type's own
                    // declaration site (e.g. a class that failed to
                    // resolve); that overlap is accepted, since silence
                    // is worse than one redundant diagnostic.
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
                    (AnnotationOutcome::Absent, Some(actual)) => {
                        let registered_type = if decl.kind == VariableDeclarationKind::Const {
                            actual
                        } else {
                            crate::types::widen(&ctx.arena, actual)
                        };
                        if let Some(symbol_id) = id.symbol_id.get() {
                            ctx.symbols.declare(symbol_id, registered_type);
                        }
                    }
                    // Annotated (and resolved) with no initializer
                    // (`let x: number;`). Same registration gap as above:
                    // nothing to check yet, but the binding still needs a
                    // type on record before any later reference to it is
                    // checked.
                    (AnnotationOutcome::Resolved(declared), None) => {
                        if let Some(symbol_id) = id.symbol_id.get() {
                            ctx.symbols.declare(symbol_id, declared);
                        }
                    }
                    (AnnotationOutcome::Absent, None) => {}
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
                Resolution::Circular | Resolution::NotFound => {
                    // Every other unsupported construct gets a visible
                    // warning at its own site via push_unsupported; a
                    // class that failed to resolve used to be the one
                    // exception, silently skipped here with nothing
                    // telling the reader why. This is also the one place
                    // that reports it: `resolve()`'s result isn't cached
                    // on failure, so this same failure would otherwise
                    // repeat silently at every place the class is
                    // referenced too.
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
            let Type::Object(instance_object) = ctx.arena.get(instance_type).clone() else { return };

            let outer_class_instance = ctx.current_class_instance.replace(instance_type);

            // Constructor body, if there is one and its params are fully
            // annotated.
            if let Some(ctor) = super::declare::find_constructor(class) {
                if let Some(ctor_body) = &ctor.body {
                    if let Some(params) = resolve_function_params(&ctor.params, &mut ctx.namespace, &mut ctx.arena) {
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

pub(super) fn bind_params(
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
