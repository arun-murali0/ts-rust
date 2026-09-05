//! Public diagnostic representation, decoupled from Oxc's own diagnostic
//! types so the API stays stable if the parser dependency ever changes.

use crate::line_index::LineIndex;

#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub file_name: String,
    /// Byte offsets into the source, matching Oxc's span representation.
    /// Deliberately not a line/column: converting requires a full scan
    /// of the source text (see `crate::line_index::LineIndex`), so it's
    /// done once per file at the point something actually needs to
    /// *display* a position, not stored redundantly on every
    /// diagnostic. Native callers want a codepoint column; WASM callers
    /// want a UTF-16 column — baking one in here would silently be
    /// wrong for the other consumer.
    pub start: u32,
    pub end: u32,
}

#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Diagnostic {
    /// Formats as `Severity: message (file:line:col)`, with `line`/`col`
    /// as 1-based Unicode-codepoint positions — the convention a native
    /// CLI/terminal consumer expects (matches rustc's own style).
    ///
    /// Build one `LineIndex` per file with `LineIndex::new(source)` and
    /// reuse it across every diagnostic for that file, rather than
    /// rebuilding it per call — the whole point of `LineIndex` is
    /// amortizing the line-boundary scan across every diagnostic in the
    /// file instead of repeating it.
    pub fn format_with_position(&self, line_index: &LineIndex, source: &str) -> String {
        let (line, col) = line_index.line_col_utf8(self.start, source);
        format!("{:?}: {} ({}:{}:{})", self.severity, self.message, self.file_name, line, col)
    }
}

impl std::fmt::Display for Diagnostic {
    /// Quick byte-range representation — useful for `tracing`/debug
    /// logging where a `LineIndex`/source text isn't at hand, but NOT
    /// what a user should see. Use `format_with_position` for
    /// user-facing output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}: {} ({}:{}-{})",
            self.severity, self.message, self.file_name, self.start, self.end
        )
    }
}
