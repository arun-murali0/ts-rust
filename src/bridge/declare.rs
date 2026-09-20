use oxc_ast::ast::{BinaryOperator, BindingPattern, Expression, Program, Statement, UnaryOperator};

use crate::type_annotation::{resolve_function_params, resolve_type_annotation};
use crate::types::{ObjectType, PropertyEntry, Type};

use super::context::CheckContext;

// Two passes over the same statement list. The first registers every named type
// (interfaces, aliases, classes, enums) before anything is resolved, so a
// function declared earlier in the file can reference a type declared later. The
// second pass resolves function and variable signatures, which can now look up
// any of those names regardless of source order.
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

                // A generic function's own type parameters need to be resolvable
                // by name while its parameter and return type annotations are
                // being resolved, then removed again immediately afterward so
                // they do not leak into any other declaration's resolution.
                let scope = ctx.namespace.push_type_params(&mut ctx.arena, func);

                let params =
                    resolve_function_params(&func.params, &mut ctx.namespace, &mut ctx.arena);
                let return_type = func
                    .return_type
                    .as_ref()
                    .and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));

                ctx.namespace.pop_type_params(scope);

                // Any parameter or the return type failing to resolve, an unknown
                // name in an annotation for example, means the whole function is
                // left undeclared rather than declared with a guessed type. A
                // later call to it will report an honest unresolved-name problem
                // instead of silently passing or failing arity checks it should
                // not be subject to.
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

    // A TypeScript enum is really two separate names sharing one identifier: used
    // in a type position, it means the union of its member literal types; used in
    // a value position, it means an object whose properties are its members. Both
    // are registered here from the same resolved member list.
    let member_types: Vec<_> = members.iter().map(|(_, type_id)| *type_id).collect();
    let type_position = ctx.arena.alloc_union(member_types);
    ctx.namespace.insert_resolved(&decl.id.name, type_position);

    let properties = members
        .into_iter()
        .map(|(name, type_id)| PropertyEntry {
            name: name.into(),
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
    use oxc_ast::ast::TSEnumMemberName;

    let mut members = Vec::with_capacity(decl.body.members.len());
    // Every numeric member's value so far, by name, so a later initializer can
    // refer to an earlier member (`ReadWrite = Read | Write`).
    let mut numeric_values: Vec<(String, f64)> = Vec::new();
    // Tracks the running value for TypeScript's own auto-increment rule: a
    // numeric member with no initializer takes the previous numeric member's
    // value plus one. Reset to None whenever a string member breaks that chain,
    // since auto-increment only continues across consecutive numeric members.
    let mut prev_numeric: Option<f64> = None;

    for member in &decl.body.members {
        let name = match &member.id {
            TSEnumMemberName::Identifier(id) => id.name.to_string(),
            TSEnumMemberName::String(s) => s.value.to_string(),

            TSEnumMemberName::ComputedString(_) | TSEnumMemberName::ComputedTemplateString(_) => {
                // A computed member name means the enum's shape cannot be known
                // without evaluating an expression, so the whole enum is left
                // unresolved rather than guessing.
                return None;
            }
        };

        let member_type = match &member.initializer {
            Some(Expression::StringLiteral(s)) => {
                prev_numeric = None;
                arena.alloc(Type::StringLiteral(s.value.to_string()))
            }

            // Any other initializer is only usable if it is a constant
            // expression this checker can fully evaluate, as tsc does for
            // `1 << 0` or `Read | Write`. One it cannot evaluate, such as a
            // function call, leaves the whole enum unresolved rather than
            // guessing a value.
            Some(initializer) => {
                let value = eval_enum_constant(initializer, &numeric_values)?;
                prev_numeric = Some(value);
                numeric_values.push((name.clone(), value));
                arena.alloc(Type::NumberLiteral(value))
            }
            None => {
                let next = match prev_numeric {
                    Some(n) => n + 1.0,
                    None if members.is_empty() => 0.0,

                    None => return None,
                };
                prev_numeric = Some(next);
                numeric_values.push((name.clone(), next));
                arena.alloc(Type::NumberLiteral(next))
            }
        };

        members.push((name, member_type));
    }

    Some(members)
}

// Evaluates the constant numeric expressions tsc accepts as an enum member
// initializer: numeric literals, parentheses, unary + - ~, the arithmetic and
// bitwise binary operators, and references to earlier members of the same enum.
// Anything else, or a result that is not a finite number, is None, which leaves
// the enum unresolved the same way an unsupported initializer always has.
fn eval_enum_constant(expr: &Expression, known: &[(String, f64)]) -> Option<f64> {
    let value = match expr {
        Expression::NumericLiteral(n) => n.value,
        Expression::ParenthesizedExpression(inner) => eval_enum_constant(&inner.expression, known)?,
        Expression::Identifier(id) => known.iter().find(|(name, _)| name == id.name.as_str())?.1,
        Expression::UnaryExpression(unary) => {
            let operand = eval_enum_constant(&unary.argument, known)?;
            match unary.operator {
                UnaryOperator::UnaryPlus => operand,
                UnaryOperator::UnaryNegation => -operand,
                UnaryOperator::BitwiseNot => f64::from(!to_int32(operand)),
                _ => return None,
            }
        }
        Expression::BinaryExpression(binary) => {
            let left = eval_enum_constant(&binary.left, known)?;
            let right = eval_enum_constant(&binary.right, known)?;
            match binary.operator {
                BinaryOperator::Addition => left + right,
                BinaryOperator::Subtraction => left - right,
                BinaryOperator::Multiplication => left * right,
                BinaryOperator::Division => left / right,
                BinaryOperator::Remainder => left % right,
                BinaryOperator::Exponential => left.powf(right),
                BinaryOperator::ShiftLeft => {
                    f64::from(to_int32(left).wrapping_shl(to_uint32(right) & 31))
                }
                BinaryOperator::ShiftRight => {
                    f64::from(to_int32(left).wrapping_shr(to_uint32(right) & 31))
                }
                BinaryOperator::ShiftRightZeroFill => {
                    f64::from(to_uint32(left).wrapping_shr(to_uint32(right) & 31))
                }
                BinaryOperator::BitwiseAnd => f64::from(to_int32(left) & to_int32(right)),
                BinaryOperator::BitwiseOR => f64::from(to_int32(left) | to_int32(right)),
                BinaryOperator::BitwiseXOR => f64::from(to_int32(left) ^ to_int32(right)),
                _ => return None,
            }
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

// JavaScript's ToUint32 and ToInt32: truncate, then wrap modulo 2^32. The bitwise
// operators and shifts are defined on these 32-bit views of a number, not on the
// f64 itself.
fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32
}

fn to_int32(n: f64) -> i32 {
    to_uint32(n) as i32
}
