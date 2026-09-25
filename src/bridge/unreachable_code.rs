use oxc_ast::ast::{Program, Statement};
use oxc_cfg::ControlFlowGraph;
use oxc_semantic::Semantic;
use oxc_span::GetSpan;

use super::context::CheckContext;

// A statement whose own basic block the CFG marks unreachable can never run --
// most commonly, code placed after an unconditional `return` or `throw`.
// Reported once per contiguous dead region, at its first statement, rather than
// once per statement inside it: everything listed after that first statement in
// the same sibling list shares its dead block, since nothing between them can
// make control flow resume, so walking (and reporting) the rest would only
// repeat the same finding. walk_statements owns stopping at that first one;
// walk_statement reports-or-recurses for one statement and says which it did, so
// its caller's loop knows whether to keep going.
//
// Verified against oxc's real graph, not assumed: a function declaration placed
// after an unconditional return is not flagged (see
// tests/unreachable_code.rs::an_unreachable_function_declaration_is_not_flagged).
// oxc's builder places a hoisted declaration's node in the function's entry
// block rather than in its textual position, so it never lands in a dead block
// to begin with -- this checker does nothing special to make that happen, it
// falls out of asking the graph rather than the AST.
//
// Deliberately narrower than tsc's TS7027 in one remaining way, left as a gap
// rather than approximated: only the statement kinds check_statement itself
// understands (see statements/mod.rs) are walked. A class method's body, and
// the body of a try, labeled, do-while, for-in or for-of statement, are not
// descended into, since those are not modelled as a plain statement list here
// the way a block or a function body is.
pub(crate) fn check_unreachable_code(
    program: &Program,
    semantic: &Semantic,
    ctx: &mut CheckContext<'_, '_>,
) {
    // A missing graph means silently reporting nothing rather than panicking,
    // the same choice parse::analyze's own doc comment makes for a missing node
    // table: this pass is additive (it only ever adds diagnostics), so losing it
    // should not be able to take the rest of the check down with it.
    // parse::analyze always builds the graph, so in practice this only fires if
    // that call site changes without this one noticing.
    let Some(cfg) = semantic.cfg() else {
        return;
    };
    walk_statements(&program.body, semantic, cfg, ctx);
}

// A list of statements that run one after another in the same scope: a function
// body, a block, or one switch case's statements. Stops at the first one the
// graph marks unreachable, for the reason explained on check_unreachable_code.
fn walk_statements(
    stmts: &[Statement],
    semantic: &Semantic,
    cfg: &ControlFlowGraph,
    ctx: &mut CheckContext<'_, '_>,
) {
    for stmt in stmts {
        if walk_statement(stmt, semantic, cfg, ctx) {
            break;
        }
    }
}

// Reports `stmt` and returns true if the graph marks its own block unreachable.
// Otherwise recurses into whatever nested statement list(s) it has -- the reason
// this exists at all, rather than checking is_unreachable directly in
// walk_statements' loop -- and returns false either way, since a statement that
// was itself reachable when entered still counts as reachable to its own
// siblings, even if something nested inside it goes dead partway through.
fn walk_statement(
    stmt: &Statement,
    semantic: &Semantic,
    cfg: &ControlFlowGraph,
    ctx: &mut CheckContext<'_, '_>,
) -> bool {
    // Asking the graph rather than re-deriving reachability from the AST here is
    // the whole reason this pass exists: is_unreachable already accounts for
    // every way the builder can prove a block dead (an unconditional return or
    // throw before it, a condition the parser can see is always false, ...),
    // so nothing here has to special-case any of those individually.
    let block = semantic.nodes().cfg_id(stmt.node_id());
    if cfg.basic_block(block).is_unreachable() {
        ctx.error(
            crate::diagnostic_messages::messages::unreachable_code(),
            stmt.span(),
        );
        return true;
    }

    match stmt {
        Statement::BlockStatement(block_stmt) => {
            walk_statements(&block_stmt.body, semantic, cfg, ctx);
        }
        Statement::IfStatement(if_stmt) => {
            walk_statement(&if_stmt.consequent, semantic, cfg, ctx);
            if let Some(alternate) = &if_stmt.alternate {
                walk_statement(alternate, semantic, cfg, ctx);
            }
        }
        Statement::WhileStatement(while_stmt) => {
            walk_statement(&while_stmt.body, semantic, cfg, ctx);
        }
        Statement::ForStatement(for_stmt) => {
            walk_statement(&for_stmt.body, semantic, cfg, ctx);
        }
        Statement::SwitchStatement(switch_stmt) => {
            for case in &switch_stmt.cases {
                walk_statements(&case.consequent, semantic, cfg, ctx);
            }
        }
        Statement::FunctionDeclaration(func) => {
            // A function only ever fails to have a body for an ambient
            // declaration (`declare function f(): void;`), which has nothing to
            // walk into.
            let Some(body) = &func.body else { return false };
            walk_statements(&body.statements, semantic, cfg, ctx);
        }
        // Every other kind either has no nested statement list of its own
        // (ExpressionStatement, ReturnStatement, ...) or is one of the gaps
        // named in this module's doc comment (ClassDeclaration, TryStatement,
        // ...); either way, is_unreachable already caught it above if it needs
        // catching at all, so there is nothing further to walk into here.
        _ => {}
    }
    false
}
