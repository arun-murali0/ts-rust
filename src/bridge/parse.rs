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

pub fn analyze<'a>(program: &'a Program<'a>) -> Scoping {
    SemanticBuilder::new()
        .build(program)
        .semantic
        .into_scoping()
}
