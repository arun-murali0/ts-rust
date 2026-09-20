use crate::diagnostic_codes::DiagnosticCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
pub enum Severity {
    Error,
    Warning,
}
#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub file_name: String,

    // Byte offsets into the source, not line and column. Converting to line and
    // column needs the LineIndex built during parsing, which is not always
    // available at the point a diagnostic gets constructed, so the raw offsets are
    // kept here and converted lazily by whichever formatter actually needs them.
    pub start: u32,
    pub end: u32,
}

impl Diagnostic {
    pub fn format_with_position(
        &self,
        line_index: &crate::line_index::LineIndex,
        source: &str,
    ) -> String {
        let view = self.to_view(line_index, source);
        format!(
            "{}:{}:{}: {}: {} {}",
            self.file_name,
            view.range.start.line,
            view.range.start.column,
            match self.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            self.code,
            view.message
        )
    }
}
