use ts_rust::{Severity, TypeChecker};

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
fn generic_interface_substitution_checks_clean() {
    let source =
        include_str!("fixtures/generics-tier2/generic_interface_substitution_checks_clean.ts");
    let diagnostics = check(source, "generic_interface_substitution_checks_clean.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn generic_interface_substitution_mismatch_is_caught() {
    let source = include_str!(
        "fixtures/generics-tier2/generic_interface_substitution_mismatch_is_caught.ts"
    );
    let diagnostics = check(
        source,
        "generic_interface_substitution_mismatch_is_caught.ts",
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
fn generic_type_alias_substitution_checks_clean() {
    let source =
        include_str!("fixtures/generics-tier2/generic_type_alias_substitution_checks_clean.ts");
    let diagnostics = check(source, "generic_type_alias_substitution_checks_clean.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn generic_type_alias_substitution_mismatch_is_caught() {
    let source = include_str!(
        "fixtures/generics-tier2/generic_type_alias_substitution_mismatch_is_caught.ts"
    );
    let diagnostics = check(
        source,
        "generic_type_alias_substitution_mismatch_is_caught.ts",
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
fn two_instantiations_of_the_same_interface_stay_distinct() {
    let source = include_str!(
        "fixtures/generics-tier2/two_instantiations_of_the_same_interface_stay_distinct.ts"
    );
    let diagnostics = check(
        source,
        "two_instantiations_of_the_same_interface_stay_distinct.ts",
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "Box<number> and Box<string> must not be interchangeable, got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("not assignable"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn bare_generic_reference_without_type_arguments_still_resolves() {
    let source = include_str!(
        "fixtures/generics-tier2/bare_generic_reference_without_type_arguments_still_resolves.ts"
    );
    let diagnostics = check(
        source,
        "bare_generic_reference_without_type_arguments_still_resolves.ts",
    );
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Error),
        "a bare, un-instantiated generic reference should not be a hard error, got: {diagnostics:?}"
    );
}
