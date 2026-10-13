use oxc_ast::ast::{
    BinaryOperator, BindingPattern, Class, Declaration, ExportDefaultDeclarationKind, Expression,
    Function, Program, Statement, TSEnumDeclaration, TSInterfaceDeclaration, TSModuleBlock,
    TSNamespaceDeclaration, TSNamespaceDeclarationBody, TSTypeAliasDeclaration, UnaryOperator,
    VariableDeclaration,
};

use crate::type_annotation::{resolve_function_params, resolve_type_annotation};
use crate::types::Type;

use super::context::CheckContext;

// One declaration the declare pass cares about, whether it stands alone or sits
// behind `export` / `export default`.
//
// Problem: both passes matched on Statement::FunctionDeclaration and friends, and an
// exported declaration is a different statement kind that wraps the same payload, so
// `export function f(...)` and `export interface I {...}` were never declared at all
// (and the check pass then warned they were unsupported).
// Picked: look through the export wrapper once, here, and hand both passes the same
// flat list, so a declaration is treated the same whether or not it is exported.
// Cost: a list of references is built per pass; `export { a, b }` and
// `export * from` declare nothing and are skipped, which is correct for one file.
#[derive(Clone, Copy)]
pub(super) enum TopLevelItem<'a> {
    Alias(&'a TSTypeAliasDeclaration<'a>),
    Interface(&'a TSInterfaceDeclaration<'a>),
    Class(&'a Class<'a>),
    Enum(&'a TSEnumDeclaration<'a>),
    Variable(&'a VariableDeclaration<'a>),
    Function(&'a Function<'a>),
    Namespace(&'a TSNamespaceDeclaration<'a>),
    // `declare module "x" { ... }`: its members are declared in a scope named for `x`.
    ExternalModule(&'a oxc_ast::ast::TSExternalModuleDeclaration<'a>),
}

// The scope name of `declare module "x"`: quoted, so no identifier can collide with it.
pub(super) fn module_scope(specifier: &str) -> String {
    format!("\"{specifier}\"")
}

// The block of a `namespace N { ... }` this checker reads: a plain name with a block body.
// `namespace A.B { ... }` (a dotted name) is not one, and neither is `declare module "x"`
// or `declare global`, so those stay what they were: unchecked and silent.
pub(super) fn namespace_block<'a>(
    ns: &'a TSNamespaceDeclaration<'a>,
) -> Option<&'a TSModuleBlock<'a>> {
    match &ns.body {
        TSNamespaceDeclarationBody::TSModuleBlock(block) => Some(block),
        TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => None,
    }
}

// Every declaration in a statement list, each with whether it is exported. A namespace's
// members are read through the same list, so it means the same inside and outside one.
pub(super) fn items_in<'a>(statements: &'a [Statement<'a>]) -> Vec<(TopLevelItem<'a>, bool)> {
    let mut items = Vec::new();
    for stmt in statements {
        match stmt {
            Statement::TSTypeAliasDeclaration(d) => items.push((TopLevelItem::Alias(d), false)),
            Statement::TSInterfaceDeclaration(d) => items.push((TopLevelItem::Interface(d), false)),
            Statement::ClassDeclaration(d) => items.push((TopLevelItem::Class(d), false)),
            Statement::TSEnumDeclaration(d) => items.push((TopLevelItem::Enum(d), false)),
            Statement::VariableDeclaration(d) => items.push((TopLevelItem::Variable(d), false)),
            Statement::FunctionDeclaration(d) => items.push((TopLevelItem::Function(d), false)),
            Statement::TSNamespaceDeclaration(d) => items.push((TopLevelItem::Namespace(d), false)),
            Statement::TSExternalModuleDeclaration(d) => {
                items.push((TopLevelItem::ExternalModule(d), false))
            }
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::TSTypeAliasDeclaration(d) => {
                    items.push((TopLevelItem::Alias(d), true))
                }
                Declaration::TSInterfaceDeclaration(d) => {
                    items.push((TopLevelItem::Interface(d), true))
                }
                Declaration::ClassDeclaration(d) => items.push((TopLevelItem::Class(d), true)),
                Declaration::TSEnumDeclaration(d) => items.push((TopLevelItem::Enum(d), true)),
                Declaration::VariableDeclaration(d) => {
                    items.push((TopLevelItem::Variable(d), true))
                }
                Declaration::FunctionDeclaration(d) => {
                    items.push((TopLevelItem::Function(d), true))
                }
                Declaration::TSNamespaceDeclaration(d) => {
                    items.push((TopLevelItem::Namespace(d), true))
                }
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(d) => {
                    items.push((TopLevelItem::Function(d), true))
                }
                ExportDefaultDeclarationKind::ClassDeclaration(d) => {
                    items.push((TopLevelItem::Class(d), true))
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(d) => {
                    items.push((TopLevelItem::Interface(d), true))
                }
                _ => {}
            },
            _ => {}
        }
    }
    items
}

