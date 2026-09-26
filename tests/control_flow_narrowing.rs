use ts_rust::{DiagnosticCode, TypeChecker};

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

fn check(fixture_source: &str, file_name: &str) -> Vec<ts_rust::Diagnostic> {
    init_tracing();
    let checker = TypeChecker::new();
    let result = checker.check_source(fixture_source, file_name);
    assert!(result.is_ok(), "fixture should at least parse: {result:?}");
    result
        .map(|checked| checked.diagnostics)
        .unwrap_or_default()
}

#[test]
fn typeof_narrows_the_true_branch() {
    let source = include_str!("fixtures/control-flow-narrowing/typeof_narrowing_string_branch.ts");
    let diagnostics = check(source, "typeof_narrowing_string_branch.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn typeof_narrows_the_else_branch_to_the_complement() {
    let source =
        include_str!("fixtures/control-flow-narrowing/typeof_narrowing_else_branch_mismatch.ts");
    let diagnostics = check(source, "typeof_narrowing_else_branch_mismatch.ts");

    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn typeof_object_narrows_to_the_object_member() {
    let source = include_str!(
        "fixtures/control-flow-narrowing/typeof_object_narrows_to_the_object_member.ts"
    );
    let diagnostics = check(source, "typeof_object_narrows_to_the_object_member.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn typeof_function_narrows_to_the_function_member() {
    let source = include_str!(
        "fixtures/control-flow-narrowing/typeof_function_narrows_to_the_function_member.ts"
    );
    let diagnostics = check(source, "typeof_function_narrows_to_the_function_member.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn typeof_object_keeps_null_because_typeof_null_is_object() {
    let source = include_str!("fixtures/control-flow-narrowing/typeof_object_keeps_null.ts");
    let diagnostics = check(source, "typeof_object_keeps_null.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic for returning a possibly-null value, got: {diagnostics:?}"
    );
}

#[test]
fn a_nested_condition_refines_the_enclosing_branch() {
    let source = include_str!("fixtures/control-flow-narrowing/nested_conditions_compose.ts");
    let diagnostics = check(source, "nested_conditions_compose.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn an_unmodelled_typeof_tag_narrows_nothing() {
    let source =
        include_str!("fixtures/control-flow-narrowing/unknown_typeof_tag_narrows_nothing.ts");
    let diagnostics = check(source, "unknown_typeof_tag_narrows_nothing.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn equality_against_null_narrows_both_branches() {
    let source = include_str!("fixtures/control-flow-narrowing/equality_null_narrowing.ts");
    let diagnostics = check(source, "equality_null_narrowing.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn truthiness_narrows_out_null_and_undefined() {
    let source = include_str!("fixtures/control-flow-narrowing/truthy_narrowing.ts");
    let diagnostics = check(source, "truthy_narrowing.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn local_annotated_variable_is_registered_for_later_reference() {
    let source =
        include_str!("fixtures/control-flow-narrowing/local_annotated_variable_is_registered.ts");
    let diagnostics = check(source, "local_annotated_variable_is_registered.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic for assigning number to string, got: {diagnostics:?}"
    );
}

#[test]
fn local_annotated_variable_without_initializer_is_registered() {
    let source = include_str!(
        "fixtures/control-flow-narrowing/local_annotated_variable_without_initializer_is_registered.ts"
    );
    let diagnostics = check(
        source,
        "local_annotated_variable_without_initializer_is_registered.ts",
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic for assigning number to string, got: {diagnostics:?}"
    );
}

// A guard clause's narrowing must stay inside the scope it ran in: it should not
// survive past a loop body, a bare block, a switch case, or a function body that
// only ran once. Each fixture below pairs a leak that must not happen with, in
// the last two, a check that the ordinary same-scope case the fix must not break
// still works.

#[test]
fn guard_clause_in_while_body_does_not_leak() {
    let source =
        include_str!("fixtures/narrowing-scopes/guard_clause_in_while_body_does_not_leak.ts");
    let diagnostics = check(source, "guard_clause_in_while_body_does_not_leak.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].code, DiagnosticCode::DeclaredTypeMismatch);
}

#[test]
fn guard_clause_in_for_body_does_not_leak() {
    let source =
        include_str!("fixtures/narrowing-scopes/guard_clause_in_for_body_does_not_leak.ts");
    let diagnostics = check(source, "guard_clause_in_for_body_does_not_leak.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].code, DiagnosticCode::DeclaredTypeMismatch);
}

#[test]
fn guard_clause_in_block_does_survive() {
    let source = include_str!("fixtures/narrowing-scopes/guard_clause_in_block_does_survive.ts");
    let diagnostics = check(source, "guard_clause_in_block_does_survive.ts");
    // Confirmed against real tsc, not assumed: a bare block is not a
    // control-flow construct, so narrowing correctly survives past it, unlike
    // the while/for/switch/function cases nearby.
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn narrowing_does_not_leak_between_switch_cases() {
    let source =
        include_str!("fixtures/narrowing-scopes/narrowing_does_not_leak_between_switch_cases.ts");
    let diagnostics = check(source, "narrowing_does_not_leak_between_switch_cases.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].code, DiagnosticCode::DeclaredTypeMismatch);
}

#[test]
fn narrowing_does_not_leak_past_switch() {
    let source = include_str!("fixtures/narrowing-scopes/narrowing_does_not_leak_past_switch.ts");
    let diagnostics = check(source, "narrowing_does_not_leak_past_switch.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].code, DiagnosticCode::DeclaredTypeMismatch);
}

// check_function_declaration also resets ctx.narrow around a function body (see
// src/bridge/statements/functions.rs), for the same reason as the scopes tested
// above. It has no test here: NarrowState is keyed by SymbolId, and oxc assigns
// every binding in a program a distinct one, so two different functions' own
// parameters can never collide on a lookup. Skipping the reset would still leave
// ctx.narrow holding entries no later lookup can ever match -- a real hygiene
// issue, since the map grows for the rest of the check for no reason, but not one
// a diagnostic-based test can observe.

#[test]
fn guard_clause_still_narrows_within_the_same_function() {
    let source = include_str!(
        "fixtures/narrowing-scopes/guard_clause_still_narrows_within_the_same_function.ts"
    );
    let diagnostics = check(
        source,
        "guard_clause_still_narrows_within_the_same_function.ts",
    );
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}
