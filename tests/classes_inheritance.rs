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
fn method_call_through_member_expression_is_checked() {
    let source =
        include_str!("fixtures/classes-inheritance/method_call_through_member_expression.ts");
    let diagnostics = check(source, "method_call_through_member_expression.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn extends_flattens_inherited_fields() {
    let source = include_str!("fixtures/classes-inheritance/extends_inherits_fields.ts");
    let diagnostics = check(source, "extends_inherits_fields.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn missing_an_inherited_field_is_caught() {
    let source = include_str!("fixtures/classes-inheritance/extends_missing_inherited_field.ts");
    let diagnostics = check(source, "extends_missing_inherited_field.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn new_expression_checks_constructor_arity() {
    let source = include_str!("fixtures/classes-inheritance/new_expression_arity_mismatch.ts");
    let diagnostics = check(source, "new_expression_arity_mismatch.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(diagnostics[0].message.contains("Expected 2 argument"));
}

#[test]
fn this_expression_resolves_to_the_class_instance_type() {
    let source = include_str!("fixtures/classes-inheritance/this_expression_known_gap.ts");
    let diagnostics = check(source, "this_expression_known_gap.ts");
    assert!(
        diagnostics.is_empty(),
        "expected `this.count` to resolve correctly, got: {diagnostics:?}"
    );
}

#[test]
fn this_expression_type_mismatch_is_caught() {
    let source = include_str!("fixtures/classes-inheritance/this_expression_type_mismatch.ts");
    let diagnostics = check(source, "this_expression_type_mismatch.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn untyped_constructor_param_skips_arity_check_instead_of_false_flagging() {
    let source =
        include_str!("fixtures/classes-inheritance/untyped_constructor_param_skips_arity_check.ts");
    let diagnostics = check(source, "untyped_constructor_param_skips_arity_check.ts");
    assert_eq!(
        diagnostics.len(),
        3,
        "expected two implicit-any errors (x and y) and one skipped-arity warning, got: {diagnostics:?}"
    );

    let implicit_any: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("implicitly has an 'any' type"))
        .collect();
    assert_eq!(implicit_any.len(), 2, "got: {diagnostics:?}");
    assert!(implicit_any.iter().all(|d| d.severity == Severity::Error));

    let skipped_arity = diagnostics
        .iter()
        .find(|d| d.message.contains("untyped parameter"));
    assert_eq!(
        skipped_arity.map(|diagnostic| diagnostic.severity),
        Some(Severity::Warning),
        "got: {diagnostics:?}"
    );
}

#[test]
fn one_unresolvable_member_makes_the_whole_class_unsupported() {
    let source = include_str!(
        "fixtures/classes-inheritance/one_unresolvable_member_makes_whole_class_unsupported.ts"
    );
    let diagnostics = check(
        source,
        "one_unresolvable_member_makes_whole_class_unsupported.ts",
    );
    assert_eq!(
        diagnostics.len(),
        2,
        "expected the implicit-any error and the unsupported-class warning, got: {diagnostics:?}"
    );

    let implicit_any = diagnostics
        .iter()
        .find(|d| d.message.contains("implicitly has an 'any' type"));
    assert_eq!(
        implicit_any.map(|diagnostic| diagnostic.severity),
        Some(Severity::Error),
        "got: {diagnostics:?}"
    );

    let unsupported = diagnostics.iter().find(|d| d.severity == Severity::Warning);
    assert!(
        unsupported.is_some_and(|d| d.message.contains("Point")),
        "got: {diagnostics:?}"
    );
}

#[test]
fn untyped_method_param_skips_arity_not_whole_class() {
    let source = include_str!(
        "fixtures/classes-inheritance/untyped_method_param_skips_arity_not_whole_class.ts"
    );
    let diagnostics = check(
        source,
        "untyped_method_param_skips_arity_not_whole_class.ts",
    );

    assert_eq!(
        diagnostics.len(),
        4,
        "expected exactly four diagnostics, got: {diagnostics:?}"
    );

    let implicit_any = diagnostics
        .iter()
        .find(|d| d.message.contains("implicitly has an 'any' type"));
    assert_eq!(
        implicit_any.map(|diagnostic| diagnostic.severity),
        Some(Severity::Error),
        "expected an implicit-any error for `value`, got: {diagnostics:?}"
    );

    let count_mismatch = diagnostics
        .iter()
        .find(|d| d.message.contains("not assignable"));
    assert!(
        count_mismatch.is_some(),
        "expected a type-mismatch diagnostic for `count`, got: {diagnostics:?}"
    );
    assert_eq!(
        count_mismatch.map(|diagnostic| diagnostic.severity),
        Some(Severity::Error)
    );

    let untyped_warning = diagnostics
        .iter()
        .find(|d| d.message.contains("untyped parameter"));
    assert!(
        untyped_warning.is_some(),
        "expected an untyped-parameter warning, got: {diagnostics:?}"
    );
    assert_eq!(
        untyped_warning.map(|diagnostic| diagnostic.severity),
        Some(Severity::Warning)
    );

    let binary_op_mismatch = diagnostics.iter().find(|d| d.message.contains("Operator"));
    assert!(
        binary_op_mismatch.is_some(),
        "expected the `1 - \"x\"` mismatch inside the call to still be caught, got: {diagnostics:?}"
    );
    assert_eq!(
        binary_op_mismatch.map(|diagnostic| diagnostic.severity),
        Some(Severity::Error)
    );
}
