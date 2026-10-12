// What a unit exports, as portable declarations (LLD 3.4).
//
// Runs inside check_file, after the bodies are checked and before the arena is handed
// back, because a portable declaration is read out of the arena and the arena is cleared
// as soon as the unit ends. Nothing here reports a diagnostic and nothing here changes a
// type: it only reads what the declare pass already resolved.
//
// The file's top-level declarations are listed first, each with the names it is exported
// under, and the exports are built from that list. That is what lets one path serve every
// way of exporting a declaration: `export interface A`, `export default function f`, and
// `export { a, b as c }` naming a declaration written elsewhere in the file.
//
// What is exported: an interface, type alias, function, and `const`/`let`/`var`. The
// exports are read after the bodies are checked, so a function without a return
// annotation and a variable without a type annotation are exported with the type that
// was inferred for them. A name that was imported and is exported
// again (`export { X }`, `export { X } from "./a"`) is not a new declaration: the table
// maps it to the key it already has, and the dependent finds the declaration through the
// same provider. An interface or alias that is not exported but that an export names
// (`interface Hidden` and `export function f(h: Hidden)`) is exported under its own key
// and no name: the dependent can follow the reference, and cannot import it. Anything
// else that is exported (a class, an enum, a default expression, `export *`, an
// overload after the first) is left out and marks the table
// incomplete. Leaving an export out makes a dependent see nothing; it never makes it see
// something wrong.

use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression, Function,
    ModuleExportName, Program, Statement, TSTypeParameterDeclaration,
};
use oxc_semantic::SymbolId;
use oxc_span::GetSpan;

use crate::arena::{BUILTIN_GENERICS, DeclSlot, TypeId};
use crate::fxhash::FxHashMap;
use crate::keys::{DeclKey, DeclKind, ModuleKey, TypeParamKey};
use crate::namespace::Resolution;
use crate::portable::{
    DeclInput, Importer, ModuleExports, PNode, PortableDecl, TypeParamInput, export_decl,
};
use crate::stable_walk::KeyResolver;
use crate::types::{Type, TypeParameterId};

use super::context::CheckContext;

/// What `collect_exports` found: the declarations, the names they are exported under,
/// and whether anything exported had to be left out.
#[derive(Default)]
pub(crate) struct ExportTable {
    pub decls: Vec<PortableDecl>,
    /// Export name to the declaration's key. A name that is both a type and a value
    /// (`interface Foo` and `const Foo`) appears once for each; the declaration's kind
    /// tells them apart. A name that is an import exported again maps to a key whose
    /// declaration is in another unit's `decls`.
    pub names: Vec<(String, DeclKey)>,
    pub incomplete: bool,
    /// True when the collector ran at all (the unit had a module key). A table that
    /// was never collected is not a complete list of nothing.
    pub collected: bool,
}

// A top-level declaration the exports may be built from, and the names it is exported
// under (empty for one that is not exported).
struct Item<'ast> {
    name: String,
    kind: DeclKind,
    // The position among the earlier declarations of the same name and kind in the file,
    // counted over every top-level declaration, exported or not.
    index: u32,
    node: Node<'ast>,
    exported_as: Vec<String>,
}

enum Node<'ast> {
    Interface,
    Alias,
    Function(&'ast Function<'ast>),
    Var { symbol: Option<SymbolId> },
    Enum { symbol: Option<SymbolId> },
    // The instance side of a class: read from the namespace like an interface.
    Class,
    // The constructor, the class name used as a value.
    ClassValue { symbol: Option<SymbolId> },
}

// One export waiting for the resolver to be complete: every exported declaration has to
// have its slot and type parameters registered before any of them is written out,
// because they may name each other in any order.
struct Pending {
    key: DeclKey,
    name: String,
    kind: DeclKind,
    body: TypeId,
    type_params: Vec<TypeParamInput>,
    exported_as: Vec<String>,
}

// The unit's side of the contract (LLD 3.4): which slot is which declaration, and which
// type parameter belongs to which. Built from the exports only, so a body that names a
// declaration outside them gets no key and the export is reported as unsupported.
#[derive(Default)]
struct UnitKeys {
    slots: FxHashMap<DeclSlot, (DeclKey, String)>,
    tparams: FxHashMap<TypeParameterId, TypeParamKey>,
}

