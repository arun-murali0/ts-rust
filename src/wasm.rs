//! JS/npm-facing surface. Only compiled with `--features wasm`.
//!
//! Kept separate from `lib.rs` so the native library stays free of
//! wasm-bindgen/serde dependencies entirely when the feature is off.

use wasm_bindgen::prelude::*;

use crate::line_index::LineIndex;
use crate::{diagnostics::Diagnostic, TypeChecker as CoreChecker};

/// What actually crosses the JS boundary. `Diagnostic` itself stays
/// byte-offset-only (see `diagnostics.rs`) — this wrapper is where the
/// UTF-16 conversion happens, specifically because JS strings are
/// UTF-16 and the LSP spec defaults to UTF-16 code-unit positions. A JS
/// consumer indexing into its own source string, or an editor placing a
/// cursor, needs this — a raw byte offset or a codepoint count would
/// both silently misplace positions on any line containing an
/// astral-plane character (many emoji, some CJK extension blocks).
#[cfg_attr(feature = "wasm", derive(serde::Serialize))]
pub struct WasmDiagnostic {
    severity: crate::diagnostics::Severity,
    message: String,
    file_name: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

impl WasmDiagnostic {
    fn from_diagnostic(diagnostic: &Diagnostic, line_index: &LineIndex, source: &str) -> Self {
        let (start_line, start_column) = line_index.line_col_utf16(diagnostic.start, source);
        let (end_line, end_column) = line_index.line_col_utf16(diagnostic.end, source);
        Self {
            severity: diagnostic.severity,
            message: diagnostic.message.clone(),
            file_name: diagnostic.file_name.clone(),
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

/// Call once from JS before using the checker, to get readable panic
/// messages in the browser/Node console instead of an opaque wasm trap.
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct TsRustChecker {
    inner: CoreChecker,
}

#[wasm_bindgen]
impl TsRustChecker {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: CoreChecker::new(),
        }
    }

    /// Check `source` and return diagnostics as a plain JS array of objects
    /// (via serde-wasm-bindgen), rather than exposing our Rust enum types
    /// directly across the boundary. Positions are UTF-16 line/column
    /// pairs (`start_line`, `start_column`, `end_line`, `end_column`),
    /// not raw byte offsets — see `WasmDiagnostic`'s doc comment for why
    /// that conversion has to happen here rather than in `Diagnostic`
    /// itself.
    #[wasm_bindgen(js_name = checkSource)]
    pub fn check_source(&self, source: &str, file_name: &str) -> Result<JsValue, JsValue> {
        let result = self
            .inner
            .check_source(source, file_name)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let line_index = LineIndex::new(source);
        let wasm_diagnostics: Vec<WasmDiagnostic> = result
            .diagnostics
            .iter()
            .map(|diagnostic| WasmDiagnostic::from_diagnostic(diagnostic, &line_index, source))
            .collect();

        serde_wasm_bindgen::to_value(&wasm_diagnostics).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

impl Default for TsRustChecker {
    fn default() -> Self {
        Self::new()
    }
}

// Re-exported so the JS type declarations (via wasm-bindgen's generated
// .d.ts) have a name to point at. Points at `WasmDiagnostic`, not
// `Diagnostic` — `WasmDiagnostic` (UTF-16 line/column) is what actually
// gets serialized across the boundary by `check_source` above.
pub type DiagnosticList = Vec<WasmDiagnostic>;
