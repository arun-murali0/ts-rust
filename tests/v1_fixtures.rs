//! Integration tests: exercises the full public API (`TypeChecker::check_source`)
//! against real fixture files, per docs/ROADMAP.md V1 exit criteria.
//!
//! Run with `cargo test -- --nocapture` and `RUST_LOG=trace` to see the
//! `tracing` spans emitted through parse -> declare -> check while
//! debugging locally:
//!
//!   RUST_LOG=trace cargo test -- --nocapture

use ts_rust::{Severity, TypeChecker};

fn init_tracing() {
    // `try_init` (not `init`) because multiple tests in this binary would
    // otherwise panic trying to install a global subscriber twice.
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
fn primitive_mismatch_is_caught() {
    let source = include_str!("fixtures/v1/primitive_mismatch.ts");
    let diagnostics = check(source, "primitive_mismatch.ts");

    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0].message.contains("not assignable"));
}

#[test]
fn correct_primitive_assignment_produces_no_diagnostics() {
    let source = include_str!("fixtures/v1/primitive_ok.ts");
    let diagnostics = check(source, "primitive_ok.ts");

    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn function_argument_mismatch_is_caught() {
    let source = include_str!("fixtures/v1/function_arg_mismatch.ts");
    let diagnostics = check(source, "function_arg_mismatch.ts");

    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(diagnostics[0]
        .message
        .contains("not assignable to parameter"));
}

#[test]
fn function_return_mismatch_is_caught() {
    let source = include_str!("fixtures/v1/function_return_mismatch.ts");
    let diagnostics = check(source, "function_return_mismatch.ts");

    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert!(diagnostics[0].message.contains("Return type"));
}

#[test]
fn binary_operator_mismatch_is_caught() {
    let source = include_str!("fixtures/v1/binary_op_mismatch.ts");
    let diagnostics = check(source, "binary_op_mismatch.ts");

    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("cannot be applied")),
        "expected an operator-mismatch diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn unsupported_statement_degrades_honestly_not_silently() {
    let source = include_str!("fixtures/v1/unsupported_class.ts");
    let diagnostics = check(source, "unsupported_class.ts");

    // Per docs/DESIGN.md section 1.5: an unsupported construct must produce a
    // visible diagnostic, never a silent pass and never a panic.
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message.contains("not yet checked")),
        "expected an Unsupported-style warning, got: {diagnostics:?}"
    );
}

#[test]
fn malformed_source_reports_a_parse_error_instead_of_panicking() {
    init_tracing();
    let checker = TypeChecker::new();
    let result = checker.check_source("const x = ;", "broken.ts");

    assert!(result.is_err(), "expected a parse error, got: {result:?}");
}
