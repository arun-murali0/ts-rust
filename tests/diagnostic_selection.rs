use ts_rust::{FileId, Severity, UnitContext, Worker};

// The code and head line tsc chooses for a mismatch, not only whether a line is reported.

fn errors(source: &str) -> Vec<(Option<u32>, String)> {
    let ctx = UnitContext::new(FileId::ROOT, "a.ts", source);
    Worker::new()
        .check_unit(&ctx)
        .unwrap()
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.code.tsc_code(), d.message.to_string()))
        .collect()
}

// An element's missing property stays beneath the array-level mismatch, as in tsc.
#[test]
fn array_of_a_lacking_type_is_reported_at_the_array() {
    let found = errors(
        "interface I1 { a: number }\nclass C1 { a!: number; m!: string }\n\
         declare var i: I1[];\nvar c: C1[] = i;\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Some(2322), "{found:?}");
    assert!(
        found[0]
            .1
            .starts_with("Type 'I1[]' is not assignable to type 'C1[]'."),
        "{found:?}"
    );
}

// A plain object still gets the missing-property head.
#[test]
fn object_lacking_a_property_is_reported_as_missing_property() {
    let found = errors("class C1 { a!: number; m!: string }\nvar o: C1 = { a: 1 };\n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Some(2741), "{found:?}");
}

// A type used where a value is expected is TS2693, an unknown name is still TS2304.
#[test]
fn type_used_as_a_value_is_ts2693() {
    let found = errors(
        "interface I { a: number }\ntype A = { b: string };\nconst x = I;\nconst y = A;\nconst z = Iterator;\nconst w = Nope;\n",
    );
    let codes: Vec<_> = found.iter().map(|(code, _)| *code).collect();
    assert_eq!(
        codes,
        [Some(2693), Some(2693), Some(2693), Some(2304)],
        "{found:?}"
    );
}