impl KeyResolver for UnitKeys {
    fn decl_key(&self, slot: DeclSlot) -> Option<DeclKey> {
        self.slots.get(&slot).map(|(key, _)| *key)
    }
    fn decl_name(&self, slot: DeclSlot) -> Option<String> {
        self.slots.get(&slot).map(|(_, name)| name.clone())
    }
    fn type_param_key(&self, id: TypeParameterId) -> Option<TypeParamKey> {
        self.tparams.get(&id).copied()
    }
}

// The unit's own declarations, then the ones it imported: an export may name a type that
// came from another file, and that type keeps the key it has in its own file.
struct WithImports<'a> {
    unit: &'a UnitKeys,
    imported: &'a Importer,
}

impl KeyResolver for WithImports<'_> {
    fn decl_key(&self, slot: DeclSlot) -> Option<DeclKey> {
        self.unit
            .decl_key(slot)
            .or_else(|| self.imported.decl_key(slot))
    }
    fn decl_name(&self, slot: DeclSlot) -> Option<String> {
        self.unit
            .decl_name(slot)
            .or_else(|| self.imported.decl_name(slot))
    }
    fn type_param_key(&self, id: TypeParameterId) -> Option<TypeParamKey> {
        self.unit
            .type_param_key(id)
            .or_else(|| self.imported.type_param_key(id))
    }
}

// `export { local as exported }`, before it is matched to a declaration.
struct Specifier {
    local: String,
    exported: String,
}

fn export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(n) => n.name.to_string(),
        ModuleExportName::IdentifierReference(n) => n.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

// Adds a declaration to the list and gives it its merge index.
fn push_item<'ast>(
    items: &mut Vec<Item<'ast>>,
    seen: &mut FxHashMap<(String, u8), u32>,
    name: &str,
    kind: DeclKind,
    node: Node<'ast>,
    exported_as: Vec<String>,
) {
    let slot = seen.entry((name.to_string(), kind as u8)).or_insert(0);
    let index = *slot;
    *slot += 1;
    items.push(Item {
        name: name.to_string(),
        kind,
        index,
        node,
        exported_as,
    });
}

fn push_declaration<'ast>(
    items: &mut Vec<Item<'ast>>,
    seen: &mut FxHashMap<(String, u8), u32>,
    declaration: &'ast Declaration<'ast>,
    exported: bool,
    incomplete: &mut bool,
) {
    let named = |name: &str| {
        if exported {
            vec![name.to_string()]
        } else {
            Vec::new()
        }
    };
    match declaration {
        Declaration::TSInterfaceDeclaration(d) => {
            let names = named(&d.id.name);
            push_item(
                items,
                seen,
                &d.id.name,
                DeclKind::Interface,
                Node::Interface,
                names,
            );
        }
        Declaration::TSTypeAliasDeclaration(d) => {
            let names = named(&d.id.name);
            push_item(items, seen, &d.id.name, DeclKind::Alias, Node::Alias, names);
        }
        Declaration::FunctionDeclaration(f) => {
            if let Some(id) = &f.id {
                let names = named(&id.name);
                push_item(
                    items,
                    seen,
                    &id.name,
                    DeclKind::Function,
                    Node::Function(f),
                    names,
                );
            } else if exported {
                *incomplete = true;
            }
        }
        Declaration::VariableDeclaration(v) => {
            for declarator in &v.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    // `export const { a, b } = ...` has no annotation of its own to export.
                    if exported {
                        *incomplete = true;
                    }
                    continue;
                };
                let names = named(&id.name);
                let node = Node::Var {
                    symbol: id.symbol_id.get(),
                };
                push_item(items, seen, &id.name, DeclKind::Var, node, names);
            }
        }
        Declaration::TSEnumDeclaration(d) => {
            let names = named(&d.id.name);
            let node = Node::Enum {
                symbol: d.id.symbol_id.get(),
            };
            push_item(items, seen, &d.id.name, DeclKind::Enum, node, names);
        }
        Declaration::ClassDeclaration(c) => {
            if let Some(id) = &c.id {
                push_class(items, seen, &id.name, id.symbol_id.get(), named(&id.name));
            } else if exported {
                *incomplete = true;
            }
        }
        // A namespace or a module block has a value side the portable form does not carry
        // yet. Not exported, they are nothing to us.
        _ => {
            if exported {
                *incomplete = true;
            }
        }
    }
}

