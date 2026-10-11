// Step 6a: what a unit exports, as portable declarations (LLD 3.4).
//
// Runs inside check_file, after the bodies are checked and before the arena is handed
// back, because a portable declaration is read out of the arena and the arena is cleared
// as soon as the unit ends. Nothing here reports a diagnostic and nothing here changes a
// type: it only reads what the declare pass already resolved.
//
// What is exported: an `export`ed interface, type alias, function with a return
// annotation, and `const`/`let`/`var` with a type annotation. Anything else that is
// exported (a class, an enum, a default export, a re-export, an unannotated value, an
// overload after the first, a declaration whose body names something that is not itself
// exported) is left out and marks the table incomplete. Leaving an export out makes a
// dependent see nothing; it never makes it see something wrong.

use oxc_ast::ast::{
    BindingPattern, Declaration, Function, Program, Statement, TSTypeParameterDeclaration,
    VariableDeclaration,
};
use oxc_span::GetSpan;

use crate::arena::{BUILTIN_GENERICS, DeclSlot, TypeId};
use crate::fxhash::FxHashMap;
use crate::keys::{DeclKey, DeclKind, ModuleKey, TypeParamKey};
use crate::namespace::Resolution;
use crate::portable::{DeclInput, PortableDecl, TypeParamInput, export_decl};
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
    /// tells them apart.
    pub names: Vec<(String, DeclKey)>,
    pub incomplete: bool,
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

pub(crate) fn collect_exports<'ast>(
    program: &'ast Program<'ast>,
    ctx: &mut CheckContext<'ast, '_>,
    module: ModuleKey,
) -> ExportTable {
    let mut table = ExportTable::default();
    let mut keys = UnitKeys::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut seen: FxHashMap<(String, u8), u32> = FxHashMap::default();

    for stmt in &program.body {
        match stmt {
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::TSInterfaceDeclaration(d) => {
                    let index = next_index(&mut seen, &d.id.name, DeclKind::Interface);
                    match interface_or_alias(ctx, module, &d.id.name, DeclKind::Interface, index) {
                        Some(p) => register(&mut keys, &mut pending, ctx, p),
                        None => table.incomplete = true,
                    }
                }
                Declaration::TSTypeAliasDeclaration(d) => {
                    let index = next_index(&mut seen, &d.id.name, DeclKind::Alias);
                    match interface_or_alias(ctx, module, &d.id.name, DeclKind::Alias, index) {
                        Some(p) => register(&mut keys, &mut pending, ctx, p),
                        None => table.incomplete = true,
                    }
                }
                Declaration::FunctionDeclaration(f) => {
                    let name = f.id.as_ref().map(|id| id.name.to_string());
                    let index = name
                        .as_deref()
                        .map(|n| next_index(&mut seen, n, DeclKind::Function))
                        .unwrap_or(0);
                    match function(ctx, module, f, index) {
                        Some(p) => register(&mut keys, &mut pending, ctx, p),
                        None => table.incomplete = true,
                    }
                }
                Declaration::VariableDeclaration(v) => {
                    variables(
                        ctx,
                        module,
                        v,
                        &mut seen,
                        &mut keys,
                        &mut pending,
                        &mut table,
                    );
                }
                // A class or an enum has a value side the portable form does not carry
                // yet; a namespace or a module block likewise.
                _ => table.incomplete = true,
            },
            // Declarations that are not exported still take a merge index, so an
            // `interface Foo` after one that is not exported is the second Foo.
            Statement::TSInterfaceDeclaration(d) => {
                next_index(&mut seen, &d.id.name, DeclKind::Interface);
            }
            Statement::TSTypeAliasDeclaration(d) => {
                next_index(&mut seen, &d.id.name, DeclKind::Alias);
            }
            Statement::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    next_index(&mut seen, &id.name, DeclKind::Function);
                }
            }
            Statement::ExportDefaultDeclaration(_)
            | Statement::ExportNamedDeclaration(_)
            | Statement::ExportAllDeclaration(_) => table.incomplete = true,
            _ => {}
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

    for p in pending {
        let input = DeclInput {
            key: p.key,
            name: &p.name,
            kind: p.kind,
            body: p.body,
            type_params: &p.type_params,
        };
        match export_decl(&ctx.arena, &keys, &input) {
            Some(decl) => {
                table.names.push((p.name, p.key));
                table.decls.push(decl);
            }
            None => table.incomplete = true,
        }
    }
    table
}

