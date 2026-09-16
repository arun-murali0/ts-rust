use std::hint::black_box;
use ts_rust::TypeChecker;

#[path = "../benches/support/complex_fixtures.rs"]
mod complex_fixtures;

use complex_fixtures::{
    class_hierarchy_source, complex_source, connected_application_source,
    destructuring_heavy_source, generic_heavy_source, nested_object_source,
    wide_discriminated_union_source,
};

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn run_and_report(checker: &TypeChecker, label: &str, source: &str) {
    let result = checker.check_source(black_box(source), "heap_profile.ts");

    if let Err(error) = &result {
        eprintln!("heap workload `{label}` failed: {error:?}");
    }

    assert!(result.is_ok(), "heap workload `{label}` failed: {result:?}");

    let _ = black_box(result);
}

fn main() {
    let checker = TypeChecker::new();

    let large = large_source(1_000);
    let wide_union = wide_discriminated_union_source(50);
    let nested_objects = nested_object_source(50);
    let class_hierarchy = class_hierarchy_source(50);
    let generic_calls = generic_heavy_source(500);
    let destructuring = destructuring_heavy_source(500);
    let complex_200 = complex_source(200);

    let connected_10 = connected_application_source(10);
    let connected_50 = connected_application_source(50);
    let connected_100 = connected_application_source(100);

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    run_and_report(&checker, "large_1000_functions", &large);
    run_and_report(&checker, "wide_union_50_variants", &wide_union);
    run_and_report(&checker, "nested_objects_depth_50", &nested_objects);
    run_and_report(&checker, "class_hierarchy_depth_50", &class_hierarchy);
    run_and_report(&checker, "generic_calls_500", &generic_calls);
    run_and_report(&checker, "destructuring_500_bindings", &destructuring);
    run_and_report(&checker, "complex_mixed_scale_200", &complex_200);

    run_and_report(&checker, "connected_application_scale_10", &connected_10);

    run_and_report(&checker, "connected_application_scale_50", &connected_50);

    run_and_report(&checker, "connected_application_scale_100", &connected_100);
}

fn large_source(function_count: usize) -> String {
    let mut src = String::with_capacity(function_count * 80);

    for i in 0..function_count {
        src.push_str(&format!(
            "function fn{i}(a: number, b: number): number {{\n\
             \treturn a + b;\n\
             }}\n"
        ));
    }

    src
}
