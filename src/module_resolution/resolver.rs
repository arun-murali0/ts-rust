use std::path::{Path, PathBuf};

use oxc_resolver::{ResolveOptions, Resolver};

use super::error::ModuleError;

// oxc_resolver with the options a TypeScript project needs, and its error type turned
// into ModuleError so the dependency does not appear in the public API.
//
// The resolver caches directory listings and package.json reads, so one instance is
// built per project and shared by reference, including across the threads that
// check_project uses.
pub struct ModuleResolver {
    resolver: Resolver,
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self {
            resolver: Resolver::new(options()),
        }
    }

    /// Resolves `specifier` as written in `from`, the importing file.
    pub fn resolve_from_file(&self, from: &Path, specifier: &str) -> Result<PathBuf, ModuleError> {
        self.resolver
            .resolve_file(from, specifier)
            .map(|resolution| resolution.path().to_path_buf())
            .map_err(|error| unresolved(from, specifier, &error))
    }

    /// Like `resolve_from_file`, but looks for declaration files, which is what an
    /// `import type` needs: a package that ships only types has no JavaScript entry for
    /// the plain resolver to find.
    pub fn resolve_dts_from_file(
        &self,
        from: &Path,
        specifier: &str,
    ) -> Result<PathBuf, ModuleError> {
        self.resolver
            .resolve_dts(from, specifier)
            .map(|resolution| resolution.path().to_path_buf())
            .map_err(|error| unresolved(from, specifier, &error))
    }
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn unresolved(from: &Path, specifier: &str, error: &oxc_resolver::ResolveError) -> ModuleError {
    ModuleError::Unresolved {
        specifier: specifier.to_owned(),
        from: from.to_path_buf(),
        reason: error.to_string(),
    }
}

// `types` comes first among the conditions and main fields so a package's declaration
// entry wins over its JavaScript one. The extension aliases are TypeScript's NodeNext
// rule: `./a.js` in source means `./a.ts` on disk, because the emitted file is the one
// that gets named.
fn options() -> ResolveOptions {
    let strings =
        |items: &[&str]| -> Vec<String> { items.iter().map(|item| (*item).to_owned()).collect() };
    ResolveOptions {
        extensions: strings(&[".ts", ".tsx", ".mts", ".cts", ".d.ts", ".js", ".jsx"]),
        extension_alias: vec![
            (".js".to_owned(), strings(&[".ts", ".tsx", ".d.ts", ".js"])),
            (".jsx".to_owned(), strings(&[".tsx", ".jsx"])),
            (".mjs".to_owned(), strings(&[".mts", ".d.mts", ".mjs"])),
            (".cjs".to_owned(), strings(&[".cts", ".d.cts", ".cjs"])),
        ],
        condition_names: strings(&["types", "import", "node"]),
        main_fields: strings(&["types", "typings", "module", "main"]),
        ..ResolveOptions::default()
    }
}
