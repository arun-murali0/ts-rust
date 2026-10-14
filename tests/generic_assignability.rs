use ts_rust::{FileId, Severity, UnitContext, Worker};

// A generic function is instantiated against the signature it is compared with before
// the two are related, as tsc does, so `<T>(x: T) => T` fits `(x: number) => number`.

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

#[test]
fn generic_identity_is_accepted_as_an_argument_for_a_concrete_signature() {
    let found = errors(
        "declare function id<T>(x: T): T;\n\
         declare function apply(f: (x: number) => number): number;\n\
         apply(id);\n",
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn generic_function_is_accepted_when_assigned_to_a_concrete_annotation() {
    let found = errors(
        "declare function id<T>(x: T): T;\n\
         const g: (x: number) => number = id;\n",
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn instantiation_still_rejects_a_mismatched_return_type() {
    let found = errors(
        "declare function id<T>(x: T): T;\n\
         const bad: (x: number) => string = id;\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_constraint_that_the_target_violates_is_still_reported() {
    let found = errors(
        "declare function len<T extends string>(x: T): number;\n\
         const f: (x: number) => number = len;\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn parameter_unused_in_the_signature_does_not_block_assignment() {
    let found = errors(
        "declare function pick<T>(x: number): number;\n\
         const f: (x: number) => number = pick;\n",
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_target_parameter_of_any_instantiates_the_source_with_any() {
    let found = errors(
        "declare function first<T>(ts: T[]): T;\n\
         declare function run<R>(f: (c: any) => R): R;\n\
         const r: number = run<number>(first);\n",
    );
    assert!(found.is_empty(), "{found:?}");
}
