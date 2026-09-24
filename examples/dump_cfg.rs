// Prints the control flow graph oxc builds for a TypeScript file: each basic block
// with its instructions, each edge with its kind, and the block every control-flow
// AST node was entered in.
//
// This is an example, not a checker feature, because its only job is to settle
// questions about oxc's graph before code depends on the answers. oxc does not
// document how it wires each construct, and what it does is not what one would
// guess: both branches of an `if` leave by `Jump` edges (the else path is not a
// `Normal` edge), and `&&`, `||` and `??` are plain `Normal` edges with the
// operator recorded nowhere. Which branch a block belongs to has to be recovered
// from the AST, so the real shape needs to be seen, not assumed.
//
//     cargo run --example dump_cfg -- examples/cfg_sample.ts

use std::{env, fs, process};

use oxc_allocator::Allocator;
use oxc_cfg::graph::visit::EdgeRef;
use oxc_parser::Parser;
use oxc_semantic::{NodeId, Semantic, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};

// The AST kinds whose block placement narrowing will depend on: branches, loops,
// exits and function boundaries. Everything else (identifiers, literals, types) is
// left out on purpose, since hundreds of those lines would bury the few that
// matter.
const FLOW_KINDS: &[&str] = &[
    "IfStatement",
    "ConditionalExpression",
    "LogicalExpression",
    "ChainExpression",
    "AssignmentExpression",
    "WhileStatement",
    "DoWhileStatement",
    "ForStatement",
    "ForInStatement",
    "ForOfStatement",
    "SwitchStatement",
    "SwitchCase",
    "TryStatement",
    "CatchClause",
    "ReturnStatement",
    "ThrowStatement",
    "BreakStatement",
    "ContinueStatement",
    "LabeledStatement",
    "BlockStatement",
    "Function",
    "ArrowFunctionExpression",
];

const SNIPPET_WIDTH: usize = 40;

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: cargo run --example dump_cfg -- <file.ts>");
        process::exit(2);
    };
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("cannot read {path}: {error}");
        process::exit(1);
    });

    let source_type = SourceType::from_path(&path)
        .unwrap_or_else(|_| SourceType::default().with_typescript(true));
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &source, source_type).parse();
    if parsed.diagnostics.has_errors() {
        for error in parsed.diagnostics.errors() {
            eprintln!("parse error: {error}");
        }
        process::exit(1);
    }

    // Both flags are needed, as in the checker's own analyze: without
    // with_build_nodes(true) the node table is empty, so every node id in the graph
    // points at nothing and describing an instruction panics.
    let semantic = SemanticBuilder::new()
        .with_cfg(true)
        .with_build_nodes(true)
        .build(&parsed.program)
        .semantic;
    let Some(cfg) = semantic.cfg() else {
        eprintln!("no control flow graph: enable oxc_semantic's `cfg` feature");
        process::exit(1);
    };

    println!("== blocks ==");
    for block_ix in cfg.graph.node_indices() {
        let block = cfg.basic_block(block_ix);
        let note = if block.is_unreachable() {
            "  (unreachable)"
        } else {
            ""
        };
        println!("block {}{note}", block_ix.index());
        for instruction in block.instructions() {
            let target = instruction
                .node_id
                .map_or_else(String::new, |id| describe(&semantic, id, &source));
            println!("    {:?} {target}", instruction.kind);
        }
    }

    // Edges are printed in insertion order because that order is the only thing that
    // tells the two `Jump` edges of an `if` apart: the builder adds the consequent's
    // before the alternate's, and nothing else on the edge says which is which.
    println!("\n== edges (in the order the builder added them) ==");
    for edge in cfg.graph.edge_references() {
        println!(
            "block {} -> block {}  {:?}",
            edge.source().index(),
            edge.target().index(),
            edge.weight()
        );
    }

    println!("\n== control-flow nodes and the block each was entered in ==");
    for node in semantic.nodes().iter() {
        // debug_name borrows from the kind, so the kind gets its own binding; a
        // temporary would be dropped while the name is still in use.
        let kind = node.kind();
        let name = kind.debug_name();
        // debug_name appends a payload to some kinds (`LabeledStatement(outer)`,
        // `CallExpression(f)`), so kinds are matched on the part before the
        // parenthesis rather than on the whole name.
        let base = name.split('(').next().unwrap_or(&name);
        if FLOW_KINDS.contains(&base) {
            let block = semantic.nodes().cfg_id(node.id());
            println!(
                "block {:<3} {name}  `{}`",
                block.index(),
                snippet(&source, node.span())
            );
        }
    }
}

fn describe(semantic: &Semantic<'_>, node_id: NodeId, source: &str) -> String {
    let node = semantic.nodes().get_node(node_id);
    format!(
        "{} `{}`",
        node.kind().debug_name(),
        snippet(source, node.span())
    )
}

// The first line of a node's source text, cut to SNIPPET_WIDTH: enough to match a
// block or an instruction back to the code it came from, short enough to keep one
// line per entry so a whole file's dump stays readable.
fn snippet(source: &str, span: Span) -> String {
    let text = source
        .get(span.start as usize..span.end as usize)
        .unwrap_or("");
    let first_line = text.lines().next().unwrap_or("");
    let mut shown: String = first_line.chars().take(SNIPPET_WIDTH).collect();
    if first_line.chars().count() > SNIPPET_WIDTH || text.lines().count() > 1 {
        shown.push('…');
    }
    shown
}
