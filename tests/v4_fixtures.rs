//! Integration tests for V4 Tier 1. Item 1 so far: arrow functions and
//! function expressions as values, including function-type parameter
//! annotations (the shape a real callback-taking function needs).

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
fn fully_annotated_arrow_function_checks_clean() {
    let source = include_str!("fixtures/v4/arrow_function_fully_annotated.ts");
    let diagnostics = check(source, "arrow_function_fully_annotated.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn arrow_function_return_type_mismatch_is_caught() {
    let source = include_str!("fixtures/v4/arrow_function_return_type_mismatch.ts");
    let diagnostics = check(source, "arrow_function_return_type_mismatch.ts");
    assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
}

#[test]
fn untyped_arrow_param_defaults_to_any_instead_of_blocking_the_function() {
    let source = include_str!("fixtures/v4/arrow_function_untyped_param_defaults_to_any.ts");
    let diagnostics = check(source, "arrow_function_untyped_param_defaults_to_any.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn expression_bodied_arrow_return_type_is_inferred_and_checked() {
    let source = include_str!("fixtures/v4/arrow_expression_body_return_inferred.ts");
    let diagnostics = check(source, "arrow_expression_body_return_inferred.ts");
    assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic (the `bad` binding), got: {diagnostics:?}");
}

#[test]
fn function_expression_works_as_a_value() {
    let source = include_str!("fixtures/v4/function_expression_as_value.ts");
    let diagnostics = check(source, "function_expression_as_value.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn callback_parameter_type_annotation_end_to_end() {
    // The realistic case: a function that takes a callback (a
    // function-type parameter annotation), called with an actual arrow
    // function argument. Exercises TSFunctionType resolution, arrow
    // function inference, and function subtyping together.
    let source = include_str!("fixtures/v4/callback_parameter_type_annotation.ts");
    let diagnostics = check(source, "callback_parameter_type_annotation.ts");
    assert!(diagnostics.is_empty(), "expected no false positives, got: {diagnostics:?}");
}

#[test]
fn any_typed_value_is_callable() {
    // check_callable used to only exclude Type::Error from its "not
    // callable" diagnostic — calling a value of type `any` incorrectly
    // errored, even though that's always legal in real TS/JS.
    let source = include_str!("fixtures/v4/any_typed_value_is_callable.ts");
    let diagnostics = check(source, "any_typed_value_is_callable.ts");

    assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got: {diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("Operator"),
        "expected the `1 - \"x\"` mismatch inside the call, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, ts_rust::Severity::Error);
}

#[test]
fn function_expression_does_not_inherit_this() {
    // infer_function_expression_type used to leave current_class_instance
    // untouched, letting `this` inside a plain function expression
    // nested in a class method incorrectly resolve to that class's
    // instance type. Arrow functions correctly DO inherit `this`
    // lexically (see infer_arrow_function_type, deliberately untouched);
    // plain function expressions get their own dynamic `this` and
    // should not.
    let source = include_str!("fixtures/v4/function_expression_does_not_inherit_this.ts");
    let diagnostics = check(source, "function_expression_does_not_inherit_this.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics: `this` inside the nested function expression should resolve to \
         Error (unknown), not leak in Counter's instance type. Got: {diagnostics:?}"
    );
}

#[test]
fn callback_type_annotation_with_untyped_param_still_registers() {
    // A TSFunctionType annotation (e.g. a callback parameter's own type,
    // `fn: (x) => number`) used to require every one of ITS OWN
    // parameters to be typed too, via the strict resolve_function_params
    // — inconsistent with how an actual arrow-function value handles the
    // identical gap (defaults to `any`). That inconsistency meant the
    // whole enclosing function (`apply`) silently failed to register at
    // all, with its body never checked. See
    // resolve_params_with_any_fallback in type_annotation.rs.
    let source = include_str!("fixtures/v4/callback_type_annotation_with_untyped_param.ts");
    let diagnostics = check(source, "callback_type_annotation_with_untyped_param.ts");

    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic (apply's body should now actually be checked), got: {diagnostics:?}"
    );
    assert!(
        diagnostics[0].message.contains("not assignable"),
        "expected the `const result: string = fn(value)` mismatch inside apply's body, got: {diagnostics:?}"
    );
}
