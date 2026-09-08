use iai_callgrind::{library_benchmark, library_benchmark_group, main};
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

#[library_benchmark]
#[bench::tiny(tiny_source())]
#[bench::realistic_50_functions(realistic_source())]
#[bench::large_1000_functions(large_source(1_000))]
fn check_source(source: String) {
    let checker = TypeChecker::new();

    let result = checker.check_source(black_box(&source), "bench.ts");

    assert!(
        result.is_ok(),
        "benchmark source failed to check: {result:?}"
    );
    let _ = black_box(result);
}

library_benchmark_group!(
    name = checker_group;
    benchmarks = check_source
);

main!(library_benchmark_groups = checker_group);
