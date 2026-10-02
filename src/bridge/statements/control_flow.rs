use oxc_ast::ast::{
    Expression, ForStatement, IfStatement, Statement, SwitchStatement, WhileStatement,
};
use oxc_semantic::Scoping;

use super::super::context::CheckContext;
use super::super::expressions::infer_expression_type;
use super::super::narrow::{
    NarrowState, join_states, narrow_condition, narrow_switch_case, narrow_switch_default,
    switch_discriminant,
};
use super::{check_statement, check_variable_declaration, statement_always_exits};

// Where control flow joins after an `if`. What the code after the statement sees
// depends on which branches can fall out the bottom, because a branch that always
// exits contributes nothing to it; that is why the guard clause
// `if (x === null) return;` narrows everything that follows. When both branches
// can fall through their end states are joined, so a variable narrowed or assigned
// on both keeps the union of the two, while one narrowed on only one branch
// reverts to its earlier type (`if (x === null) { x = "d"; }` leaves x a string).
// When neither can, the code after is unreachable and the earlier state is kept.
// With no else branch the missing alternate is the condition's false side alone.
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
    let consequent_end = std::mem::replace(&mut ctx.narrow, outer_narrow.clone());

    let (alternate_always_exits, alternate_end) = match &if_stmt.alternate {
        Some(alternate) => {
            ctx.narrow.extend(false_overrides);
            check_statement(alternate, scoping, ctx);
            (statement_always_exits(alternate), ctx.narrow.clone())
        }
        None => {
            let mut implicit_else = outer_narrow.clone();
            implicit_else.extend(false_overrides);
            (false, implicit_else)
        }
    };

    ctx.narrow = match (consequent_always_exits, alternate_always_exits) {
        (true, false) => alternate_end,
        (false, true) => consequent_end,
        (false, false) => join_states(&consequent_end, &alternate_end, &mut ctx.arena),
        (true, true) => outer_narrow,
    };
}

// Whether a `break` appears anywhere inside `stmt`. Deliberately generous: a break
// that belongs to a nested loop or switch is counted too, and a `try` or `with` is
// assumed to hide one, because the only use of this answer is to decide that a loop
// can only be left by its test failing, and a false "yes there is a break" merely
// skips a narrowing. A function body is not entered: a break cannot cross it.
fn contains_break(stmt: &Statement) -> bool {
    match stmt {
        Statement::BreakStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(contains_break),
        Statement::IfStatement(if_stmt) => {
            contains_break(&if_stmt.consequent)
                || if_stmt.alternate.as_ref().is_some_and(contains_break)
        }
        Statement::WhileStatement(inner) => contains_break(&inner.body),
        Statement::DoWhileStatement(inner) => contains_break(&inner.body),
        Statement::ForStatement(inner) => contains_break(&inner.body),
        Statement::ForInStatement(inner) => contains_break(&inner.body),
        Statement::ForOfStatement(inner) => contains_break(&inner.body),
        Statement::LabeledStatement(inner) => contains_break(&inner.body),
        Statement::SwitchStatement(inner) => inner
            .cases
            .iter()
            .any(|case| case.consequent.iter().any(contains_break)),
        Statement::TryStatement(_) | Statement::WithStatement(_) => true,
        _ => false,
    }
}

// What the code after a loop sees. The loop test narrows the body (`while (x !== null)
// { x.length }`), since the test runs before every pass. The body is walked once, so
// narrowing it establishes does not carry into the next pass. After the loop the state
// is the join of the state from before (the body may not run at all) and the state the
// body ended in. And if nothing in the body can `break`, the loop is only left by its
// test failing, so the test's false side also holds afterwards: after `while (x !==
// null) { ... }` the variable is null.
fn state_after_loop(
    outer_narrow: &NarrowState,
    body_end: &NarrowState,
    exit_overrides: Option<NarrowState>,
    body: &Statement,
    ctx: &mut CheckContext<'_, '_>,
) -> NarrowState {
    let mut after = join_states(outer_narrow, body_end, &mut ctx.arena);
    if let Some(exit_overrides) = exit_overrides
        && !contains_break(body)
    {
        after.extend(exit_overrides);
    }
    after
}

