use oxc_span::Span;

use crate::arena::{TypeArena, TypeId};
use crate::diagnostic_messages::DiagnosticMessage;
use crate::diagnostics::{Diagnostic, Severity};
use crate::namespace::TypeNamespace;
use crate::semantic::SemanticQueries;
use crate::semantic::queries::RelationCache;
use crate::symbol_map::SymbolTypeMap;
use crate::types::FileId;

use super::narrow::{JumpFrame, NarrowState, WriteMap};

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

    // Memoizes is_subtype, is_assignable and is_disjoint answers by relation and the
    // exact TypeId pair asked about, for the lifetime of this one file's check. The
    // pair is ordered, never symmetrized for subtyping: subtyping is not symmetric
    // (see subtyping.rs), so (a, b) and (b, a) are independent entries.
    //
    // The cache is tied to the arena's generation (see RelationCache). Resolving a
    // declaration's Ref with TypeArena::resolve_ref changes what an existing id means,
    // and the next query drops every answer from before it, so nothing stale can be
    // read back.
    //
    // This only catches repeats of the *same* TypeId pair. alloc() reuses one TypeId
    // for identical anonymous composites (so the same-shaped object literal at two
    // call sites shares an entry here), but a named type, a union or a
    // declaration's Ref has its own id and its own entries even when it
    // matches another by shape. What it does catch: the same subterm pair reached from
    // different branches of one recursive object or union comparison, and the
    // (candidate, existing) pairs generic inference compares, so a call like
    // `allSame(1, 2, ..., 8)` computes each pair once.
    //
    // type_annotation::check_type_argument_constraint does not use it, because that
    // function receives only the namespace and the arena, not this context. The
    // generation check would make a cached answer safe there too; it stays uncached
    // because threading the cache through a resolution-time call has not been needed.
    pub relation_cache: RelationCache,

    pub current_return_type: Option<TypeId>,
    // Problem: a function with no return annotation has no declared type to check its
    // `return` statements against, and none to give its callers.
    // Picked: while such a function's body is walked, every returned type lands here,
    // and enter_return_scope / leave_return_scope keep it per function, so a nested
    // function, arrow or method never adds its returns to the enclosing one.
    // Cost: a caller checked before the body is walked sees the placeholder `any`.
    pub inferred_returns: Option<Vec<TypeId>>,

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

    // The type of each branch of every conditional expression checked so far, by the
    // expression's start offset. tsc reports a ternary that does not fit its target
    // once per branch that does not fit; the branches' types cannot be recovered from
    // the union the whole expression has, and inferring them again would report their
    // own errors twice, so they are kept when they are first inferred.
    pub conditional_arms: std::collections::HashMap<u32, (TypeId, TypeId)>,

    // The `break` and `continue` targets the walker is currently inside, innermost last.
    // A jump records the narrowing in force where it happens into its target's frame, so
    // the code after a loop (and the head of its next pass) can join those states in
    // instead of only seeing the state at the end of the body. See control_flow.rs.
    pub jump_frames: Vec<JumpFrame>,

    // The label of a `label: while (...)` waiting for its loop, so a `continue label` can
    // find the loop. Set by the labeled statement and consumed by the loop it labels.
    pub pending_loop_label: Option<String>,

    // Where each variable is written, collected once per file by check_top_level. Empty
    // until then, which makes every loop and closure behave as if nothing were assigned.
    pub writes: WriteMap,
}

// What a function body replaces on entry and gives back on exit, so the two pieces
// of return bookkeeping can never be restored out of step with each other.
pub struct ReturnScope {
    outer_type: Option<TypeId>,
    outer_inferred: Option<Vec<TypeId>>,
}

impl<'ast, 'src> CheckContext<'ast, 'src> {
    // `infer` is true only for a function declaration with no return annotation, the one
    // place that collects what its body returns. Every other function leaves the
    // collection off, which also hides the enclosing function's list from its body.
    pub fn enter_return_scope(&mut self, return_type: Option<TypeId>, infer: bool) -> ReturnScope {
        ReturnScope {
            outer_type: std::mem::replace(&mut self.current_return_type, return_type),
            outer_inferred: std::mem::replace(&mut self.inferred_returns, infer.then(Vec::new)),
        }
    }

    // Restores the enclosing function's state and hands back the types this function
    // returned (empty unless it was entered with `infer`).
    pub fn leave_return_scope(&mut self, scope: ReturnScope) -> Vec<TypeId> {
        self.current_return_type = scope.outer_type;
        std::mem::replace(&mut self.inferred_returns, scope.outer_inferred).unwrap_or_default()
    }

    // Takes an arena the caller already owns, so a session can hand the same
    // allocation to each check in turn. The arena must already be cleared: this does
    // not reset it, because the caller is the one who knows whether it is reusing one.
    pub fn with_arena_and_file_id(file_name: &'src str, file_id: FileId, arena: TypeArena) -> Self {
        Self {
            arena,
            namespace: TypeNamespace::with_file_id(file_id),
            symbols: SymbolTypeMap::new(),
            diagnostics: Vec::new(),
            file_name,
            narrow: NarrowState::new(),
            relation_cache: RelationCache::default(),
            current_return_type: None,
            inferred_returns: None,
            current_class_instance: None,
            implicit_this: false,
            next_function_has_no_this: false,
            conditional_arms: std::collections::HashMap::new(),
            jump_frames: Vec::new(),
            pending_loop_label: None,
            writes: WriteMap::default(),
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
    //
    // Takes &mut self (not &self) because SemanticQueries carries a mutable
    // handle to relation_cache alongside the arena. Field-level destructuring
    // here borrows the two fields disjointly, but that disjointness is only
    // visible inside this function body -- past this call boundary the returned
    // SemanticQueries opaquely holds part of `self`, so callers cannot access
    // ctx.arena or ctx.relation_cache directly while a SemanticQueries from this
    // call is still alive. In practice this means any TypeId needed as an
    // argument (e.g. ctx.arena.number()) must be read into a local *before*
    // calling ctx.semantic(), not inline as part of the same expression.
    pub fn semantic(&mut self) -> SemanticQueries<'_> {
        SemanticQueries::new(&self.arena, &mut self.relation_cache)
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
