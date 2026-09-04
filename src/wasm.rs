//! JS/npm-facing surface. Only compiled with `--features wasm`.
//!
//! Kept separate from `lib.rs` so the native library stays free of
//! wasm-bindgen/serde dependencies entirely when the feature is off.

use wasm_bindgen::prelude::*;

use crate::{diagnostics::Diagnostic, TypeChecker as CoreChecker};

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
    /// directly across the boundary.
    #[wasm_bindgen(js_name = checkSource)]
    pub fn check_source(&self, source: &str, file_name: &str) -> Result<JsValue, JsValue> {
        let result = self
            .inner
            .check_source(source, file_name)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        serde_wasm_bindgen::to_value(&result.diagnostics)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

impl Default for TsRustChecker {
    fn default() -> Self {
        Self::new()
    }
}

// Re-exported so the JS type declarations (via wasm-bindgen's generated
// .d.ts) have a name to point at, once Diagnostic derives Serialize.
pub type DiagnosticList = Vec<Diagnostic>;