// The position of this declaration among the earlier ones of the same name and kind in
// the file, counted over every top-level declaration, exported or not.
fn next_index(seen: &mut FxHashMap<(String, u8), u32>, name: &str, kind: DeclKind) -> u32 {
    let slot = seen.entry((name.to_string(), kind as u8)).or_insert(0);
    let index = *slot;
    *slot += 1;
    index
}

// An interface or an alias, read out of the namespace. The type id is the one every use
// of the name resolves to, so its slot is the slot the resolver has to know.
fn interface_or_alias(
    ctx: &mut CheckContext<'_, '_>,
    module: ModuleKey,
    name: &str,
    kind: DeclKind,
    index: u32,
) -> Option<Pending> {
    // A second part of a merged interface is already inside the first's body, and a
    // redeclared alias is an error reported elsewhere.
    if index > 0 {
        return None;
    }
    let Resolution::Resolved(type_id) = ctx.namespace.resolve(name, &mut ctx.arena) else {
        return None;
    };
    // An interface, a class and a type-literal alias come back as a Ref, a named alias
    // as a Named; a generic alias or a primitive alias is the body itself and nothing
    // refers to it by slot.
    let body = ctx
        .arena
        .decl_parts(type_id)
        .map_or(type_id, |(_, body)| body);
    Some(Pending {
        key: DeclKey::new(module, name, kind, index),
        name: name.to_string(),
        kind,
        body,
        type_params: Vec::new(),
    })
}

fn function(
    ctx: &mut CheckContext<'_, '_>,
    module: ModuleKey,
    f: &Function<'_>,
    index: u32,
) -> Option<Pending> {
    let id = f.id.as_ref()?;
    // The first overload is the only one the checker declares; a later one would be
    // exported under a key that has no type behind it. A function without a return
    // annotation is declared with a placeholder `any` until its body is walked, which
    // is a guess and not a signature.
    if index > 0 || f.return_type.is_none() {
        return None;
    }
    let body = ctx.symbols.get(id.symbol_id.get()?)?;
    Some(Pending {
        key: DeclKey::new(module, &id.name, DeclKind::Function, index),
        name: id.name.to_string(),
        kind: DeclKind::Function,
        body,
        type_params: Vec::new(),
    })
    .and_then(|p| with_type_params(ctx, p, f.type_parameters.as_deref()))
}

fn variables(
    ctx: &mut CheckContext<'_, '_>,
    module: ModuleKey,
    v: &VariableDeclaration<'_>,
    seen: &mut FxHashMap<(String, u8), u32>,
    keys: &mut UnitKeys,
    pending: &mut Vec<Pending>,
    table: &mut ExportTable,
) {
    for declarator in &v.declarations {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            // `export const { a, b } = ...` has no annotation of its own to export.
            table.incomplete = true;
            continue;
        };
        let index = next_index(seen, &id.name, DeclKind::Var);
        let body = declarator
            .type_annotation
            .as_ref()
            .and_then(|_| ctx.symbols.get(id.symbol_id.get()?));
        match body {
            Some(body) if index == 0 => {
                let p = Pending {
                    key: DeclKey::new(module, &id.name, DeclKind::Var, index),
                    name: id.name.to_string(),
                    kind: DeclKind::Var,
                    body,
                    type_params: Vec::new(),
                };
                register(keys, pending, ctx, p);
            }
            _ => table.incomplete = true,
        }
    }
}

// Registers an export with the resolver and queues it. An interface's type parameters
// may sit on a merged part and not on the exported one, so for an interface or an alias
// the namespace says which declaration actually carries them.
fn register(
    keys: &mut UnitKeys,
    pending: &mut Vec<Pending>,
    ctx: &mut CheckContext<'_, '_>,
    mut p: Pending,
) {
    if matches!(p.kind, DeclKind::Interface | DeclKind::Alias) {
        let declared = ctx.namespace.declared_type_param_decl(&p.name);
        match with_type_params(ctx, p, declared) {
            Some(with) => p = with,
            None => return,
        }
    }
    if matches!(p.kind, DeclKind::Interface | DeclKind::Alias) {
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
    pending.push(p);
}

// The declaration's own `<T extends B, ...>` list as export inputs. The bound is read off
// the GenericParameter node the declare pass already allocated for the parameter, so it
// is the very type the body was resolved against. None when a parameter has no node,
// which means the declaration was never resolved with it in scope.
fn with_type_params(
    ctx: &CheckContext<'_, '_>,
    mut p: Pending,
    declared: Option<&TSTypeParameterDeclaration<'_>>,
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
        p.type_params.push(TypeParamInput {
            id,
            name: param.name.name.to_string(),
            bound: *bound,
        });
    }
    Some(p)
}
