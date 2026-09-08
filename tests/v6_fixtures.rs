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
fn generic_identity_infers_return_type_per_call() {
    let source = include_str!("fixtures/v6/generic_identity_infers_return_type.ts");
    let diagnostics = check(source, "generic_identity_infers_return_type.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn generic_return_type_mismatch_is_caught() {
    let source = include_str!("fixtures/v6/generic_return_type_mismatch_is_caught.ts");
    let diagnostics = check(source, "generic_return_type_mismatch_is_caught.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("not assignable"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn generic_multiple_type_params_infer_independently() {
    let source = include_str!("fixtures/v6/generic_multiple_type_params_infer_independently.ts");
    let diagnostics = check(
        source,
        "generic_multiple_type_params_infer_independently.ts",
    );
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn generic_array_element_type_is_inferred() {
    let source = include_str!("fixtures/v6/generic_array_element_type_is_inferred.ts");
    let diagnostics = check(source, "generic_array_element_type_is_inferred.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn generic_type_param_usable_inside_body() {
    let source = include_str!("fixtures/v6/generic_type_param_usable_inside_body.ts");
    let diagnostics = check(source, "generic_type_param_usable_inside_body.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}
