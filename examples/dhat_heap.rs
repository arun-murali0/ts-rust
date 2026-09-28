// Heap profile of the checker, one line of numbers per workload.
//
//     # all workloads, per-workload allocation deltas:
//     CARGO_PROFILE_RELEASE_DEBUG=true \
//       cargo run --release --example dhat_heap --features dhat-heap
//
//     # one workload only, so dhat-heap.json and the peak are about it alone:
//     CARGO_PROFILE_RELEASE_DEBUG=true \
//       cargo run --release --example dhat_heap --features dhat-heap -- class_hierarchy
//
// The optional argument is a substring of a workload label. Without the
// `dhat-heap` feature the same workloads run with no profiler and print only
// their diagnostic counts, which is the cheap way to check that a change to the
// arena did not change what the checker reports.
//
// Debug info (the CARGO_PROFILE_RELEASE_DEBUG above) is what lets dhat resolve
// stack frames to function names; release is opt-level "s" with no symbols.

use std::hint::black_box;
use std::process;

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

// `report_peak` is only meaningful when a single workload runs: dhat's peak is
// the peak of the whole run so far, not of one workload, so with several
// workloads it would just be the largest of them repeated on every line.
#[cfg_attr(not(feature = "dhat-heap"), allow(unused_variables))]
fn run_and_report(checker: &TypeChecker, label: &str, source: &str, report_peak: bool) {
    #[cfg(feature = "dhat-heap")]
    let before = dhat::HeapStats::get();

    let result = checker.check_source(black_box(source), "heap_profile.ts");

    // Read before `result` is dropped. The allocation totals only count
    // allocations, so dropping it later would not change them either way.
    #[cfg(feature = "dhat-heap")]
    let after = dhat::HeapStats::get();

    // Checking the count and not only `is_ok()`: a workload that starts
    // reporting different diagnostics is doing different work, and an arena
    // change that alters this number has changed behavior, not just speed.
    let diagnostics = match &result {
        Ok(checked) => checked.diagnostics.len(),
        Err(error) => panic!("heap workload `{label}` failed: {error:?}"),
    };

    #[cfg(feature = "dhat-heap")]
    {
        let blocks = after.total_blocks - before.total_blocks;
        let bytes = after.total_bytes - before.total_bytes;
        if report_peak {
            println!(
                "{label:<34} {blocks:>9} blocks {bytes:>12} bytes  peak {:>11} bytes  {diagnostics:>4} diagnostics",
                after.max_bytes
            );
        } else {
            println!(
                "{label:<34} {blocks:>9} blocks {bytes:>12} bytes  {diagnostics:>4} diagnostics"
            );
        }
    }
    #[cfg(not(feature = "dhat-heap"))]
    println!("{label:<34} {diagnostics:>4} diagnostics");

    let _ = black_box(result);
}

fn main() {
    let filter = std::env::args().nth(1);
    let checker = TypeChecker::new();

    // Every source is built up front, before the profiler starts, so generating
    // the fixtures is never counted as checker allocation.
    let workloads: Vec<(&str, String)> = vec![
        ("large_1000_functions", large_source(1_000)),
        (
            "wide_union_50_variants",
            wide_discriminated_union_source(50),
        ),
        ("nested_objects_depth_50", nested_object_source(50)),
        ("class_hierarchy_depth_50", class_hierarchy_source(50)),
        ("generic_calls_500", generic_heavy_source(500)),
        (
            "destructuring_500_bindings",
            destructuring_heavy_source(500),
        ),
        ("complex_mixed_scale_200", complex_source(200)),
        (
            "connected_application_scale_10",
            connected_application_source(10),
        ),
        (
            "connected_application_scale_50",
            connected_application_source(50),
        ),
        (
            "connected_application_scale_100",
            connected_application_source(100),
        ),
    ];

    let selected: Vec<&(&str, String)> = workloads
        .iter()
        .filter(|(label, _)| {
            filter
                .as_deref()
                .is_none_or(|wanted| label.contains(wanted))
        })
        .collect();

    if selected.is_empty() {
        eprintln!(
            "no workload label contains {:?}; available:",
            filter.unwrap_or_default()
        );
        for (label, _) in &workloads {
            eprintln!("  {label}");
        }
        process::exit(2);
    }

    let single = filter.is_some();

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    for (label, source) in selected {
        run_and_report(&checker, label, source, single);
    }
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