// In order: list the file's top-level declarations with the names each is exported
// under, then attach `export { a as b }` specifiers (to them, or to what the file
// imported) and the re-exports from other modules. Every interface and alias is
// registered before any is written out, exported or not, so a body may name one that is
// not exported and the bodies may name each other in any order. Everything built is then
// written against the finished key table, and only the exported ones, with the
// declarations they reach, leave the unit. `export *` is merged last, since a name the
// file exports itself wins over a star's. Whatever cannot be exported cleanly sets
// `incomplete` instead of being guessed, so a dependent sees fewer names, never a wrong
// one.
pub(crate) fn collect_exports<'ast>(
    program: &'ast Program<'ast>,
    ctx: &mut CheckContext<'ast, '_>,
    module: ModuleKey,
) -> ExportTable {
    let mut table = ExportTable {
        collected: true,
        ..ExportTable::default()
    };
    let mut items: Vec<Item<'ast>> = Vec::new();
    let mut seen: FxHashMap<(String, u8), u32> = FxHashMap::default();
    let mut specifiers: Vec<Specifier> = Vec::new();
    // `export { a } from "./a"`: the source as written, and its specifiers.
    let mut from_modules: Vec<(String, Vec<Specifier>)> = Vec::new();
    // `export * from "./a"`: the source as written.
    let mut stars: Vec<String> = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ExportDeclaration(export) => push_declaration(
                &mut items,
                &mut seen,
                &export.declaration,
                true,
                &mut table.incomplete,
            ),
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) if f.id.is_some() => {
                    let id = f.id.as_ref().map(|id| id.name.as_str()).unwrap_or_default();
                    let names = vec!["default".to_string()];
                    push_item(
                        &mut items,
                        &mut seen,
                        id,
                        DeclKind::Function,
                        Node::Function(f),
                        names,
                    );
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(d) => {
                    let names = vec!["default".to_string()];
                    push_item(
                        &mut items,
                        &mut seen,
                        &d.id.name,
                        DeclKind::Interface,
                        Node::Interface,
                        names,
                    );
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) if c.id.is_some() => {
                    if let Some(id) = &c.id {
                        push_class(
                            &mut items,
                            &mut seen,
                            &id.name,
                            id.symbol_id.get(),
                            vec!["default".to_string()],
                        );
                    }
                }
                // `export default Foo` names a declaration of this file, so it is
                // `export { Foo as default }`.
                ExportDefaultDeclarationKind::Identifier(id) => specifiers.push(Specifier {
                    local: id.name.to_string(),
                    exported: "default".to_string(),
                }),
                // An anonymous function or class, or any other expression: no declaration
                // of ours to give a key to.
                _ => table.incomplete = true,
            },
            Statement::ExportNamedDeclaration(named) => {
                for spec in &named.specifiers {
                    specifiers.push(Specifier {
                        local: export_name(&spec.local),
                        exported: export_name(&spec.exported),
                    });
                }
            }
            Statement::ExportFromDeclaration(from) => {
                let list = from
                    .specifiers
                    .iter()
                    .map(|spec| Specifier {
                        local: export_name(&spec.local),
                        exported: export_name(&spec.exported),
                    })
                    .collect();
                from_modules.push((from.source.value.to_string(), list));
            }
            // `export * from "./a"` re-exports what ./a exports, read once the file's own
            // exports are known. `export * as ns from` needs a namespace object type
            // the portable form does not have.
            Statement::ExportAllDeclaration(all) => {
                if all.exported.is_some() {
                    table.incomplete = true;
                } else {
                    stars.push(all.source.value.to_string());
                }
            }
            Statement::TSInterfaceDeclaration(d) => push_item(
                &mut items,
                &mut seen,
                &d.id.name,
                DeclKind::Interface,
                Node::Interface,
                Vec::new(),
            ),
            Statement::TSTypeAliasDeclaration(d) => push_item(
                &mut items,
                &mut seen,
                &d.id.name,
                DeclKind::Alias,
                Node::Alias,
                Vec::new(),
            ),
            Statement::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    push_class(
                        &mut items,
                        &mut seen,
                        &id.name,
                        id.symbol_id.get(),
                        Vec::new(),
                    );
                }
            }
            // `export = x` and `export as namespace X` change what the module exports in a
            // way this pass does not read, so the list cannot be called complete.
            // `export = name` hands the module's whole value to one declaration. It is
            // listed under the name `export=`, which no source can write, so an
            // `import x = require()` or a default import can find it. The module stays
            // partial, because a named import of one of its properties is still legal.
            Statement::TSExportAssignment(assign) => {
                table.incomplete = true;
                if let Expression::Identifier(id) = &assign.expression {
                    specifiers.push(Specifier {
                        local: id.name.to_string(),
                        exported: "export=".to_string(),
                    });
                }
            }
            Statement::TSNamespaceExportDeclaration(_) => table.incomplete = true,
            Statement::TSEnumDeclaration(d) => {
                let node = Node::Enum {
                    symbol: d.id.symbol_id.get(),
                };
                push_item(
                    &mut items,
                    &mut seen,
                    &d.id.name,
                    DeclKind::Enum,
                    node,
                    Vec::new(),
                );
            }
            Statement::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    push_item(
                        &mut items,
                        &mut seen,
                        &id.name,
                        DeclKind::Function,
                        Node::Function(f),
                        Vec::new(),
                    );
                }
            }
            Statement::VariableDeclaration(v) => {
                for declarator in &v.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        let node = Node::Var {
                            symbol: id.symbol_id.get(),
                        };
                        push_item(
                            &mut items,
                            &mut seen,
                            &id.name,
                            DeclKind::Var,
                            node,
                            Vec::new(),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    // `export { a, b as c }`: each names a declaration of this file (any kind that has
    // that name, so a type and a value both go out), or something this file imported.
    for spec in specifiers {
        let mut found = false;
        for item in items.iter_mut().filter(|item| item.name == spec.local) {
            item.exported_as.push(spec.exported.clone());
            found = true;
        }
        if found {
            continue;
        }
        match ctx.imported_names.get(&spec.local) {
            Some(keys) if !keys.is_empty() => {
                for key in keys {
                    table.names.push((spec.exported.clone(), *key));
                }
            }
            // An unknown local name is an error tsc reports; there is nothing to export.
            _ => table.incomplete = true,
        }
    }
    // `export { a } from "./a"`: the other module's own export, under its own key.
    for (source, list) in from_modules {
        for spec in list {
            let keys = ctx
                .imports
                .map(|imports| imports.export_keys(&source, &spec.local))
                .unwrap_or_default();
            if keys.is_empty() {
                table.incomplete = true;
            }
            for key in keys {
                table.names.push((spec.exported.clone(), key));
            }
        }
    }

    let mut keys = UnitKeys::default();
    let mut pending: Vec<Pending> = Vec::new();
    // In a declaration file a top-level declaration with no `export` is exported anyway
    // unless the file says otherwise (`export {}`), and which of the two it is cannot be
    // told here. So such a file's list is partial: what it names explicitly still crosses,
    // and a name missing from it is never reported.
    if program.source_type.is_typescript_definition()
        && items.iter().any(|item| item.exported_as.is_empty())
    {
        table.incomplete = true;
    }

    // Every interface and alias of the file is registered, exported or not: an exported
    // declaration may name one that is not (`interface Hidden` and `export function
    // f(h: Hidden)`), and it can only be written out with a key if the resolver knows it.
    // A declaration that cannot be built is skipped when it is not exported, and marks
    // the table incomplete when it is.
    for item in &items {
        let exported = !item.exported_as.is_empty();
        if !exported
            && !matches!(
                item.kind,
                DeclKind::Interface | DeclKind::Alias | DeclKind::Class
            )
        {
            continue;
        }
        let built = match &item.node {
            Node::Interface | Node::Alias => interface_or_alias(ctx, module, item),
            Node::Function(f) => function(ctx, module, f, item),
            Node::Var { symbol } => variable(ctx, module, *symbol, item),
            Node::Enum { symbol } => enumeration(ctx, module, *symbol, item),
            Node::Class => interface_or_alias(ctx, module, item),
            Node::ClassValue { symbol } => variable(ctx, module, *symbol, item),
        };
        match built.and_then(|p| register(&mut keys, ctx, p)) {
            Some(p) => pending.push(p),
            None if exported => table.incomplete = true,
            None => {}
        }
    }

    // Builtins (`Promise<T>`, `Record<K, V>`) are the one kind of declaration a body may
    // name without it being exported: they resolve to the built-in key.
    for name in BUILTIN_GENERICS {
        if let Some(slot) = ctx.arena.existing_builtin_slot(name) {
            keys.slots
                .insert(slot, (DeclKey::builtin(name), name.to_string()));
        }
    }

    let resolver = WithImports {
        unit: &keys,
        imported: &ctx.importer,
    };
    // Every candidate is written out; which of them leave the unit is decided below.
    let mut written: FxHashMap<DeclKey, (PortableDecl, Vec<String>)> = FxHashMap::default();
    let mut order: Vec<DeclKey> = Vec::new();
    for p in pending {
        let input = DeclInput {
            key: p.key,
            name: &p.name,
            kind: p.kind,
            body: p.body,
            type_params: &p.type_params,
        };
        match export_decl(&ctx.arena, &resolver, &input) {
            Some(decl) => {
                order.push(p.key);
                written.insert(p.key, (decl, p.exported_as));
            }
            None if !p.exported_as.is_empty() => table.incomplete = true,
            None => {}
        }
    }

    // The exported declarations leave under their names. A declaration they reach but do
    // not export leaves under its own key and no name, so a dependent can follow the
    // reference without being able to import it. Reached ones are found by walking the
    // references of what already left, to a fixed point.
    let mut leaving: Vec<DeclKey> = order
        .iter()
        .copied()
        .filter(|key| !written[key].1.is_empty())
        .collect();
    let mut seen: std::collections::HashSet<DeclKey> = leaving.iter().copied().collect();
    let mut cursor = 0;
    while cursor < leaving.len() {
        let key = leaving[cursor];
        cursor += 1;
        for node in written[&key].0.body.nodes.iter() {
            if let PNode::DeclRef { key: reached, .. } | PNode::App { key: reached, .. } = node {
                if written.contains_key(reached) && seen.insert(*reached) {
                    leaving.push(*reached);
                }
            }
        }
    }
    // A name the file exports itself wins over one a star re-export brings in, and the
    // first star wins over a later one. The re-exported declarations are another unit's,
    // found through the provider under the key they already have. A star over a module
    // whose own list is partial, or that is not known, is partial too.
    for source in stars {
        let imports = ctx.imports;
        let known = imports.map(|i| i.module_exports(&source));
        if !matches!(known, Some(ModuleExports::Known { complete: true })) {
            table.incomplete = true;
        }
        let local: std::collections::HashSet<String> = order
            .iter()
            .flat_map(|key| written[key].1.iter().cloned())
            .chain(table.names.iter().map(|(name, _)| name.clone()))
            .collect();
        for (name, key) in imports.map(|i| i.export_all(&source)).unwrap_or_default() {
            if !local.contains(&name) {
                table.names.push((name, key));
            }
        }
    }
    for key in order {
        if !seen.contains(&key) {
            continue;
        }
        if let Some((decl, names)) = written.remove(&key) {
            for name in names {
                table.names.push((name, key));
            }
            table.decls.push(decl);
        }
    }
    table
}

// An interface or an alias, read out of the namespace. The type id is the one every use
// of the name resolves to, so its slot is the slot the resolver has to know.
fn interface_or_alias(
    ctx: &mut CheckContext<'_, '_>,
    module: ModuleKey,
    item: &Item<'_>,
) -> Option<Pending> {
    // A second part of a merged interface is already inside the first's body, and a
    // redeclared alias is an error reported elsewhere.
    if item.index > 0 {
        return None;
    }
    let Resolution::Resolved(type_id) = ctx.namespace.resolve(&item.name, &mut ctx.arena) else {
        return None;
    };
    // An interface, a class and a type-literal alias come back as a Ref, a named alias
    // as a Named; a generic alias or a primitive alias is the body itself and nothing
    // refers to it by slot.
    let body = ctx
        .arena
        .decl_parts(type_id)
        .map_or(type_id, |(_, body)| body);
    Some(pending_of(module, item, body))
}

fn function<'ast>(
    ctx: &mut CheckContext<'ast, '_>,
    module: ModuleKey,
    f: &'ast Function<'ast>,
    item: &Item<'_>,
) -> Option<Pending> {
    let id = f.id.as_ref()?;
    // The first overload is the only one the checker declares; a later one would be
    // exported under a key that has no type behind it. A function without a return
    // annotation was declared with a placeholder `any` and its body has been walked by
    // the time exports are read, so the signature holds the inferred return type.
    if item.index > 0 {
        return None;
    }
    let body = ctx.symbols.get(id.symbol_id.get()?)?;
    with_type_params(
        ctx,
        pending_of(module, item, body),
        f.type_parameters.as_deref(),
    )
}

