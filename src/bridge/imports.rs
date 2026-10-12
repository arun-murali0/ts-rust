// Step 6b: binding the names an `import` declaration brings in.
//
// Runs at the start of the declare pass, before the file's own declarations, so every
// later phase sees an imported name as it sees a local one: a type name resolves in the
// namespace, a value name has a type under its symbol. A specifier the project does not
// know, or a name the module does not export, binds nothing and the name stays unknown.
//
// Problem: that silence also hid a typo. `import { Usr } from "./a"` bound nothing and
// reported nothing, so a broken import looked clean.
// Picked: report only what is certain. A relative specifier no file answers is TS2307.
// A name missing from a module whose export list is complete is TS2305 (TS1192 for a
// default). Against a list that left something out (a class, `export *`, an inferred
// value) the name may be one we dropped, so nothing is reported.
// Cost: a bare specifier ("react") is never reported as missing until packages are read,
// and a name missing from a partial module is still silent.

use oxc_span::{GetSpan, Span};

use crate::diagnostic_messages::messages;
use oxc_ast::ast::{
    ImportDeclaration, ImportDeclarationSpecifier, ImportOrExportKind, ModuleExportName, Program,
    Statement, TSModuleReference,
};

use crate::keys::{DeclKey, DeclKind};
use crate::portable::{DeclProvider, ImportSource, ModuleExports};
use crate::types::{ObjectType, PropertyEntry, Type};

use super::context::CheckContext;

pub(super) fn bind_imports<'ast>(program: &'ast Program<'ast>, ctx: &mut CheckContext<'ast, '_>) {
    let Some(source) = ctx.imports else {
        return;
    };
    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                report_missing_module(source, decl.source.value.as_str(), decl.source.span, ctx);
                bind_declaration(decl, source, ctx);
            }
            Statement::ExportFromDeclaration(from) => {
                let specifier = from.source.value.as_str();
                report_missing_module(source, specifier, from.source.span, ctx);
                for spec in &from.specifiers {
                    let name = module_export_name(&spec.local);
                    report_missing_name(source, specifier, &name, spec.local.span(), ctx);
                }
            }
            Statement::ExportAllDeclaration(all) => {
                report_missing_module(source, all.source.value.as_str(), all.source.span, ctx);
            }
            // `import x = require(\"./a\")` is `import * as x` for a module with named
            // exports; for one that uses `export =` the list is partial and `x` stays
            // unbound.
            Statement::TSImportEqualsDeclaration(decl) => {
                if let TSModuleReference::ExternalModuleReference(reference) =
                    &decl.module_reference
                {
                    let specifier = reference.expression.value.as_str();
                    report_missing_module(source, specifier, reference.expression.span, ctx);
                    // A module that says `export = x` is that one thing; any other is the
                    // object of its named exports.
                    let whole = source.export_keys(specifier, "export=");
                    if !whole.is_empty() {
                        for key in whole {
                            bind_key(
                                ctx,
                                source,
                                key,
                                &decl.id,
                                decl.import_kind == ImportOrExportKind::Type,
                            );
                        }
                    } else if decl.import_kind != ImportOrExportKind::Type {
                        bind_namespace_object(ctx, source, specifier, &decl.id);
                    }
                }
            }
            _ => {}
        }
    }
}

fn module_export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(n) => n.name.to_string(),
        ModuleExportName::IdentifierReference(n) => n.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

// TS2307, for a relative specifier that no file of the project answers.
fn report_missing_module<'ast>(
    source: &dyn ImportSource,
    specifier: &str,
    span: Span,
    ctx: &mut CheckContext<'ast, '_>,
) {
    if specifier.starts_with('.') && source.module_exports(specifier) == ModuleExports::Unresolved {
        ctx.error(messages::module_not_found(specifier), span);
    }
}

// TS2305 (TS1192 for `default`), for a name a complete module does not export.
fn report_missing_name<'ast>(
    source: &dyn ImportSource,
    specifier: &str,
    name: &str,
    span: Span,
    ctx: &mut CheckContext<'ast, '_>,
) {
    let ModuleExports::Known { complete: true } = source.module_exports(specifier) else {
        return;
    };
    if !source.export_keys(specifier, name).is_empty() {
        return;
    }
    let message = if name == "default" {
        messages::no_default_export(specifier)
    } else {
        messages::no_exported_member(specifier, name)
    };
    ctx.error(message, span);
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
                report_missing_name(source, specifier, imported, spec.imported.span(), ctx);
                bind_name(ctx, source, specifier, imported, &spec.local, type_only);
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                report_missing_name(source, specifier, "default", spec.local.span, ctx);
                // A module with no default that says `export = x` is what a default
                // import gets (the interop every bundler and `esModuleInterop` give).
                let imported = if source.export_keys(specifier, "default").is_empty()
                    && !source.export_keys(specifier, "export=").is_empty()
                {
                    "export="
                } else {
                    "default"
                };
                bind_name(
                    ctx,
                    source,
                    specifier,
                    imported,
                    &spec.local,
                    whole_declaration_is_type,
                );
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                if !whole_declaration_is_type {
                    bind_namespace_object(ctx, source, specifier, &spec.local);
                }
            }
        }
    }
}

// `import * as ns`: `ns` is an object whose properties are the module's value exports (a
// function, a variable, an enum, a class constructor). A type the module exports is not a
// property, and `ns.Name` in a type position is not read yet, so that annotation stays
// unresolved and says nothing.
//
// Problem: a property missing from the object would be reported as not existing, so the
// object is only built from a module whose export list is complete. Against a partial one
// `ns` stays unbound, which loses an error and never invents one.
fn bind_namespace_object<'ast>(
    ctx: &mut CheckContext<'ast, '_>,
    source: &dyn ImportSource,
    specifier: &str,
    local: &oxc_ast::ast::BindingIdentifier<'_>,
) {
    if source.module_exports(specifier) != (ModuleExports::Known { complete: true }) {
        return;
    }
    let Some(symbol_id) = local.symbol_id.get() else {
        return;
    };
    let provider: &dyn DeclProvider = source;
    let mut properties: Vec<PropertyEntry> = Vec::new();
    for (name, key) in source.export_all(specifier) {
        let Some(decl) = provider.decl(key) else {
            continue;
        };
        let type_id = match decl.kind {
            DeclKind::Function | DeclKind::Var => {
                ctx.importer.import_decl(&mut ctx.arena, provider, key)
            }
            DeclKind::Enum => ctx
                .importer
                .import_enum(&mut ctx.arena, provider, key)
                .map(|(_, value)| value),
            _ => None,
        };
        let Some(type_id) = type_id else {
            continue;
        };
        if properties.iter().any(|p| &*p.name == name.as_str()) {
            continue;
        }
        properties.push(PropertyEntry {
            name: name.into(),
            type_id,
            optional: false,
            is_method: false,
        });
    }
    let object = ctx.arena.alloc(Type::Object(ObjectType::new(properties)));
    ctx.symbols.declare(symbol_id, object);
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
        // An enum is a type and a value under one name; `import type` brings in the type.
        DeclKind::Enum => {
            let Some((type_position, value_type)) =
                ctx.importer.import_enum(&mut ctx.arena, provider, key)
            else {
                return;
            };
            ctx.namespace
                .insert_imported(&local.name, type_position, Vec::new());
            if !type_only {
                if let Some(symbol_id) = local.symbol_id.get() {
                    ctx.symbols.declare(symbol_id, value_type);
                }
            }
        }
        _ => {}
    }
}
