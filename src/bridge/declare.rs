use oxc_ast::ast::{BindingPattern, Program, Statement};

use crate::type_annotation::{resolve_function_params, resolve_type_annotation};
use crate::types::{ObjectType, PropertyEntry, Type};

use super::context::CheckContext;

pub fn declare_top_level<'ast>(program: &'ast Program<'ast>, ctx: &mut CheckContext<'ast, '_>) {
    for stmt in &program.body {
        match stmt {
            Statement::TSTypeAliasDeclaration(decl) => {
                ctx.namespace
                    .insert_type_alias(&decl.id.name, &decl.type_annotation);
            }
            Statement::TSInterfaceDeclaration(decl) => {
                ctx.namespace.insert_interface(&decl.id.name, decl);
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    ctx.namespace.insert_class(&id.name, class);
                }
            }
            Statement::TSEnumDeclaration(decl) => declare_enum(decl, ctx),
            _ => {}
        }
    }

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                        continue;
                    };
                    let Some(annotation) = &declarator.type_annotation else {
                        continue;
                    };
                    let Some(symbol_id) = id.symbol_id.get() else {
                        continue;
                    };

                    if let Some(type_id) =
                        resolve_type_annotation(annotation, &mut ctx.namespace, &mut ctx.arena)
                    {
                        ctx.symbols.declare(symbol_id, type_id);
                    }
                }
            }

            Statement::FunctionDeclaration(func) => {
                let Some(name) = func.id.as_ref() else {
                    continue;
                };
                let Some(symbol_id) = name.symbol_id.get() else {
                    continue;
                };

                let scope = ctx.namespace.push_type_params(&mut ctx.arena, func);

                let params =
                    resolve_function_params(&func.params, &mut ctx.namespace, &mut ctx.arena);
                let return_type = func
                    .return_type
                    .as_ref()
                    .and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));

                ctx.namespace.pop_type_params(scope);

                let (Some(params), Some(return_type)) = (params, return_type) else {
                    continue;
                };

                let function_type =
                    ctx.arena
                        .alloc(crate::types::Type::Function(crate::types::FunctionType {
                            params,
                            return_type,
                            is_untyped: false,
                        }));
                ctx.symbols.declare(symbol_id, function_type);
            }

            Statement::ClassDeclaration(class) => {
                let Some(name) = class.id.as_ref() else {
                    continue;
                };
                let Some(symbol_id) = name.symbol_id.get() else {
                    continue;
                };

                let instance_type = match ctx.namespace.resolve(&name.name, &mut ctx.arena) {
                    crate::namespace::Resolution::Resolved(type_id) => type_id,
                    _ => continue,
                };

                let (constructor_params, constructor_is_untyped) = match find_constructor(class) {
                    None => (Vec::new(), false),
                    Some(ctor) => match resolve_function_params(
                        &ctor.params,
                        &mut ctx.namespace,
                        &mut ctx.arena,
                    ) {
                        Some(params) => (params, false),
                        None => (Vec::new(), true),
                    },
                };

                let constructor_type =
                    ctx.arena
                        .alloc(crate::types::Type::Function(crate::types::FunctionType {
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

pub(super) fn find_constructor<'a>(
    class: &'a oxc_ast::ast::Class<'a>,
) -> Option<&'a oxc_ast::ast::Function<'a>> {
    use oxc_ast::ast::{ClassElement, MethodDefinitionKind};
    class.body.body.iter().find_map(|element| {
        let ClassElement::MethodDefinition(method) = element else {
            return None;
        };
        (method.kind == MethodDefinitionKind::Constructor).then_some(&*method.value)
    })
}

fn declare_enum(decl: &oxc_ast::ast::TSEnumDeclaration, ctx: &mut CheckContext<'_, '_>) {
    let Some(symbol_id) = decl.id.symbol_id.get() else {
        return;
    };
    let Some(members) = resolve_enum_members(decl, &mut ctx.arena) else {
        return;
    };

    let member_types: Vec<_> = members.iter().map(|(_, type_id)| *type_id).collect();
    let type_position = ctx.arena.alloc_union(member_types);
    ctx.namespace.insert_resolved(&decl.id.name, type_position);

    let properties = members
        .into_iter()
        .map(|(name, type_id)| PropertyEntry {
            name,
            type_id,
            optional: false,
        })
        .collect();
    let value_type = ctx.arena.alloc(Type::Object(ObjectType { properties }));
    ctx.symbols.declare(symbol_id, value_type);
}

fn resolve_enum_members(
    decl: &oxc_ast::ast::TSEnumDeclaration,
    arena: &mut crate::arena::TypeArena,
) -> Option<Vec<(String, crate::arena::TypeId)>> {
    use oxc_ast::ast::{Expression, TSEnumMemberName};

    let mut members = Vec::with_capacity(decl.body.members.len());
    let mut prev_numeric: Option<f64> = None;

    for member in &decl.body.members {
        let name = match &member.id {
            TSEnumMemberName::Identifier(id) => id.name.to_string(),
            TSEnumMemberName::String(s) => s.value.to_string(),

            TSEnumMemberName::ComputedString(_) | TSEnumMemberName::ComputedTemplateString(_) => {
                return None
            }
        };

        let member_type = match &member.initializer {
            Some(Expression::NumericLiteral(n)) => {
                prev_numeric = Some(n.value);
                arena.alloc(Type::NumberLiteral(n.value))
            }
            Some(Expression::StringLiteral(s)) => {
                prev_numeric = None;
                arena.alloc(Type::StringLiteral(s.value.to_string()))
            }

            Some(_) => return None,
            None => {
                let next = match prev_numeric {
                    Some(n) => n + 1.0,
                    None if members.is_empty() => 0.0,

                    None => return None,
                };
                prev_numeric = Some(next);
                arena.alloc(Type::NumberLiteral(next))
            }
        };

        members.push((name, member_type));
    }

    Some(members)
}
