//! Bundles the state threaded through every statement/expression walker.
//! Without this, adding one more thing to check (like narrowing) means
//! touching every function signature in `bridge/`. `scoping` stays a
//! separate parameter since it comes from `oxc_semantic`, a different
//! layer, and never changes once bound at the top of `check_program`.

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
    /// Narrowed types currently in effect. Restored to whatever it was
    /// before entering an `if` branch once that branch finishes checking —
    /// see `bridge/statements.rs`'s `IfStatement` handling.
    pub narrow: NarrowState,
    /// The return type of the function body currently being checked, so a
    /// `return` statement inside a nested `if`/block can still be checked
    /// against it without threading it through every function signature.
    pub current_return_type: Option<TypeId>,
    /// The instance type of the class whose constructor or method body is
    /// currently being checked, so `this` resolves to it without
    /// threading it through every signature. See
    /// `bridge/statements.rs`'s `ClassDeclaration` handling and
    /// `bridge/expressions.rs`'s `Expression::ThisExpression` arm.
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
