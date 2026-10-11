use ts_rust::{
    CancelToken, Diagnostic, FileId, TypeChecker, UnitContext, UnitError, UnitOutput, Worker,
};

// The worker is the one place state outlives a unit, so these tests are about what has
// to match a one-shot check and what must not survive from one unit to the next.

fn unit<'a>(source: &'a str, file_name: &'a str) -> UnitContext<'a> {
    UnitContext::new(FileId::ROOT, file_name, source)
}

fn key(diagnostics: &[Diagnostic]) -> Vec<(String, u32, u32, String)> {
    diagnostics
        .iter()
        .map(|d| (d.code.to_string(), d.start, d.end, d.message.clone()))
        .collect()
}

fn check(worker: &mut Worker, source: &str, file_name: &str) -> UnitOutput {
    worker.check_unit(&unit(source, file_name)).unwrap()
}

#[test]
fn a_unit_reports_the_same_diagnostics_as_a_one_shot_check() {
    let source = "const n: number = 'x'; function f(a: string): number { return a; }";
    let one_shot = TypeChecker::new().check_source(source, "m.ts").unwrap();
    let from_unit = check(&mut Worker::new(), source, "m.ts");

    assert!(!one_shot.diagnostics.is_empty());
    assert_eq!(key(&one_shot.diagnostics), key(&from_unit.diagnostics));
}

#[test]
fn the_same_unit_gives_the_same_output_after_other_units() {
    let target =
        "const n: number = 'x'; interface Box<T> { value: T } const b: Box<string> = { value: 1 };";
    let other =
        "type Pair<A, B> = { a: A; b: B }; const p: Pair<number, string> = { a: 1, b: 'x' };";

    let mut worker = Worker::new();
    let first = check(&mut worker, target, "a.ts");
    check(&mut worker, other, "b.ts");
    check(&mut worker, other, "c.ts");
    let again = check(&mut worker, target, "a.ts");

    assert!(!first.diagnostics.is_empty());
    assert_eq!(key(&first.diagnostics), key(&again.diagnostics));
}

#[test]
fn declarations_from_one_unit_are_not_visible_to_the_next() {
    let declares = "interface Widget { size: number } const w: Widget = { size: 1 };";
    let uses = "const w: Widget = { size: 1 };";

    let fresh = check(&mut Worker::new(), uses, "uses.ts");
    assert!(
        !fresh.diagnostics.is_empty(),
        "Widget is unknown in a fresh worker"
    );

    let mut worker = Worker::new();
    let declared = check(&mut worker, declares, "declares.ts");
    assert!(declared.diagnostics.is_empty(), "{declared:?}");
    let after = check(&mut worker, uses, "uses.ts");

    assert_eq!(key(&fresh.diagnostics), key(&after.diagnostics));
}

#[test]
fn a_file_that_does_not_parse_is_an_error_and_leaves_the_worker_usable() {
    let mut worker = Worker::new();
    assert!(worker.check_unit(&unit("const = ;", "broken.ts")).is_err());

    let output = check(&mut worker, "const n: number = 1;", "ok.ts");
    assert!(output.diagnostics.is_empty(), "{output:?}");
}

#[test]
fn a_unit_keeps_the_file_id_it_was_given() {
    let mut worker = Worker::new();
    let ctx = UnitContext::new(FileId::new(5), "five.ts", "const n: number = 'x';");
    let output = worker.check_unit(&ctx).unwrap();

    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(output.diagnostics[0].file_name, "five.ts");
}

#[test]
fn a_unit_cancelled_before_it_starts_has_no_output() {
    let token = CancelToken::new();
    token.cancel();
    let mut worker = Worker::new();
    let ctx = unit("const n: number = 'x';", "late.ts").with_cancel(token);

    assert!(matches!(worker.check_unit(&ctx), Err(UnitError::Cancelled)));

    let after = check(&mut worker, "const n: number = 'x';", "late.ts");
    assert_eq!(after.diagnostics.len(), 1);
}

#[test]
fn a_token_that_is_never_raised_changes_nothing() {
    let source = "const n: number = 'x'; function f(a: string): number { return a; }";
    let mut worker = Worker::new();
    let plain = check(&mut worker, source, "m.ts");
    let ctx = unit(source, "m.ts").with_cancel(CancelToken::new());
    let with_token = worker.check_unit(&ctx).unwrap();

    assert!(!with_token.incomplete);
    assert_eq!(key(&plain.diagnostics), key(&with_token.diagnostics));
}

#[test]
fn a_finished_unit_is_not_incomplete() {
    let output = check(&mut Worker::new(), "const n: number = 1;", "ok.ts");
    assert!(!output.incomplete);
}

#[test]
fn diagnostics_come_out_ordered_by_start_then_code_then_message() {
    let source =
        "const a: number = 'x';\nfunction f(s: string): number { return s; }\nconst b: string = 1;";
    let output = check(&mut Worker::new(), source, "order.ts");

    let keys = key(&output.diagnostics);
    assert!(keys.len() >= 3, "{keys:?}");
    let mut sorted: Vec<_> = keys
        .iter()
        .map(|(code, start, _, message)| (*start, code.clone(), message.clone()))
        .collect();
    let unsorted = sorted.clone();
    sorted.sort();
    assert_eq!(unsorted, sorted);
}
