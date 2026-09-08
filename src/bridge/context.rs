use oxc_span::Span;

use crate::arena::{TypeArena, TypeId};
use crate::diagnostics::{Diagnostic, Severity};
use crate::namespace::TypeNamespace;
use crate::symbol_map::SymbolTypeMap;

use super::narrow::NarrowState;

pub struct CheckContext<'ast, 'src> {
    pub arena: TypeArena,
    pub namespace: TypeNamespace<'ast>,
    pub symbols: SymbolTypeMap,
    pub diagnostics: Vec<Diagnostic>,
    pub file_name: &'src str,

    pub narrow: NarrowState,

    pub current_return_type: Option<TypeId>,

    pub current_class_instance: Option<TypeId>,
}

impl<'ast, 'src> CheckContext<'ast, 'src> {
    pub fn new(file_name: &'src str) -> Self {
        Self {
            arena: TypeArena::new(),
            namespace: TypeNamespace::new(),
            symbols: SymbolTypeMap::new(),
            diagnostics: Vec::new(),
            file_name,
            narrow: NarrowState::new(),
            current_return_type: None,
            current_class_instance: None,
        }
    }

    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            file_name: self.file_name.to_string(),
            start: span.start,
            end: span.end,
        });
    }

    pub fn warning(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            file_name: self.file_name.to_string(),
            start: span.start,
            end: span.end,
        });
    }
}