// Two passes over the same statement list. The first registers every named type
// (interfaces, aliases, classes, enums) before anything is resolved, so a
// function declared earlier in the file can reference a type declared later. The
// second pass resolves function and variable signatures, which can look up
// any of those names regardless of source order.
pub fn declare_top_level<'ast>(program: &'ast Program<'ast>, ctx: &mut CheckContext<'ast, '_>) {
    let items = items_in(&program.body);
    declare_types(&items, ctx);
    for (name, span, involves_enum) in ctx.namespace.take_declaration_collisions() {
        let message = if involves_enum {
            crate::diagnostic_messages::messages::enum_declaration_merge()
        } else {
            crate::diagnostic_messages::messages::duplicate_type_declaration(&name)
        };
        ctx.error(message, span);
    }
    declare_values(&items, ctx, program.source_type.is_typescript_definition());
}

// First pass over a statement list: every named type, so the rest can refer to it.
// A namespace's members are registered the same way, under its name.
fn declare_types<'ast>(items: &[(TopLevelItem<'ast>, bool)], ctx: &mut CheckContext<'ast, '_>) {
    for (item, _) in items.iter().copied() {
        match item {
            TopLevelItem::Alias(decl) => {
                ctx.namespace.insert_type_alias(
                    &decl.id.name,
                    &decl.type_annotation,
                    decl.type_parameters.as_deref(),
                    decl.id.span,
                );
            }
            TopLevelItem::Interface(decl) => {
                ctx.namespace.insert_interface(&decl.id.name, decl);
            }
            TopLevelItem::Class(class) => {
                if let Some(id) = &class.id {
                    ctx.namespace.insert_class(&id.name, class);
                }
            }
            TopLevelItem::Enum(decl) => declare_enum(decl, ctx),
            TopLevelItem::Namespace(ns) => {
                if let Some(block) = namespace_block(ns) {
                    ctx.namespace.push_scope(&ns.id.name);
                    declare_types(&items_in(&block.body), ctx);
                    ctx.namespace.pop_scope();
                }
            }
            TopLevelItem::ExternalModule(module) => {
                if let Some(body) = &module.body {
                    ctx.namespace.push_scope(&module_scope(&module.id.value));
                    declare_types(&items_in(&body.body), ctx);
                    ctx.namespace.pop_scope();
                }
            }
            _ => {}
        }
    }
}

