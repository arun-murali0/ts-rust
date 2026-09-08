#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn large_source(function_count: usize) -> String {
    let mut src = String::with_capacity(function_count * 80);
    for i in 0..function_count {
        src.push_str(&format!(
            "function fn{i}(a: number, b: number): number {{\n    return a + b;\n}}\n"
        ));
    }
    src
}

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let source = large_source(1_000);
    let checker = ts_rust::TypeChecker::new();
    let result = checker.check_source(&source, "dhat_large.ts");

    if let Err(error) = &result {
        eprintln!("dhat benchmark source failed to check: {error}");
    }
    let _ = std::hint::black_box(result);
}
