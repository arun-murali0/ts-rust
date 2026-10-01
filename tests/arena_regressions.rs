use ts_rust::{Diagnostic, Severity, TypeChecker};

#[allow(dead_code)]
#[path = "../benches/support/complex_fixtures.rs"]
mod complex_fixtures;

fn check(source: &str, file_name: &str) -> Vec<Diagnostic> {
    let checker = TypeChecker::new();
    let result = checker.check_source(source, file_name);
    assert!(result.is_ok(), "source should at least parse: {result:?}");
    result
        .map(|checked| checked.diagnostics)
        .unwrap_or_default()
}

fn has_error(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

// tsc prints the primitive, "Type 'string' is not assignable to type 'number'",
// because an alias of a primitive is not preserved in messages. Here the alias
// must at least not leak onto every other `number`.
#[test]
fn alias_of_a_primitive_does_not_rename_the_primitive() {
    let source = "type Age = number;\nconst a: Age = 5;\nconst n: number = \"text\";\n";
    let diagnostics = check(source, "alias_of_primitive.ts");

    assert!(
        has_error(&diagnostics),
        "the mismatch must still be reported: {diagnostics:?}"
    );
    for diagnostic in &diagnostics {
        assert!(
            !diagnostic.message.contains("Age"),
            "plain `number` printed as the alias: {diagnostic:?}"
        );
    }
}

// An empty enum collapses to `never`, a fixed slot. Naming it names `never`.
#[test]
fn empty_enum_does_not_rename_never() {
    let source = "enum Empty {}\nconst n: never = 1;\n";
    let diagnostics = check(source, "empty_enum.ts");

    assert!(
        has_error(&diagnostics),
        "assigning 1 to never must still be reported: {diagnostics:?}"
    );
    for diagnostic in &diagnostics {
        assert!(
            !diagnostic.message.contains("Empty"),
            "`never` printed as the enum name: {diagnostic:?}"
        );
    }
}

// `A | B` goes through alloc_union, which dedups members with
// structurally_equal. A and B are identical recursive shapes, so that compares
// A -> B -> A -> B ... Must return, and this source has no type errors.
#[test]
fn identical_recursive_interfaces_in_a_union_terminates() {
    let source = "\
interface A { next: A | null; }
interface B { next: B | null; }
type Either = A | B;
function f(x: Either): number { return 0; }
";
    let diagnostics = check(source, "recursive_twins.ts");

    assert!(!has_error(&diagnostics), "got: {diagnostics:?}");
}

// Diagnostic counts measured with examples/dhat_heap.rs on the interning commit.
// The checker reports several diagnostics on these fixtures (wide union: 2 per
// variant; `first<T>` returning `items[0]`: 1; connected app: about 10 per unit),
// so the counts pin behavior. If one changes after an arena edit, look at the
// messages before looking at the timings.
//
// The connected_application counts were lowered by exactly one per unit (10 -> 9)
// when parenthesized expressions started being looked through (350d5b5). Each
// unit's `runBatch` ends in `return ( ... );`, which used to hit the catch-all in
// infer_expression_type and report one "unimplemented expression kind" warning.
// That warning is gone on purpose.
//
// They dropped again, to 2 per unit plus 1 from the shared prelude (21, 101 and 201
// at scales 10, 50 and 100), and complex_mixed_scale_200 from 205 to 5, when a
// property read on a union became legal whenever every member has the property.
// Before that, reading a shared field such as `kind` on an un-narrowed union was
// reported as missing, once per read. The wide_union_50 count went to 0 for the
// same reason. Print the remaining messages before trusting a new number.
#[test]
fn benchmark_fixtures_report_the_expected_number_of_diagnostics() {
    use complex_fixtures::*;

    let cases: Vec<(&str, String, usize)> = vec![
        (
            "wide_union_50_variants",
            wide_discriminated_union_source(50),
            0,
        ),
        ("nested_objects_depth_50", nested_object_source(50), 0),
        ("class_hierarchy_depth_50", class_hierarchy_source(50), 0),
        ("generic_calls_500", generic_heavy_source(500), 1),
        (
            "destructuring_500_bindings",
            destructuring_heavy_source(500),
            0,
        ),
        ("complex_mixed_scale_200", complex_source(200), 5),
        (
            "connected_application_scale_10",
            connected_application_source(10),
            21,
        ),
        (
            "connected_application_scale_50",
            connected_application_source(50),
            101,
        ),
        (
            "connected_application_scale_100",
            connected_application_source(100),
            201,
        ),
    ];

    let mismatches: Vec<String> = cases
        .iter()
        .filter_map(|(label, source, expected)| {
            let actual = check(source, "baseline.ts").len();
            (actual != *expected).then(|| format!("{label}: expected {expected}, got {actual}"))
        })
        .collect();

    assert!(
        mismatches.is_empty(),
        "diagnostic counts changed: {mismatches:#?}"
    );
}