fn variable(
    ctx: &mut CheckContext<'_, '_>,
    module: ModuleKey,
    symbol: Option<SymbolId>,
    item: &Item<'_>,
) -> Option<Pending> {
    // The type is whatever the symbol holds once the bodies are checked: the annotation
    // when there is one, otherwise what the initializer inferred (a `const` keeps its
    // literal, a `let` is widened), which is the type tsc writes into a declaration file.
    if item.index > 0 {
        return None;
    }
    let body = ctx.symbols.get(symbol?)?;
    Some(pending_of(module, item, body))
}

// An enum is exported by its value-position type: an object of the member literals, which
// is all the importer needs to rebuild both the type and the value. A merged enum, or one
// the declare pass could not resolve (a computed member), has no such type here and is
// left out.
fn enumeration(
    ctx: &mut CheckContext<'_, '_>,
    module: ModuleKey,
    symbol: Option<SymbolId>,
    item: &Item<'_>,
) -> Option<Pending> {
    if item.index > 0 {
        return None;
    }
    let body = ctx.symbols.get(symbol?)?;
    Some(pending_of(module, item, body))
}

// A class is two declarations that share its name: the instance type, which another type
// can name, and the constructor, which is the name used as a value. Each is exported under
// the same names, and an import of the name binds both.
fn push_class<'ast>(
    items: &mut Vec<Item<'ast>>,
    seen: &mut FxHashMap<(String, u8), u32>,
    name: &str,
    symbol: Option<SymbolId>,
    names: Vec<String>,
) {
    push_item(
        items,
        seen,
        name,
        DeclKind::Class,
        Node::Class,
        names.clone(),
    );
    push_item(
        items,
        seen,
        name,
        DeclKind::Var,
        Node::ClassValue { symbol },
        names,
    );
}

