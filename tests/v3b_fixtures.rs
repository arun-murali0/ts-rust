//! Integration tests for V3b: typeof, equality, and truthiness narrowing.
//! Narrowing here is scoped strictly to inside the branch that earned it,
//! it does not persist past the `if` statement. See
//! known_gap_narrowing_does_not_persist_past_if.ts for what that costs.

use ts_rust::TypeChecker;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

fn check(fixture_source: &str, file_name: &str) -> Vec<ts_rust::Diagnostic> {
    init_tracing();
    let checker = TypeChecker::new();
    checker.check_source(fixture_source, file_name).expect("fixture should at least parse").diagnostics
}

#[test]
fn typeof_narrows_the_true_branch() {
    let source = include_str!("fixtures/v3b/typeof_narrowing_string_branch.ts");
    let diagnostics = check(source, "typeof_narrowing_string_branch.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn typeof_narrows_the_else_branch_to_the_complement() {
    let source = include_str!("fixtures/v3b/typeof_narrowing_else_branch_mismatch.ts");
    let diagnostics = check(source, "typeof_narrowing_else_branch_mismatch.ts");
    // The `else` branch narrows x to `number`, which isn't assignable to
    // the function's declared `string` return type.
    assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
}

#[test]
fn equality_against_null_narrows_both_branches() {
    let source = include_str!("fixtures/v3b/equality_null_narrowing.ts");
    let diagnostics = check(source, "equality_null_narrowing.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn truthiness_narrows_out_null_and_undefined() {
    let source = include_str!("fixtures/v3b/truthy_narrowing.ts");
    let diagnostics = check(source, "truthy_narrowing.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn narrowing_does_not_persist_past_the_if_statement_known_gap() {
    // If this starts passing because persistent join semantics got
    // implemented, update this test and docs/ROADMAP.md together, don't
    // just delete it.
    let source = include_str!("fixtures/v3b/known_gap_narrowing_does_not_persist_past_if.ts");
    let diagnostics = check(source, "known_gap_narrowing_does_not_persist_past_if.ts");
    assert_eq!(diagnostics.len(), 1, "known-gap fixture behavior changed, got: {diagnostics:?}");
}

#[test]
fn local_annotated_variable_is_registered_for_later_reference() {
    // Regression test: an annotated local variable declaration
    // (`let x: number = 5;`) was checked once against its own initializer
    // but never registered in the symbol map. A later reference to it in
    // the same body then resolved via oxc_semantic fine but found
    // nothing in ctx.symbols, silently fell back to the error sentinel,
    // and passed every subsequent check against it, since Error is
    // universally compatible in is_subtype. That turned a real type
    // error (`let y: string = x` where x is a number) into a false
    // negative instead of a diagnostic.
    let source = include_str!("fixtures/v3b/local_annotated_variable_is_registered.ts");
    let diagnostics = check(source, "local_annotated_variable_is_registered.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic for assigning number to string, got: {diagnostics:?}"
    );
}

#[test]
fn local_annotated_variable_without_initializer_is_registered() {
    // Same gap, different branch of the match: `let x: number;` with no
    // initializer has nothing to check at the declaration site, but the
    // binding still needs a type on record before any later reference to
    // it is checked.
    let source = include_str!("fixtures/v3b/local_annotated_variable_without_initializer_is_registered.ts");
    let diagnostics = check(source, "local_annotated_variable_without_initializer_is_registered.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic for assigning number to string, got: {diagnostics:?}"
    );
}
