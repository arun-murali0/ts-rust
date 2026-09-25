use oxc_ast::ast::{ForStatement, IfStatement, SwitchStatement, WhileStatement};
use oxc_semantic::Scoping;

use super::super::context::CheckContext;
use super::super::expressions::infer_expression_type;
use super::super::narrow::narrow_condition;
use super::{check_statement, check_variable_declaration, statement_always_exits};

pub(super) fn check_if_statement(
    if_stmt: &IfStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    infer_expression_type(&if_stmt.test, scoping, ctx);

    let (true_overrides, false_overrides) = narrow_condition(&if_stmt.test, scoping, ctx);
    let outer_narrow = ctx.narrow.clone();

    ctx.narrow.extend(true_overrides);
    check_statement(&if_stmt.consequent, scoping, ctx);
    let consequent_always_exits = statement_always_exits(&if_stmt.consequent);

    match &if_stmt.alternate {
        Some(alternate) => {
            ctx.narrow = outer_narrow.clone();
            ctx.narrow.extend(false_overrides);
            check_statement(alternate, scoping, ctx);
            ctx.narrow = outer_narrow;
        }
        None => {
            ctx.narrow = outer_narrow;
            // The guard-clause pattern: `if (x === null) return; ... x.prop`. With
            // no else branch, there is nothing after the if statement that ran
            // under true_overrides, since the consequent always exits before
            // reaching it. So the false branch's narrowing (x is non-null) is
            // exactly what should carry into the code that follows.
            if consequent_always_exits {
                ctx.narrow.extend(false_overrides);
            }
        }
    }
}

pub(super) fn check_while_statement(
    while_stmt: &WhileStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    infer_expression_type(&while_stmt.test, scoping, ctx);

    // Not narrowed against the loop's own test (`while (x !== null)`), unlike an
    // `if`'s condition -- a gap, not a leak, and left for later. What is fixed
    // here is the leak: the body is only walked once, not once per iteration, so
    // whatever a guard clause inside it narrows must not survive past the loop's
    // closing brace, the same way an if-branch's narrowing does not survive past
    // its own.
    let outer_narrow = ctx.narrow.clone();
    check_statement(&while_stmt.body, scoping, ctx);
    ctx.narrow = outer_narrow;
}

pub(super) fn check_for_statement(
    for_stmt: &ForStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &for_stmt.init {
        check_variable_declaration(decl, scoping, ctx);
    }

    if let Some(test) = &for_stmt.test {
        infer_expression_type(test, scoping, ctx);
    }
    if let Some(update) = &for_stmt.update {
        infer_expression_type(update, scoping, ctx);
    }

    // Same reasoning as check_while_statement: the body is walked once, so any
    // narrowing a guard clause inside it establishes must stay inside the loop.
    let outer_narrow = ctx.narrow.clone();
    check_statement(&for_stmt.body, scoping, ctx);
    ctx.narrow = outer_narrow;
}

pub(super) fn check_switch_statement(
    switch_stmt: &SwitchStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    // Each case is checked independently with no narrowing applied from the
    // discriminant or the case's own test value; switch-based discriminated
    // union narrowing (`switch (shape.kind) { case "circle": ... }`) is not
    // implemented yet, unlike the if-statement narrowing above.
    infer_expression_type(&switch_stmt.discriminant, scoping, ctx);

    // Each case starts from the same narrowing the switch itself started with,
    // not from whatever the previous case left behind: cases are checked in
    // textual order here regardless of fallthrough, so without this a guard
    // clause in one case would narrow the next case's code too, and every case's
    // narrowing would otherwise leak past the switch's closing brace.
    let outer_narrow = ctx.narrow.clone();
    for case in &switch_stmt.cases {
        ctx.narrow = outer_narrow.clone();
        if let Some(test) = &case.test {
            infer_expression_type(test, scoping, ctx);
        }
        for stmt in &case.consequent {
            check_statement(stmt, scoping, ctx);
        }
    }
    ctx.narrow = outer_narrow;
}
