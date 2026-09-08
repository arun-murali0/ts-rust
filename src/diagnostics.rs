use crate::line_index::LineIndex;

#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub file_name: String,

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
    pub fn format_with_position(&self, line_index: &LineIndex, source: &str) -> String {
        let (line, col) = line_index.line_col_utf8(self.start, source);
        format!(
            "{:?}: {} ({}:{}:{})",
            self.severity, self.message, self.file_name, line, col
        )
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}: {} ({}:{}-{})",
            self.severity, self.message, self.file_name, self.start, self.end
        )
    }
}
