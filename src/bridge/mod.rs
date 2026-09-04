//! The only part of the crate that imports oxc_ast, oxc_parser, and
//! oxc_semantic types directly. type_annotation.rs and namespace.rs also
//! touch oxc_ast, since resolving a type annotation and lazily resolving
//! an interface both need it, but neither of those touches oxc_parser or
//! oxc_semantic. Everything in this module translates between Oxc's AST
//! and the parser-agnostic core (arena.rs, types.rs, subtyping.rs).

mod context;
mod declare;
mod expressions;
mod narrow;
mod parse;
mod statements;

use oxc_allocator::Allocator;

use crate::diagnostics::Diagnostic;
use crate::error::CheckerError;
use context::CheckContext;

pub use parse::parse;

/// Runs parse + `oxc_semantic` bind, then drops everything. Exists only so
/// `benches/checker_benchmark.rs` can isolate that cost from declare/check
/// — see `docs/VERIFICATION.md`.
pub fn parse_and_bind_only(source: &str, file_name: &str) -> Result<(), CheckerError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, source, file_name)?;
    let _scoping = parse::analyze(&program);
    Ok(())
}

#[tracing::instrument(skip_all, fields(file_name))]
pub fn check_program(source: &str, file_name: &str) -> Result<Vec<Diagnostic>, CheckerError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, source, file_name)?;
    let scoping = parse::analyze(&program);

    let mut ctx = CheckContext::new(file_name);

    // Pass 1: hoist every top-level type and value signature so later
    // code can reference something declared earlier or later in the same
    // file. Pass 2: walk statement bodies and check them against what
    // pass 1 registered. Function parameters are the one binding that
    // gets declared inside pass 2 itself, right before the body that
    // uses them; see bridge/statements.rs for why.
    declare::declare_top_level(&program, &mut ctx);
    statements::check_top_level(&program, &scoping, &mut ctx);

    tracing::info!(diagnostic_count = ctx.diagnostics.len(), "check complete");
    Ok(ctx.diagnostics)
}
