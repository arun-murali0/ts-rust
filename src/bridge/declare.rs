//! Pass 1 ("declare"). Two passes over the top level, in this order:
//!
//! 1. Register every `type` alias and `interface` by name, unresolved.
//!    Nothing is built yet, this just makes the name findable, so a
//!    variable declared earlier in the file can reference a type declared
//!    later.
//! 2. Register the type of every fully-annotated function and variable,
//!    keyed by the `SymbolId` `oxc_semantic` already assigned it.
//!
//! A function with even one untyped parameter isn't registered at all;
//! its call sites go unchecked rather than being guessed at.

use oxc_ast::ast::{BindingPattern, Program, Statement};

use crate::type_annotation::{resolve_function_params, resolve_type_annotation};

use super::context::CheckContext;

pub fn declare_top_level<'ast>(program: &'ast Program<'ast>, ctx: &mut CheckContext<'ast, '_>) {
    for stmt in &program.body {
        match stmt {
            Statement::TSTypeAliasDeclaration(decl) => {
                ctx.namespace.insert_type_alias(&decl.id.name, &decl.type_annotation);
            }
            Statement::TSInterfaceDeclaration(decl) => {
                ctx.namespace.insert_interface(&decl.id.name, decl);
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    ctx.namespace.insert_class(&id.name, class);
                }
            }
            _ => {}
        }
    }

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    let BindingPattern::BindingIdentifier(id) = &declarator.id else { continue };
                    let Some(annotation) = &declarator.type_annotation else { continue };
                    let Some(symbol_id) = id.symbol_id.get() else { continue };

                    if let Some(type_id) = resolve_type_annotation(annotation, &mut ctx.namespace, &mut ctx.arena) {
                        ctx.symbols.declare(symbol_id, type_id);
                    }
                }
            }

            Statement::FunctionDeclaration(func) => {
                let Some(name) = func.id.as_ref() else { continue };
                let Some(symbol_id) = name.symbol_id.get() else { continue };

                let params = resolve_function_params(func, &mut ctx.namespace, &mut ctx.arena);
                let return_type =
                    func.return_type.as_ref().and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));

                let (Some(params), Some(return_type)) = (params, return_type) else { continue };

                let function_type = ctx.arena.alloc(crate::types::Type::Function(crate::types::FunctionType {
                    params,
                    return_type,
                    is_untyped: false,
                }));
                ctx.symbols.declare(symbol_id, function_type);
            }

            Statement::ClassDeclaration(class) => {
                let Some(name) = class.id.as_ref() else { continue };
                let Some(symbol_id) = name.symbol_id.get() else { continue };

                // Resolving the class's own name here builds (and caches)
                // its instance type — the flattened extends chain plus its
                // own fields and methods. See namespace.rs's Class
                // handling for how that flattening works.
                let instance_type = match ctx.namespace.resolve(&name.name, &mut ctx.arena) {
                    crate::namespace::Resolution::Resolved(type_id) => type_id,
                    _ => continue,
                };

                // A class with no constructor genuinely has zero
                // parameters; a class with a constructor that has an
                // untyped parameter does not, it's unresolved. Those used
                // to collapse to the same `params: vec![]` via
                // `.unwrap_or_default()`, which made `new Point(1, 2)`
                // falsely fail arity checking against a class whose
                // constructor was merely untyped, not zero-arg.
                // `is_untyped` on the resulting FunctionType keeps the two
                // cases distinguishable at the point a `new` expression
                // actually needs to trust this arity; see
                // bridge/expressions.rs's `infer_new_expression_type`.
                let (constructor_params, constructor_is_untyped) = match find_constructor(class) {
                    None => (Vec::new(), false),
                    Some(ctor) => match resolve_function_params(ctor, &mut ctx.namespace, &mut ctx.arena) {
                        Some(params) => (params, false),
                        None => (Vec::new(), true),
                    },
                };

                let constructor_type = ctx.arena.alloc(crate::types::Type::Function(crate::types::FunctionType {
                    params: constructor_params,
                    return_type: instance_type,
                    is_untyped: constructor_is_untyped,
                }));
                ctx.symbols.declare(symbol_id, constructor_type);
            }

            _ => {}
        }
    }
}

pub(super) fn find_constructor<'a>(class: &'a oxc_ast::ast::Class<'a>) -> Option<&'a oxc_ast::ast::Function<'a>> {
    use oxc_ast::ast::{ClassElement, MethodDefinitionKind};
    class.body.body.iter().find_map(|element| {
        let ClassElement::MethodDefinition(method) = element else { return None };
        (method.kind == MethodDefinitionKind::Constructor).then_some(&*method.value)
    })
}
