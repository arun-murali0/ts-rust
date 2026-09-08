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
    let result = checker.check_source(fixture_source, file_name);
    assert!(result.is_ok(), "fixture should at least parse: {result:?}");
    result
        .map(|checked| checked.diagnostics)
        .unwrap_or_default()
}

#[test]
fn typeof_narrows_the_true_branch() {
    let source = include_str!("fixtures/v3b/typeof_narrowing_string_branch.ts");
    let diagnostics = check(source, "typeof_narrowing_string_branch.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn typeof_narrows_the_else_branch_to_the_complement() {
    let source = include_str!("fixtures/v3b/typeof_narrowing_else_branch_mismatch.ts");
    let diagnostics = check(source, "typeof_narrowing_else_branch_mismatch.ts");

    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn equality_against_null_narrows_both_branches() {
    let source = include_str!("fixtures/v3b/equality_null_narrowing.ts");
    let diagnostics = check(source, "equality_null_narrowing.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn truthiness_narrows_out_null_and_undefined() {
    let source = include_str!("fixtures/v3b/truthy_narrowing.ts");
    let diagnostics = check(source, "truthy_narrowing.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn local_annotated_variable_is_registered_for_later_reference() {
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
    let source =
        include_str!("fixtures/v3b/local_annotated_variable_without_initializer_is_registered.ts");
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
