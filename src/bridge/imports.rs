// Step 6b: binding the names an `import` declaration brings in.
//
// Runs at the start of the declare pass, before the file's own declarations, so every
// later phase sees an imported name as it sees a local one: a type name resolves in the
// namespace, a value name has a type under its symbol. Nothing here reports a
// diagnostic. A specifier the project does not know, or a name the module does not
// export, binds nothing and the name stays unknown, which loses an error and never
// invents one (the same rule exports follow when they leave something out).

use oxc_ast::ast::{
    ImportDeclaration, ImportDeclarationSpecifier, ImportOrExportKind, ModuleExportName, Program,
    Statement,
};

use crate::keys::{DeclKey, DeclKind};
use crate::portable::{DeclProvider, ImportSource};

use super::context::CheckContext;

pub(super) fn bind_imports<'ast>(program: &'ast Program<'ast>, ctx: &mut CheckContext<'ast, '_>) {
    let Some(source) = ctx.imports else {
        return;
    };
    for stmt in &program.body {
        if let Statement::ImportDeclaration(decl) = stmt {
            bind_declaration(decl, source, ctx);
        }
    }
}

fn bind_declaration<'ast>(
    decl: &ImportDeclaration<'_>,
    source: &dyn ImportSource,
    ctx: &mut CheckContext<'ast, '_>,
) {
    let specifier = decl.source.value.as_str();
    let whole_declaration_is_type = decl.import_kind == ImportOrExportKind::Type;
    for item in decl.specifiers.iter().flatten() {
        match item {
            ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                let imported = match &spec.imported {
                    ModuleExportName::IdentifierName(n) => n.name.as_str(),
                    ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                    ModuleExportName::StringLiteral(s) => s.value.as_str(),
                };
                let type_only =
                    whole_declaration_is_type || spec.import_kind == ImportOrExportKind::Type;
                bind_name(ctx, source, specifier, imported, &spec.local, type_only);
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                bind_name(
                    ctx,
                    source,
                    specifier,
                    "default",
                    &spec.local,
                    whole_declaration_is_type,
                );
            }
            // `import * as ns` needs a namespace object type the portable form does not
            // have yet.
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => {}
        }
    }
}

// Binds `local` to every declaration the module exports under `imported`. A name that
// is both a type and a value (`interface Foo` and `const Foo`) is bound as both, as it
// is in the file that declares it.
fn bind_name<'ast>(
    ctx: &mut CheckContext<'ast, '_>,
    source: &dyn ImportSource,
    specifier: &str,
    imported: &str,
    local: &oxc_ast::ast::BindingIdentifier<'_>,
    type_only: bool,
) {
    for key in source.export_keys(specifier, imported) {
        bind_key(ctx, source, key, local, type_only);
    }
}

fn bind_key<'ast>(
    ctx: &mut CheckContext<'ast, '_>,
    source: &dyn ImportSource,
    key: DeclKey,
    local: &oxc_ast::ast::BindingIdentifier<'_>,
    type_only: bool,
) {
    let provider: &dyn DeclProvider = source;
    let Some(decl) = provider.decl(key) else {
        return;
    };
    // Remembered before the kind is looked at, so that a name this file only passes on
    // (`import { X } from "./a"; export { X }`) still finds the declaration.
    if !(type_only && matches!(decl.kind, DeclKind::Function | DeclKind::Var)) {
        ctx.imported_names
            .entry(local.name.to_string())
            .or_default()
            .push(key);
    }
    match decl.kind {
        DeclKind::Interface | DeclKind::Class | DeclKind::Alias => {
            let Some(type_id) = ctx.importer.import_decl(&mut ctx.arena, provider, key) else {
                return;
            };
            // A generic one carries its parameters, so `Box<number>` in this file can
            // be instantiated without the declaration's AST.
            let params = ctx
                .importer
                .generic_params(&mut ctx.arena, provider, key)
                .unwrap_or_default();
            ctx.namespace.insert_imported(&local.name, type_id, params);
        }
        // `import type { f }` brings in no value, only the right to name types.
        DeclKind::Function | DeclKind::Var if !type_only => {
            let Some(type_id) = ctx.importer.import_decl(&mut ctx.arena, provider, key) else {
                return;
            };
            if let Some(symbol_id) = local.symbol_id.get() {
                ctx.symbols.declare(symbol_id, type_id);
            }
        }
        _ => {}
    }
}
