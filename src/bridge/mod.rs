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

    declare::declare_top_level(&program, &mut ctx);
    statements::check_top_level(&program, &scoping, &mut ctx);

    tracing::info!(diagnostic_count = ctx.diagnostics.len(), "check complete");
    Ok(ctx.diagnostics)
}
