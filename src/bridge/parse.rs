//! Oxc parsing and semantic analysis entry points. The only file that
//! touches `oxc_parser`/`oxc_semantic` directly.

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::SourceType;

use crate::error::CheckerError;

#[tracing::instrument(skip(allocator, source), fields(file_name, source_len = source.len()))]
pub fn parse<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    file_name: &str,
) -> Result<Program<'a>, CheckerError> {
    // Fall back to plain TypeScript when the file name has no extension
    // Oxc recognizes (e.g. an in-memory buffer with a synthetic name).
    let source_type = SourceType::from_path(file_name)
        .unwrap_or_else(|_| SourceType::default().with_typescript(true));

    let result = Parser::new(allocator, source, source_type).parse();

    if result.diagnostics.has_errors() {
        let messages: Vec<String> = result
            .diagnostics
            .errors()
            .map(ToString::to_string)
            .collect();
        tracing::warn!(error_count = messages.len(), "parse completed with errors");
        return Err(CheckerError::Parse(messages.join("; ")));
    }

    tracing::debug!(
        statement_count = result.program.body.len(),
        "parsed successfully"
    );
    Ok(result.program)
}

/// Binds every declaration and reference in `program`, writing the results
/// into `symbol_id`/`reference_id` cells on the AST nodes themselves. This
/// mutates the tree in place through interior mutability. Callers don't
/// need `&mut program` for it to take effect, and can read `symbol_id`
/// straight off a `BindingIdentifier` or `reference_id` off an
/// `IdentifierReference` afterward.
///
/// This is what replaced the hand-rolled scope chain from V1: `oxc_semantic`
/// already resolves `var` hoisting, the temporal dead zone, and per-iteration
/// `let` bindings correctly, none of which a parent-pointer `HashMap` did.
pub fn analyze<'a>(program: &'a Program<'a>) -> Scoping {
    SemanticBuilder::new().build(program).semantic.into_scoping()
}
