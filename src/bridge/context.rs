use oxc_span::Span;

use crate::arena::{TypeArena, TypeId};
use crate::diagnostic_messages::DiagnosticMessage;
use crate::diagnostics::{Diagnostic, Severity};
use crate::namespace::TypeNamespace;
use crate::semantic::SemanticQueries;
use crate::symbol_map::SymbolTypeMap;

use super::narrow::NarrowState;

// The shared mutable state for checking one program. Every feature module
// (expressions, statements, narrowing) takes &mut CheckContext rather than owning
// its own state, by design: this keeps a single-threaded check simple today while
// making the eventual unit of parallelism explicit later, one independent
// CheckContext per file, rather than fine-grained locking inside one file's check.
// See docs/architecture.md for the fuller rationale.
pub struct CheckContext<'ast, 'src> {
    pub arena: TypeArena,
    pub namespace: TypeNamespace<'ast>,
    pub symbols: SymbolTypeMap,
    pub diagnostics: Vec<Diagnostic>,
    pub file_name: &'src str,

    pub narrow: NarrowState,

    pub current_return_type: Option<TypeId>,

    pub current_class_instance: Option<TypeId>,

    // True while checking the body of a function whose `this` has no type at all:
    // a plain function expression with no `this` parameter and no contextual
    // type. A `this` used there is an implicit any, which tsc reports under
    // noImplicitThis. Left false everywhere else, including object literal
    // methods (whose `this` is the literal) and any function this checker cannot
    // yet prove has no contextual `this`, so those stay silent.
    pub implicit_this: bool,

    // A one-shot request from the caller of infer_function_expression_type: the
    // very next function expression checked is known to have no contextual
    // `this`, so its body should run with implicit_this set. Consumed on entry.
    pub next_function_has_no_this: bool,
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
            implicit_this: false,
            next_function_has_no_this: false,
        }
    }

    pub fn error(&mut self, message: DiagnosticMessage, span: Span) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: message.code,
            message: message.text,
            file_name: self.file_name.to_string(),
            start: span.start,
            end: span.end,
        });
    }

    // The seam between AST-facing bridge code and the semantic core. Bridge code
    // calls ctx.semantic().is_assignable(...) instead of reaching into subtyping
    // directly, so TypeScript-specific assignability rules that are not plain
    // structural subtyping, such as excess property checks on object literals or
    // const assertions, have one place to live later without changing call sites.
    pub fn semantic(&self) -> SemanticQueries<'_> {
        SemanticQueries::new(&self.arena)
    }

    pub fn warning(&mut self, message: DiagnosticMessage, span: Span) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: message.code,
            message: message.text,
            file_name: self.file_name.to_string(),
            start: span.start,
            end: span.end,
        });
    }
}
