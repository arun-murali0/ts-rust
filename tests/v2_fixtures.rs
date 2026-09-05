//! Integration tests for V2: objects, unions, arrays, aliases, interfaces.

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
    checker
        .check_source(fixture_source, file_name)
        .expect("fixture should at least parse")
        .diagnostics
}

#[test]
fn extra_property_on_object_literal_is_allowed() {
    let source = include_str!("fixtures/v2/width_subtyping_extra_prop_ok.ts");
    let diagnostics = check(source, "width_subtyping_extra_prop_ok.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn missing_required_property_is_caught() {
    let source = include_str!("fixtures/v2/width_subtyping_missing_prop_error.ts");
    let diagnostics = check(source, "width_subtyping_missing_prop_error.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0].message.contains("not assignable"));
}

#[test]
fn union_assignment_checks_each_member() {
    let source = include_str!("fixtures/v2/union_assignment_both_directions.ts");
    let diagnostics = check(source, "union_assignment_both_directions.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(diagnostics[0].message.contains("not assignable"));
}

#[test]
fn array_element_type_is_covariant() {
    let source = include_str!("fixtures/v2/array_covariance.ts");
    let diagnostics = check(source, "array_covariance.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn simple_type_alias_resolves() {
    let source = include_str!("fixtures/v2/alias_resolution.ts");
    let diagnostics = check(source, "alias_resolution.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn interfaces_can_reference_each_other_regardless_of_declaration_order() {
    let source = include_str!("fixtures/v2/interface_forward_ref.ts");
    let diagnostics = check(source, "interface_forward_ref.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn excess_property_on_a_fresh_literal_is_a_known_gap_not_a_silent_pass() {
    let source = include_str!("fixtures/v2/excess_property_literal.ts");
    let diagnostics = check(source, "excess_property_literal.ts");
    assert!(
        diagnostics.is_empty(),
        "known-gap fixture behavior changed, got: {diagnostics:?}"
    );
}

#[test]
fn function_arity_mismatches_are_caught_both_directions() {
    let source = include_str!("fixtures/v2/function_arity_errors.ts");
    let diagnostics = check(source, "function_arity_errors.ts");
    assert_eq!(
        diagnostics.len(),
        2,
        "expected exactly two diagnostics, got: {diagnostics:?}"
    );
    assert!(diagnostics
        .iter()
        .all(|d| d.message.contains("Expected 2 argument")));
}

#[test]
fn missing_optional_property_is_not_an_error() {
    let source = include_str!("fixtures/v2/optional_property.ts");
    let diagnostics = check(source, "optional_property.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn width_subtyping_with_multiple_properties() {
    let source = include_str!("fixtures/v2/width_subtyping_multiple_props.ts");
    let diagnostics = check(source, "width_subtyping_multiple_props.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no errors, got: {diagnostics:?}"
    );
}

#[test]
fn union_assignment_rejects_unrelated_type() {
    let source = include_str!("fixtures/v2/union_assignment_error.ts");
    let diagnostics = check(source, "union_assignment_error.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected one error, got: {diagnostics:?}"
    );
    assert!(diagnostics[0].message.contains("not assignable"));
}

#[test]
fn array_covariance_allows_subtype_assignment() {
    let source = include_str!("fixtures/v2/array_covariance_unsound.ts");
    let diagnostics = check(source, "array_covariance_unsound.ts");
    let type_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        type_errors.is_empty(),
        "array covariance should not error, got: {type_errors:?}"
    );
}

#[test]
fn circular_type_reference_does_not_infinite_loop() {
    let source = include_str!("fixtures/v2/circular_type_reference.ts");
    let diagnostics = check(source, "circular_type_reference.ts");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("Circular") || d.message.contains("not yet checked")),
        "expected circular reference warning, got: {diagnostics:?}"
    );
}

#[test]
fn nested_object_subtyping() {
    let source = include_str!("fixtures/v2/nested_object_subtyping.ts");
    let diagnostics = check(source, "nested_object_subtyping.ts");
    assert!(
        diagnostics.is_empty(),
        "nested width subtyping should work, got: {diagnostics:?}"
    );
}

#[test]
fn function_parameters_are_usable_inside_their_own_body() {
    let source = include_str!("fixtures/v2/parameter_used_in_binary_expression.ts");
    let diagnostics = check(source, "parameter_used_in_binary_expression.ts");
    assert!(
        diagnostics.is_empty(),
        "parameters should be checkable inside their own function body, got: {diagnostics:?}"
    );
}

#[test]
fn unresolvable_type_annotation_is_reported_not_silently_swallowed() {
    // Regression test: a type annotation referencing a name that was
    // never declared at all (a typo, most commonly) used to be
    // indistinguishable from "no annotation was written" once resolution
    // failed, since both collapsed to the same `None`. The variable's
    // inferred type got registered with zero diagnostic anywhere,
    // meaning `let x: DoesNotExist = 5;` type-checked cleanly with no
    // indication `DoesNotExist` was never found.
    //
    // Fixed via bridge/statements.rs's AnnotationOutcome, which keeps
    // "absent" and "present but unresolvable" as distinct outcomes.
    let source = include_str!("fixtures/v2/unresolvable_type_annotation_is_reported.ts");
    let diagnostics = check(source, "unresolvable_type_annotation_is_reported.ts");
    assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0].message.contains("could not be resolved"));
}
