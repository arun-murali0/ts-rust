use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
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
        src.push_str(&format!(
            "function fn{i}(a: number, b: number): number {{\n    return a + b;\n}}\n"
        ));
    }
    src
}

fn bench_check_source(c: &mut Criterion) {
    let checker = TypeChecker::new();

    let tiny = tiny_source();
    c.bench_function("check_source/tiny", |b| {
        b.iter(|| {
            let result = checker.check_source(black_box(&tiny), "bench_tiny.ts");
            assert!(
                result.is_ok(),
                "benchmark source failed to check: {result:?}"
            );
            black_box(result)
        })
    });

    let realistic = realistic_source();
    c.bench_function("check_source/realistic_50_functions", |b| {
        b.iter(|| {
            let result = checker.check_source(black_box(&realistic), "bench_realistic.ts");
            assert!(
                result.is_ok(),
                "benchmark source failed to check: {result:?}"
            );
            black_box(result)
        })
    });

    let large = large_source(1_000);
    c.bench_function("check_source/large_1000_functions", |b| {
        b.iter(|| {
            let result = checker.check_source(black_box(&large), "bench_large.ts");
            assert!(
                result.is_ok(),
                "benchmark source failed to check: {result:?}"
            );
            black_box(result)
        })
    });
}

fn bench_parse_and_bind(c: &mut Criterion) {
    let large = large_source(1_000);
    c.bench_function("parse_and_bind_only/large_1000_functions", |b| {
        b.iter(|| {
            let result = ts_rust::parse_and_bind_only(black_box(&large), "bench_large.ts");
            assert!(
                result.is_ok(),
                "benchmark source failed to parse/bind: {result:?}"
            );
            black_box(result)
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_check_source, bench_parse_and_bind
}
criterion_main!(benches);
