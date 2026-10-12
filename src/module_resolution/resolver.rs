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
        let found = self
            .resolver
            .resolve_file(from, specifier)
            .map(|resolution| resolution.path().to_path_buf());
        self.with_types_fallback(from, specifier, found)
    }

    /// Like `resolve_from_file`, but looks for declaration files, which is what an
    /// `import type` needs: a package that ships only types has no JavaScript entry for
    /// the plain resolver to find.
    pub fn resolve_dts_from_file(
        &self,
        from: &Path,
        specifier: &str,
    ) -> Result<PathBuf, ModuleError> {
        let found = self
            .resolver
            .resolve_dts(from, specifier)
            .map(|resolution| resolution.path().to_path_buf());
        self.with_types_fallback(from, specifier, found)
    }

    // A package that ships JavaScript and no declarations of its own is typed by
    // `@types/<name>`, which is where TypeScript looks next. So a package specifier that
    // resolved to a JavaScript file, or to nothing, is tried as its `@types` package, and
    // that wins when it exists. A relative specifier never is.
    fn with_types_fallback(
        &self,
        from: &Path,
        specifier: &str,
        found: Result<PathBuf, oxc_resolver::ResolveError>,
    ) -> Result<PathBuf, ModuleError> {
        let wants_types = match &found {
            Ok(path) => is_javascript(path),
            Err(_) => true,
        };
        if wants_types {
            if let Some(types) = types_specifier(specifier) {
                if let Ok(resolution) = self.resolver.resolve_dts(from, &types) {
                    return Ok(resolution.path().to_path_buf());
                }
            }
        }
        found.map_err(|error| unresolved(from, specifier, &error))
    }
}

fn is_javascript(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js" | "mjs" | "cjs" | "jsx")
    )
}

// `lodash/fp` is `@types/lodash/fp`; `@scope/name` is `@types/scope__name`. None for a
// relative or absolute specifier, which no `@types` package stands for.
fn types_specifier(specifier: &str) -> Option<String> {
    if specifier.starts_with('.') || specifier.starts_with('/') || specifier.starts_with("@types/")
    {
        return None;
    }
    let (name, rest) = match specifier.strip_prefix('@') {
        Some(scoped) => {
            let (scope, tail) = scoped.split_once('/')?;
            let (name, rest) = match tail.split_once('/') {
                Some((name, rest)) => (name, format!("/{rest}")),
                None => (tail, String::new()),
            };
            (format!("{scope}__{name}"), rest)
        }
        None => match specifier.split_once('/') {
            Some((name, rest)) => (name.to_string(), format!("/{rest}")),
            None => (specifier.to_string(), String::new()),
        },
    };
    Some(format!("@types/{name}{rest}"))
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
