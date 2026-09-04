//! Performance harness. Run with `cargo bench`.
//!
//! Benchmarks the full public API end-to-end (parse + declare + check)
//! rather than internal functions in isolation, since that's the latency
//! that matters to a caller. Three sizes are covered so a regression in
//! one pass doesn't hide behind aggregate numbers: a tiny file (per-call
//! overhead), a realistic file (many small functions), and a large
//! synthetic file (surfaces non-linear scaling early). A fourth benchmark,
//! `parse_and_bind_only`, isolates just the Oxc parse + bind cost on the
//! large input, so you can tell how much of total time is Oxc's own cost
//! versus this crate's declare/check passes.
//!
//! Flamegraphs (Linux only): `cargo bench -- --profile-time=10` samples
//! each benchmark for 10 seconds with `pprof` instead of doing a normal
//! statistical run, and writes `target/criterion/<name>/profile/flamegraph.svg`
//! per benchmark. Needs `perf_event_paranoid` set low enough to read
//! performance counters as your user; on most distros:
//! `echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid`. See
//! docs/VERIFICATION.md for the full walkthrough.

use criterion::profiler::Profiler;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use ts_rust::TypeChecker;

fn tiny_source() -> String {
    "const x: number = 5;".to_string()
}

fn realistic_source() -> String {
    let mut src = String::new();
    for i in 0..50 {
        src.push_str(&format!(
            "function add{i}(a: number, b: number): number {{\n    return a + b;\n}}\nadd{i}(1, 2);\n"
        ));
    }
    src
}

fn large_source(function_count: usize) -> String {
    let mut src = String::with_capacity(function_count * 80);
    for i in 0..function_count {
        src.push_str(&format!("function fn{i}(a: number, b: number): number {{\n    return a + b;\n}}\n"));
    }
    src
}

fn bench_check_source(c: &mut Criterion) {
    let checker = TypeChecker::new();

    let tiny = tiny_source();
    c.bench_function("check_source/tiny", |b| {
        b.iter(|| checker.check_source(black_box(&tiny), "bench_tiny.ts").unwrap())
    });

    let realistic = realistic_source();
    c.bench_function("check_source/realistic_50_functions", |b| {
        b.iter(|| checker.check_source(black_box(&realistic), "bench_realistic.ts").unwrap())
    });

    let large = large_source(1_000);
    c.bench_function("check_source/large_1000_functions", |b| {
        b.iter(|| checker.check_source(black_box(&large), "bench_large.ts").unwrap())
    });
}

/// Isolates parse + `oxc_semantic` bind from declare/check, on the same
/// large input `bench_check_source` uses. Compared side by side with
/// `check_source/large_1000_functions`, the gap between the two tells you
/// how much of total time is Oxc's own cost versus this crate's
/// declare/check passes, which is exactly the split a flamegraph of the
/// full `check_source` run can't show you on its own (it's all in one
/// call stack, not separated into "before" and "after").
fn bench_parse_and_bind(c: &mut Criterion) {
    let large = large_source(1_000);
    c.bench_function("parse_and_bind_only/large_1000_functions", |b| {
        b.iter(|| ts_rust::parse_and_bind_only(black_box(&large), "bench_large.ts").unwrap())
    });
}

fn flamegraph_profiler() -> impl Profiler {
    // 100 Hz sampling rate: fine-grained enough to see individual pass
    // boundaries (parse vs declare vs check) in the tiny/realistic cases
    // without generating an unreadably large profile on the 1000-function
    // case.
    PProfProfiler::new(100, Output::Flamegraph(None))
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(flamegraph_profiler());
    targets = bench_check_source, bench_parse_and_bind
}
criterion_main!(benches);