pub(super) fn check_while_statement(
    while_stmt: &WhileStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    infer_expression_type(&while_stmt.test, scoping, ctx);

    let (true_overrides, false_overrides) = narrow_condition(&while_stmt.test, scoping, ctx);
    let outer_narrow = ctx.narrow.clone();
    ctx.narrow.extend(true_overrides);
    check_statement(&while_stmt.body, scoping, ctx);
    let body_end = ctx.narrow.clone();
    ctx.narrow = state_after_loop(
        &outer_narrow,
        &body_end,
        Some(false_overrides),
        &while_stmt.body,
        ctx,
    );
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

    // Same reasoning as check_while_statement. A `for` with no test never narrows
    // anything on exit: it can only be left by a break or a return.
    let (true_overrides, false_overrides) = match &for_stmt.test {
        Some(test) => {
            let (on_true, on_false) = narrow_condition(test, scoping, ctx);
            (on_true, Some(on_false))
        }
        None => (NarrowState::new(), None),
    };
    let outer_narrow = ctx.narrow.clone();
    ctx.narrow.extend(true_overrides);
    check_statement(&for_stmt.body, scoping, ctx);
    let body_end = ctx.narrow.clone();
    ctx.narrow = state_after_loop(
        &outer_narrow,
        &body_end,
        false_overrides,
        &for_stmt.body,
        ctx,
    );
}

pub(super) fn check_switch_statement(
    switch_stmt: &SwitchStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    infer_expression_type(&switch_stmt.discriminant, scoping, ctx);

    // `switch (kind)`, `switch (shape.kind)` and `switch (typeof x)` narrow the
    // discriminant's variable in each case body. Any other discriminant is
    // inferred normally and narrows nothing.
    let discriminant = switch_discriminant(&switch_stmt.discriminant, scoping);

    // Every case test, gathered once so default can exclude all of them: it has no
    // test of its own, only the complement of what the other cases claimed.
    let all_tests: Vec<&Expression> = switch_stmt
        .cases
        .iter()
        .filter_map(|case| case.test.as_ref())
        .collect();

    // Each case body starts from the narrowing the switch itself started with, not
    // from whatever the previous case left behind: cases are checked in textual
    // order regardless of fallthrough, so without this a guard clause in one case
    // would narrow the next case's code too, and every case's narrowing would
    // otherwise leak past the switch's closing brace.
    //
    // Case labels with an empty body fall into the next case (`case "a": case "b":
    // body`), so the body is narrowed to any of the labels it is reached through,
    // not just the last. Real fallthrough out of a non-empty body is not modeled:
    // each such case is narrowed as if reached directly, matching tsc for the
    // usual break/return style (see statement_always_exits).
    let outer_narrow = ctx.narrow.clone();
    let mut pending_tests: Vec<&Expression> = Vec::new();
    let mut pending_default = false;
    for case in &switch_stmt.cases {
        ctx.narrow = outer_narrow.clone();
        match &case.test {
            Some(test) => {
                infer_expression_type(test, scoping, ctx);
                pending_tests.push(test);
            }
            None => pending_default = true,
        }
        if case.consequent.is_empty() {
            continue;
        }

        if let Some(discriminant) = &discriminant {
            // A default grouped with other labels can be reached with any value,
            // so only a lone default or a set of plain case labels narrows.
            let overlay = match (pending_default, pending_tests.is_empty()) {
                (false, false) => narrow_switch_case(discriminant, &pending_tests, ctx),
                (true, true) => narrow_switch_default(discriminant, &all_tests, ctx),
                _ => NarrowState::new(),
            };
            ctx.narrow.extend(overlay);
        }
        pending_tests.clear();
        pending_default = false;

        for stmt in &case.consequent {
            check_statement(stmt, scoping, ctx);
        }
    }
    ctx.narrow = outer_narrow;
}
