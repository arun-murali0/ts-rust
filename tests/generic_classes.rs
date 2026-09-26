use ts_rust::{Diagnostic, DiagnosticCode, Severity, TypeChecker};

fn check(fixture_source: &str, file_name: &str) -> Vec<Diagnostic> {
    let checker = TypeChecker::new();
    let result = checker.check_source(fixture_source, file_name);
    assert!(result.is_ok(), "fixture should at least parse: {result:?}");
    result
        .map(|checked| checked.diagnostics)
        .unwrap_or_default()
}

fn single_error(diagnostics: &[Diagnostic], code: DiagnosticCode) -> &Diagnostic {
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic, got: {diagnostics:?}"
    );
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].code, code, "got: {diagnostics:?}");
    &diagnostics[0]
}

// Before this, a class's own <T, ...> list was never shadowed the way an
// interface's or alias's is (namespace::resolve's type_params match returned
// None for every class), so `T` inside a class body just resolved as an
// unknown name, and Box<number> could never substitute anything.
#[test]
fn generic_class_field_is_substituted() {
    let source = include_str!("fixtures/generic-classes/generic_class_field_is_substituted.ts");
    let diagnostics = check(source, "generic_class_field_is_substituted.ts");
    single_error(&diagnostics, DiagnosticCode::ReturnTypeMismatch);
}

#[test]
fn generic_class_method_argument_is_checked() {
    let source =
        include_str!("fixtures/generic-classes/generic_class_method_argument_is_checked.ts");
    let diagnostics = check(source, "generic_class_method_argument_is_checked.ts");
    single_error(&diagnostics, DiagnosticCode::ArgumentNotAssignable);
}

// tsc: "Type 'Box<number>' is not assignable to type 'Box<string>'."
#[test]
fn two_instantiations_of_the_same_generic_class_stay_distinct() {
    let source = include_str!(
        "fixtures/generic-classes/two_instantiations_of_the_same_generic_class_stay_distinct.ts"
    );
    let diagnostics = check(
        source,
        "two_instantiations_of_the_same_generic_class_stay_distinct.ts",
    );
    let error = single_error(&diagnostics, DiagnosticCode::DeclaredTypeMismatch);
    assert!(error.message.contains("'Box<number>'"), "got: {error:?}");
    assert!(error.message.contains("'Box<string>'"), "got: {error:?}");
}

// The common recursive-generic-class shape: referring to itself with its own
// same type parameter. Same known limitation as the interface case (a
// self-reference with a *different*, fixed argument doesn't substitute
// correctly yet) applies here too and isn't exercised.
#[test]
fn generic_class_recursive_field_with_same_type_param_resolves() {
    let source = include_str!(
        "fixtures/generic-classes/generic_class_recursive_field_with_same_type_param_resolves.ts"
    );
    let diagnostics = check(
        source,
        "generic_class_recursive_field_with_same_type_param_resolves.ts",
    );
    single_error(&diagnostics, DiagnosticCode::ReturnTypeMismatch);
}

// tsc: "Generic type 'Box<T>' requires 1 type argument(s)." -- the existing
// count-check machinery in type_annotation.rs already worked off
// declared_type_param_arity/decl, which now cover classes too, so this needed
// no changes beyond the two namespace.rs edits.
#[test]
fn generic_class_type_argument_count_mismatch_is_reported() {
    let source = include_str!(
        "fixtures/generic-classes/generic_class_type_argument_count_mismatch_is_reported.ts"
    );
    let diagnostics = check(
        source,
        "generic_class_type_argument_count_mismatch_is_reported.ts",
    );
    single_error(&diagnostics, DiagnosticCode::TypeArgumentCountMismatch);
}
