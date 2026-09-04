//! Public diagnostic representation, decoupled from Oxc's own diagnostic
//! types so the API stays stable if the parser dependency ever changes.

#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub file_name: String,
    /// Byte offsets into the source, matching Oxc's span representation.
    pub start: u32,
    pub end: u32,
}

#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
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
