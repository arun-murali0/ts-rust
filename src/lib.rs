//! `ts-rust`: a from-scratch TypeScript type checker built on Oxc's
//! parser/AST. See `docs/DESIGN.md` for architecture and `docs/ROADMAP.md`
//! for what's implemented at each version.
//!
//! # Current scope (V2)
//!
//! - primitive types (`number`, `string`, `boolean`, `null`, `undefined`,
//!   `any`, `unknown`) on variable declarations, function parameters, and
//!   return types
//! - object types, with width subtyping and optional properties
//! - array types (`T[]`), covariant in the element type
//! - union types, checked member-by-member
//! - `type` aliases and `interface` declarations, including forward
//!   references between them
//! - arithmetic and comparison binary operators
//! - function call arity and argument types
//!
//! It does **not** yet check `if`/`for`/`while`/block statements, classes,
//! narrowing, generics, or imports. Recursive aliases and interfaces
//! (`type Tree = { children: Tree[] }`) also aren't supported yet, they're
//! reported as `Unsupported` rather than causing an infinite loop.
//! Encountering any of this produces a `Warning`-severity diagnostic naming
//! the construct rather than panicking or silently passing. See
//! `docs/ROADMAP.md` for what later versions add.
//!
//! ```
//! use ts_rust::TypeChecker;
//!
//! let checker = TypeChecker::new();
//! let result = checker.check_source("const x: number = 'oops';", "input.ts").unwrap();
//! assert_eq!(result.diagnostics.len(), 1);
//! ```

mod arena;
mod bridge;
mod diagnostics;
mod error;
mod fxhash;
mod namespace;
mod subtyping;
mod symbol_map;
mod type_annotation;
mod types;

#[cfg(feature = "wasm")]
mod wasm;
#[cfg(feature = "wasm")]
pub use wasm::TsRustChecker;

pub use diagnostics::{Diagnostic, Severity};
pub use error::CheckerError;

// Not part of the stable public API; exists only so
// benches/checker_benchmark.rs, which compiles as a separate crate, can
// call it to isolate parse+bind cost from declare/check cost. `doc(hidden)`
// keeps it out of generated docs without making it any more public than
// it needs to be.
#[doc(hidden)]
pub use bridge::parse_and_bind_only;

#[derive(Default)]
pub struct TypeChecker {}

#[derive(Debug)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses `source` and runs the two-pass checking pipeline (declare,
    /// then check; see `docs/DESIGN.md` section 1.2). Returns `Err` only
    /// on parse failure; a successfully parsed file always produces a
    /// `CheckResult`, even one full of diagnostics.
    #[tracing::instrument(skip(self, source), fields(file_name = file_name))]
    pub fn check_source(&self, source: &str, file_name: &str) -> Result<CheckResult, CheckerError> {
        let diagnostics = bridge::check_program(source, file_name)?;
        Ok(CheckResult { diagnostics })
    }
}