// Second pass: signatures, which can name any type regardless of source order. A
// namespace is declared as a value (an object of its exported members) after its own
// members are, and only when that object is certain to be whole.
fn declare_values<'ast>(
    items: &[(TopLevelItem<'ast>, bool)],
    ctx: &mut CheckContext<'ast, '_>,
    ambient: bool,
) {
    for (item, _) in items.iter().copied() {
        match item {
            TopLevelItem::Variable(decl) => {
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
            TopLevelItem::Function(func) => declare_function(func, ctx),
            TopLevelItem::Class(class) => declare_class_constructor(class, ctx),
            TopLevelItem::ExternalModule(module) => {
                if let Some(body) = &module.body {
                    ctx.namespace.push_scope(&module_scope(&module.id.value));
                    declare_values(&items_in(&body.body), ctx, true);
                    ctx.namespace.pop_scope();
                }
            }
            TopLevelItem::Namespace(ns) => {
                let Some(block) = namespace_block(ns) else {
                    continue;
                };
                let members = items_in(&block.body);
                let inside = ambient || ns.declare;
                ctx.namespace.push_scope(&ns.id.name);
                declare_values(&members, ctx, inside);
                ctx.namespace.pop_scope();
                // Merged with another declaration of the same name (a second block, a
                // function, a class, an enum, a variable), the object would hide the
                // other half, so nothing is declared and the name stays unknown.
                let name = ns.id.name.as_str();
                if items
                    .iter()
                    .filter(|(other, _)| declares_value_named(other, name))
                    .count()
                    > 1
                {
                    continue;
                }
                if let (Some(symbol_id), Some(object)) = (
                    ns.id.symbol_id.get(),
                    namespace_value(&members, inside, ctx),
                ) {
                    ctx.symbols.declare(symbol_id, object);
                }
            }
            _ => {}
        }
    }
}

// Whether `item` introduces a value called `name`, which a namespace of that name would
// merge with.
fn declares_value_named(item: &TopLevelItem<'_>, name: &str) -> bool {
    match item {
        TopLevelItem::Namespace(ns) => ns.id.name == name,
        TopLevelItem::Function(f) => f.id.as_ref().is_some_and(|id| id.name == name),
        TopLevelItem::Class(c) => c.id.as_ref().is_some_and(|id| id.name == name),
        TopLevelItem::Enum(e) => e.id.name == name,
        TopLevelItem::Variable(v) => v
            .declarations
            .iter()
            .any(|d| matches!(&d.id, BindingPattern::BindingIdentifier(id) if id.name == name)),
        TopLevelItem::Alias(_) | TopLevelItem::Interface(_) | TopLevelItem::ExternalModule(_) => {
            false
        }
    }
}

// The object a namespace is as a value: one property per exported value member. None when
// any of them has no type yet (an unannotated variable is typed from its initializer
// later), because an object missing a member would report a property that exists as
// missing. In an ambient namespace every member is exported unless some is marked.
fn namespace_value(
    members: &[(TopLevelItem<'_>, bool)],
    ambient: bool,
    ctx: &mut CheckContext<'_, '_>,
) -> Option<crate::arena::TypeId> {
    let any_marked = members.iter().any(|(_, exported)| *exported);
    let mut properties: Vec<crate::types::PropertyEntry> = Vec::new();
    for (item, exported) in members.iter().copied() {
        if !(exported || (ambient && !any_marked)) {
            continue;
        }
        let mut add = |name: &str, symbol: Option<oxc_semantic::SymbolId>| -> Option<()> {
            let type_id = ctx.symbols.get(symbol?)?;
            properties.push(crate::types::PropertyEntry {
                name: name.into(),
                type_id,
                optional: false,
                is_method: false,
            });
            Some(())
        };
        match item {
            TopLevelItem::Function(f) => {
                let id = f.id.as_ref()?;
                add(&id.name, id.symbol_id.get())?;
            }
            TopLevelItem::Class(c) => {
                let id = c.id.as_ref()?;
                add(&id.name, id.symbol_id.get())?;
            }
            TopLevelItem::Enum(e) => add(&e.id.name, e.id.symbol_id.get())?,
            TopLevelItem::Namespace(n) => add(&n.id.name, n.id.symbol_id.get())?,
            TopLevelItem::Variable(v) => {
                for d in &v.declarations {
                    let BindingPattern::BindingIdentifier(id) = &d.id else {
                        return None;
                    };
                    add(&id.name, id.symbol_id.get())?;
                }
            }
            TopLevelItem::Alias(_)
            | TopLevelItem::Interface(_)
            | TopLevelItem::ExternalModule(_) => {}
        }
    }
    properties.sort_by(|a, b| a.name.cmp(&b.name));
    properties.dedup_by(|a, b| a.name == b.name);
    Some(
        ctx.arena
            .alloc(Type::Object(crate::types::ObjectType::new(properties))),
    )
}

// Registers a function's signature under its symbol. Shared by the top-level pass
// and by check_function_declaration / hoist_function_declarations, which call it
// for a function declared inside another body: the same signature rules apply
// there, and nothing but this pass would ever give such a function a type.
//
// Problem: a nested `function helper(...) {}` was never declared, so its body was
// skipped without a word and every call to it read as an unknown type.
// Picked: declare it on the way into the enclosing body, before any statement in
// that body is checked, so a call written above the declaration still sees it
// (function declarations hoist).
pub(super) fn declare_function(func: &Function<'_>, ctx: &mut CheckContext<'_, '_>) {
    let Some(name) = func.id.as_ref() else {
        return;
    };
    let Some(symbol_id) = name.symbol_id.get() else {
        return;
    };
    // A generic function's own type parameters need to be resolvable
    // by name while its parameter and return type annotations are
    // being resolved, then removed again immediately afterward so
    // they do not leak into any other declaration's resolution.
    let scope = ctx.namespace.push_type_params(&mut ctx.arena, func);
    let params = resolve_function_params(&func.params, &mut ctx.namespace, &mut ctx.arena);
    let return_type = match &func.return_type {
        Some(rt) => resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena),
        // Problem: with no return annotation the function was left undeclared, so
        // its body was never checked and a call to it read as an unknown type.
        // Picked: declare it with `any` as a placeholder return type; the body is
        // then checked against `any` (so nothing in it is a false mismatch) and
        // check_function_declaration re-declares the function with the union of
        // what the body returned once it has been walked.
        // Cost: a call checked before the body is walked sees `any`, and an async
        // or generator function stays undeclared because its real return type
        // (a Promise or an iterator) is not something this checker models.
        None if func.r#async || func.generator => None,
        None => Some(ctx.arena.any()),
    };
    ctx.namespace.pop_type_params(scope);
    // Any parameter or the return type failing to resolve, an unknown
    // name in an annotation for example, means the whole function is
    // left undeclared rather than declared with a guessed type. A
    // later call to it will report an honest unresolved-name problem
    // instead of silently passing or failing arity checks it should
    // not be subject to.
    let (Some(params), Some(return_type)) = (params, return_type) else {
        return;
    };
    let function_type = ctx
        .arena
        .alloc(crate::types::Type::Function(crate::types::FunctionType {
            params,
            return_type,
            is_untyped: false,
        }));
    ctx.symbols.declare(symbol_id, function_type);
}

fn declare_class_constructor(class: &Class<'_>, ctx: &mut CheckContext<'_, '_>) {
    let Some(name) = class.id.as_ref() else {
        return;
    };
    let Some(symbol_id) = name.symbol_id.get() else {
        return;
    };
    let instance_type = match ctx.namespace.resolve(&name.name, &mut ctx.arena) {
        crate::namespace::Resolution::Resolved(type_id) => type_id,
        _ => return,
    };
    let (constructor_params, constructor_is_untyped) = match find_constructor(class) {
        None => (Vec::new(), false),
        Some(ctor) => {
            // A generic class's own type parameters need to be in
            // scope while its constructor's parameters are
            // resolved, the same as a generic function's are just
            // above -- otherwise `T` in `constructor(value: T)`
            // fails to resolve, the whole signature falls back to
            // "untyped, can't check arity", and `new Box(1)` never
            // gets the inference-and-substitution treatment a
            // generic function call already gets in
            // check_callable. This only re-declares the same
            // shadow namespace::resolve already set up while
            // computing instance_type above; type_param_cache
            // means it resolves to the identical GenericParameter
            // TypeId, not a second, disconnected one.
            let scope = ctx
                .namespace
                .push_decl_type_params(&mut ctx.arena, class.type_parameters.as_deref());
            let params = resolve_function_params(&ctor.params, &mut ctx.namespace, &mut ctx.arena);
            ctx.namespace.pop_type_params(scope);
            match params {
                Some(params) => (params, false),
                None => (Vec::new(), true),
            }
        }
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
    // are built from the same resolved member list.
    let (type_position, value_type) = ctx.arena.build_enum(&decl.id.name, members);
    ctx.namespace
        .insert_enum(&decl.id.name, type_position, decl.id.span);
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
