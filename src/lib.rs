mod arena;
mod bridge;
mod diagnostics;
mod error;
mod fxhash;
mod line_index;
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

pub use line_index::LineIndex;

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

    #[tracing::instrument(skip(self, source), fields(file_name = file_name))]
    pub fn check_source(&self, source: &str, file_name: &str) -> Result<CheckResult, CheckerError> {
        let diagnostics = bridge::check_program(source, file_name)?;
        Ok(CheckResult { diagnostics })
    }
}
