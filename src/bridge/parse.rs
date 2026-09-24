use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::Parser;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::SourceType;

use crate::error::CheckerError;

#[tracing::instrument(skip(allocator, source), fields(file_name, source_len = source.len()))]
pub fn parse<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    file_name: &str,
) -> Result<Program<'a>, CheckerError> {
    // A recognized extension (.ts, .tsx, and so on) picks the right dialect
    // automatically. An unrecognized or missing extension, such as a file name
    // used only for a diagnostic label in a test, falls back to TypeScript rather
    // than plain JavaScript, since this checker has no reason to be given a name
    // it cannot map unless the source is meant to include TypeScript syntax.
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

// Semantic analysis with control flow graph construction switched on. oxc leaves
// the graph off by default, since most consumers never read it and building it
// costs time and memory, and leaving with_cfg(true) out would not fail: cfg() would
// just return None and flow analysis would find nothing to work with. The whole
// Semantic is returned, not only its Scoping, because the graph and the node table
// that maps an AST node to its basic block both live on it; symbol and reference
// lookups keep working through Semantic::scoping().
pub fn analyze<'a>(program: &'a Program<'a>) -> Semantic<'a> {
    let semantic = SemanticBuilder::new()
        .with_cfg(true)
        .build(program)
        .semantic;

    // Logged so a missing or empty graph is visible under RUST_LOG=debug. Nothing
    // consumes the graph yet, so without this a construction problem would only
    // surface later, as narrowing that quietly does not happen.
    if let Some(cfg) = semantic.cfg() {
        tracing::debug!(
            blocks = cfg.basic_blocks.len(),
            edges = cfg.graph.edge_count(),
            "control flow graph built"
        );
    }

    semantic
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins with_cfg(true). oxc's default is no graph at all, and cfg() then returns
    // None rather than failing, so dropping the flag would leave every later flow
    // query with nothing to read and no error to say why. An `if` with an early
    // return cannot fit in one basic block, so a single-block graph would mean the
    // builder ran but recorded no control flow.
    #[test]
    fn analyze_builds_a_control_flow_graph() {
        let allocator = Allocator::default();
        let source = "function f(x: number) { if (x > 0) { return 1; } return 2; }";
        let program = parse(&allocator, source, "cfg.ts").expect("source should parse");

        let semantic = analyze(&program);
        let cfg = semantic
            .cfg()
            .expect("with_cfg(true) and the `cfg` feature must be on");

        assert!(
            cfg.basic_blocks.len() > 1,
            "expected several basic blocks, got {}",
            cfg.basic_blocks.len()
        );
    }
}
