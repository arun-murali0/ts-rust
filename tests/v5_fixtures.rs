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
fn optional_param_can_be_omitted_or_provided() {
    let source = include_str!("fixtures/v5/optional_param_can_be_omitted.ts");
    let diagnostics = check(source, "optional_param_can_be_omitted.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn optional_param_does_not_excuse_a_missing_required_arg() {
    let source = include_str!("fixtures/v5/optional_param_does_not_excuse_missing_required_arg.ts");
    let diagnostics = check(
        source,
        "optional_param_does_not_excuse_missing_required_arg.ts",
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("argument(s)"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn rest_param_accepts_any_trailing_arg_count() {
    let source = include_str!("fixtures/v5/rest_param_accepts_any_trailing_arg_count.ts");
    let diagnostics = check(source, "rest_param_accepts_any_trailing_arg_count.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn rest_param_argument_type_mismatch_is_caught() {
    let source = include_str!("fixtures/v5/rest_param_argument_type_mismatch_is_caught.ts");
    let diagnostics = check(source, "rest_param_argument_type_mismatch_is_caught.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0]
            .message
            .contains("not assignable to parameter type"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn rest_param_identifier_is_bound_as_an_array() {
    let source = include_str!("fixtures/v5/rest_param_identifier_is_bound_as_array.ts");
    let diagnostics = check(source, "rest_param_identifier_is_bound_as_array.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn object_destructuring_binds_property_types_and_checks_clean() {
    let source = include_str!("fixtures/v5/object_destructuring_binds_property_types.ts");
    let diagnostics = check(source, "object_destructuring_binds_property_types.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn object_destructuring_missing_property_is_caught() {
    let source = include_str!("fixtures/v5/object_destructuring_missing_property_is_caught.ts");
    let diagnostics = check(source, "object_destructuring_missing_property_is_caught.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("does not exist"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn destructured_function_parameter_checks_clean() {
    let source = include_str!("fixtures/v5/destructured_function_parameter_checks_clean.ts");
    let diagnostics = check(source, "destructured_function_parameter_checks_clean.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn array_destructuring_binds_element_type_and_checks_clean() {
    let source = include_str!("fixtures/v5/array_destructuring_binds_element_type.ts");
    let diagnostics = check(source, "array_destructuring_binds_element_type.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn array_destructuring_element_type_mismatch_is_caught() {
    let source = include_str!("fixtures/v5/array_destructuring_element_type_mismatch_is_caught.ts");
    let diagnostics = check(
        source,
        "array_destructuring_element_type_mismatch_is_caught.ts",
    );
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
fn destructuring_default_value_checks_clean() {
    let source = include_str!("fixtures/v5/destructuring_default_value_checks_clean.ts");
    let diagnostics = check(source, "destructuring_default_value_checks_clean.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn object_destructuring_renamed_binding_checks_clean() {
    let source = include_str!("fixtures/v5/object_destructuring_renamed_binding_checks_clean.ts");
    let diagnostics = check(
        source,
        "object_destructuring_renamed_binding_checks_clean.ts",
    );
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn array_destructuring_a_non_array_source_is_caught() {
    let source = include_str!("fixtures/v5/array_destructuring_non_array_source_is_caught.ts");
    let diagnostics = check(source, "array_destructuring_non_array_source_is_caught.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("requires an array type"),
        "got: {diagnostics:?}"
    );
}