fn pending_of(module: ModuleKey, item: &Item<'_>, body: TypeId) -> Pending {
    Pending {
        key: DeclKey::new(module, &item.name, item.kind, item.index),
        name: item.name.clone(),
        kind: item.kind,
        body,
        type_params: Vec::new(),
        exported_as: item.exported_as.clone(),
    }
}

// Registers an export with the resolver. An interface's type parameters may sit on a
// merged part and not on the exported one, so for an interface or an alias the namespace
// says which declaration actually carries them. None when the declaration cannot be read
// completely, which the caller reports as an unsupported export.
fn register<'ast>(
    keys: &mut UnitKeys,
    ctx: &mut CheckContext<'ast, '_>,
    mut p: Pending,
) -> Option<Pending> {
    if matches!(
        p.kind,
        DeclKind::Interface | DeclKind::Alias | DeclKind::Class
    ) {
        let declared = ctx.namespace.declared_type_param_decl(&p.name);
        p = with_type_params(ctx, p, declared)?;
        // The slot the rest of the unit knows this declaration by.
        if let Resolution::Resolved(type_id) = ctx.namespace.resolve(&p.name, &mut ctx.arena) {
            if let Some((slot, _)) = ctx.arena.decl_parts(type_id) {
                keys.slots.insert(slot, (p.key, p.name.clone()));
            }
        }
    }
    for (i, tp) in p.type_params.iter().enumerate() {
        keys.tparams.insert(
            tp.id,
            TypeParamKey {
                owner: p.key,
                index: i as u32,
            },
        );
    }
    Some(p)
}

