//! Integration tests for V3c: class fields, methods, extends, `new`, and
//! `this`.

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
fn method_call_through_member_expression_is_checked() {
    let source = include_str!("fixtures/v3c/method_call_through_member_expression.ts");
    let diagnostics = check(source, "method_call_through_member_expression.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn extends_flattens_inherited_fields() {
    let source = include_str!("fixtures/v3c/extends_inherits_fields.ts");
    let diagnostics = check(source, "extends_inherits_fields.ts");
    assert!(
        diagnostics.is_empty(),
        "expected no false positives, got: {diagnostics:?}"
    );
}

#[test]
fn missing_an_inherited_field_is_caught() {
    let source = include_str!("fixtures/v3c/extends_missing_inherited_field.ts");
    let diagnostics = check(source, "extends_missing_inherited_field.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn new_expression_checks_constructor_arity() {
    let source = include_str!("fixtures/v3c/new_expression_arity_mismatch.ts");
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
    // this_expression_known_gap.ts used to be a known-gap fixture: `this`
    // fell into the generic "not yet checked" catch-all. Now that
    // Expression::ThisExpression resolves to
    // ctx.current_class_instance, `this.count` correctly resolves to
    // `number`, matching increment()'s declared return type, so this
    // fixture now produces zero diagnostics instead of one.
    let source = include_str!("fixtures/v3c/this_expression_known_gap.ts");
    let diagnostics = check(source, "this_expression_known_gap.ts");
    assert!(
        diagnostics.is_empty(),
        "expected `this.count` to resolve correctly, got: {diagnostics:?}"
    );
}

#[test]
fn this_expression_type_mismatch_is_caught() {
    // Proves `this` typing is a real check, not just a permissive
    // pass-through: this.count is `number`, but the method promises to
    // return `string`.
    let source = include_str!("fixtures/v3c/this_expression_type_mismatch.ts");
    let diagnostics = check(source, "this_expression_type_mismatch.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn untyped_constructor_param_skips_arity_check_instead_of_false_flagging() {
    // Regression test: declare.rs used to register a class's constructor
    // arity as 0 both when there's truly no constructor and when there
    // IS one but a parameter is untyped, since resolve_function_params
    // returns None for the whole parameter list the moment any single one
    // lacks an annotation, and declare.rs fell back to an empty Vec
    // either way. Those aren't the same case: `new Point(1, 2)` here
    // would be falsely flagged as "Expected 0 argument(s), but got 2."
    //
    // Fixed properly now: declare.rs sets `is_untyped: true` on the
    // constructor's FunctionType instead of silently defaulting to zero
    // params, and infer_new_expression_type reads that flag directly.
    let source = include_str!("fixtures/v3c/untyped_constructor_param_skips_arity_check.ts");
    let diagnostics = check(source, "untyped_constructor_param_skips_arity_check.ts");
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0].message.contains("untyped parameter"));
}

#[test]
fn one_unresolvable_member_makes_the_whole_class_unsupported() {
    // Regression test: namespace.rs's resolve_class used to skip an
    // unresolvable field or method individually and still register the
    // rest of the class as a partial instance type, with zero diagnostic
    // anywhere. Every caller of that class (a variable typed as it, a
    // `new` call, an assignment) would then silently check against an
    // incomplete shape.
    //
    // Now this matches resolve_object_members's existing policy for
    // interfaces: any unresolvable member fails the whole class, and
    // that failure is reported once, right here at the class's own
    // declaration site (bridge/statements.rs's ClassDeclaration arm),
    // rather than silently or repeatedly at every reference to it.
    let source =
        include_str!("fixtures/v3c/one_unresolvable_member_makes_whole_class_unsupported.ts");
    let diagnostics = check(
        source,
        "one_unresolvable_member_makes_whole_class_unsupported.ts",
    );
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0].message.contains("Point"));
}

#[test]
fn untyped_method_param_skips_arity_not_whole_class() {
    // Narrower case than one_unresolvable_member_makes_the_whole_class_
    // unsupported above: a method whose return type IS known, but whose
    // parameter isn't annotated, should register with `is_untyped`
    // rather than taking the entire class down with it. See
    // namespace.rs's resolve_class method branch and check_callable's
    // is_untyped handling in bridge/expressions.rs.
    let source = include_str!("fixtures/v3c/untyped_method_param_skips_arity_not_whole_class.ts");
    let diagnostics = check(
        source,
        "untyped_method_param_skips_arity_not_whole_class.ts",
    );

    assert_eq!(
        diagnostics.len(),
        3,
        "expected exactly three diagnostics, got: {diagnostics:?}"
    );

    // The class actually resolved: `count` is a real, checked `number`
    // property, not a symptom of the whole class collapsing to Error.
    let count_mismatch = diagnostics
        .iter()
        .find(|d| d.message.contains("not assignable"));
    assert!(
        count_mismatch.is_some(),
        "expected a type-mismatch diagnostic for `count`, got: {diagnostics:?}"
    );
    assert_eq!(count_mismatch.unwrap().severity, Severity::Error);

    // Arity itself is skipped, with a warning rather than an error.
    let untyped_warning = diagnostics
        .iter()
        .find(|d| d.message.contains("untyped parameter"));
    assert!(
        untyped_warning.is_some(),
        "expected an untyped-parameter warning, got: {diagnostics:?}"
    );
    assert_eq!(untyped_warning.unwrap().severity, Severity::Warning);

    // But each argument expression is still checked on its own terms:
    // `1 - "x"` inside the call should still be flagged.
    let binary_op_mismatch = diagnostics.iter().find(|d| d.message.contains("Operator"));
    assert!(
        binary_op_mismatch.is_some(),
        "expected the `1 - \"x\"` mismatch inside the call to still be caught, got: {diagnostics:?}"
    );
    assert_eq!(binary_op_mismatch.unwrap().severity, Severity::Error);
}
