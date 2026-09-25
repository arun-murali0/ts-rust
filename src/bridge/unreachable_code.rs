use oxc_ast::ast::{Program, Statement};
use oxc_cfg::ControlFlowGraph;
use oxc_semantic::Semantic;
use oxc_span::GetSpan;

use super::context::CheckContext;

// A statement whose own basic block the CFG marks unreachable can never run --
// most commonly, code placed after an unconditional `return` or `throw`.
// Reported once per contiguous dead region, at its first statement, rather than
// once per statement inside it: everything after that first statement shares the
// same dead block, since nothing there can branch, so descending further would
// only repeat the same finding.
//
// Deliberately narrower than tsc's TS7027, in ways left as gaps rather than
// approximated:
// - Only walks the statement kinds check_statement itself understands (see
//   statements/mod.rs): a class method's body, and the body of a try, labeled,
//   do-while, for-in or for-of statement, are not descended into, since those
//   are not modelled as a plain statement list here the way a block or a
//   function body is.
// - tsc does not flag an unreachable *function declaration* on its own --
//   hoisting means the declaration itself is never "dead" even when control
//   never reaches that point, only what runs after it can be. Not special-cased
//   here, so a function declaration placed after a `return` is reported like any
//   other statement.
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
    for stmt in &program.body {
        walk_statement(stmt, semantic, cfg, ctx);
    }
}

fn walk_statement(
    stmt: &Statement,
    semantic: &Semantic,
    cfg: &ControlFlowGraph,
    ctx: &mut CheckContext<'_, '_>,
) {
    // Asking the graph rather than re-deriving reachability from the AST here is
    // the whole reason this pass exists: is_unreachable already accounts for
    // every way the builder can prove a block dead (an unconditional return or
    // throw before it, a condition the parser can see is always false, ...),
    // so nothing here has to special-case any of those individually.
    let block = semantic.nodes().cfg_id(stmt.node_id());
    if cfg.basic_block(block).is_unreachable() {
        ctx.error(crate::diagnostic_messages::messages::unreachable_code(), stmt.span());
        // Everything nested inside a dead statement shares its dead block, since
        // nothing inside it can branch out to somewhere still live. Recursing
        // into it would only find the same deadness again, once per statement,
        // and turn one dead region into a wall of duplicate diagnostics.
        return;
    }

    // Reached only for a statement the graph says is live, so from here on this
    // is purely about finding the *nested* statement lists worth asking the same
    // question of -- an expression statement or a return has no such lists, so
    // they fall through to the catch-all with nothing left to check.
    match stmt {
        Statement::BlockStatement(block_stmt) => {
            for inner in &block_stmt.body {
                walk_statement(inner, semantic, cfg, ctx);
            }
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
                for inner in &case.consequent {
                    walk_statement(inner, semantic, cfg, ctx);
                }
            }
        }
        Statement::FunctionDeclaration(func) => {
            // A function only ever fails to have a body for an ambient
            // declaration (`declare function f(): void;`), which has nothing to
            // walk into.
            let Some(body) = &func.body else { return };
            for inner in &body.statements {
                walk_statement(inner, semantic, cfg, ctx);
            }
        }
        // Every other kind either has no nested statement list of its own
        // (ExpressionStatement, ReturnStatement, ...) or is one of the gaps
        // named in this module's doc comment (ClassDeclaration, TryStatement,
        // ...); either way, is_unreachable already caught it above if it needs
        // catching at all, so there is nothing further to walk into here.
        _ => {}
    }
}