// The declaration's own `<T extends B = D, ...>` list as export inputs. The bound is read
// off the GenericParameter node the declare pass already allocated for the parameter, so
// it is the very type the body was resolved against. The default is resolved again with
// the declaration's parameters in scope (the namespace memoizes the parameter nodes, so
// a default that names an earlier parameter gets that same node). None when a parameter
// has no node, which means the declaration was never resolved with it in scope.
fn with_type_params<'ast>(
    ctx: &mut CheckContext<'ast, '_>,
    mut p: Pending,
    declared: Option<&'ast TSTypeParameterDeclaration<'ast>>,
) -> Option<Pending> {
    let Some(declared) = declared else {
        return Some(p);
    };
    let file_id = ctx.namespace.file_id();
    for (i, param) in declared.params.iter().enumerate() {
        let id = TypeParameterId::with_file(file_id, param.span().start, i as u32);
        let node = ctx.namespace.type_param_node(id)?;
        let Type::GenericParameter(_, _, bound) = ctx.arena.raw(node) else {
            return None;
        };
        let bound = *bound;
        let default = if param.default.is_some() {
            // A default that cannot be resolved cannot be exported.
            Some(
                ctx.namespace
                    .resolve_type_param_default(&mut ctx.arena, declared, i)?,
            )
        } else {
            None
        };
        p.type_params.push(TypeParamInput {
            id,
            name: param.name.name.to_string(),
            bound,
            default,
        });
    }
    Some(p)
}
