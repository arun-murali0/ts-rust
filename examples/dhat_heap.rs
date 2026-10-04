// Heap profile of the checker, one line of numbers per workload.
//
//     # all workloads, per-workload allocation deltas:
//     CARGO_PROFILE_RELEASE_DEBUG=true \
//       cargo run --release --example dhat_heap --features dhat-heap
//
//     # the same, plus the multi-file project workloads:
//     CARGO_PROFILE_RELEASE_DEBUG=true \
//       cargo run --release --example dhat_heap --features dhat-heap,module-resolution
//
//     # plus the foundation layers (petgraph topology and the bump scratchpad):
//     CARGO_PROFILE_RELEASE_DEBUG=true \
//       cargo run --release --example dhat_heap --features dhat-heap,module-resolution,scratchpad
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
// With `module-resolution` there are also project workloads, which write a project
// to the temp directory and report two lines each: `<label>/graph` for discovering and
// resolving every import, and `<label>/check` for checking every file in parallel.
// dhat counts allocations on every thread, so a `/check` line is the whole pool's
// total and its peak is the pool's combined peak, not one file's.
//
// Debug info (the CARGO_PROFILE_RELEASE_DEBUG above) is what lets dhat resolve
// stack frames to function names; the release profile is otherwise stripped of
// symbols for the frames' sake.

use std::hint::black_box;
use std::process;

use ts_rust::TypeChecker;

#[path = "../benches/support/complex_fixtures.rs"]
mod complex_fixtures;

#[cfg(feature = "module-resolution")]
#[path = "../benches/support/project_fixtures.rs"]
mod project_fixtures;

use complex_fixtures::{
    class_hierarchy_source, complex_source, connected_application_source,
    destructuring_heavy_source, generic_heavy_source, generic_type_reference_source,
    nested_object_source, wide_discriminated_union_source,
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
    print_stats(label, &before, &after, report_peak, diagnostics);
    #[cfg(not(feature = "dhat-heap"))]
    println!("{label:<40} {diagnostics:>4} diagnostics");

    let _ = black_box(result);
}

#[cfg(feature = "dhat-heap")]
fn print_stats(
    label: &str,
    before: &dhat::HeapStats,
    after: &dhat::HeapStats,
    report_peak: bool,
    diagnostics: usize,
) {
    let blocks = after.total_blocks - before.total_blocks;
    let bytes = after.total_bytes - before.total_bytes;
    if report_peak {
        println!(
            "{label:<40} {blocks:>9} blocks {bytes:>12} bytes  peak {:>11} bytes  {diagnostics:>4} diagnostics",
            after.max_bytes
        );
    } else {
        println!("{label:<40} {blocks:>9} blocks {bytes:>12} bytes  {diagnostics:>4} diagnostics");
    }
}

// Graph discovery and the project check are measured separately, so a change that moves
// allocation from one to the other shows up. The resolver is created before the first
// reading and dropped after the last, so the resolver's own caches belong to `/graph`.
#[cfg(feature = "module-resolution")]
#[cfg_attr(not(feature = "dhat-heap"), allow(unused_variables))]
fn run_project_and_report(label: &str, project: &project_fixtures::Project, report_peak: bool) {
    use ts_rust::{ModuleGraph, ModuleResolver, check_project};

    let resolver = ModuleResolver::new();

    #[cfg(feature = "dhat-heap")]
    let before_graph = dhat::HeapStats::get();

    let graph = match ModuleGraph::build(black_box(&project.entries), &resolver) {
        Ok(graph) => graph,
        Err(error) => panic!("heap workload `{label}` could not build its graph: {error}"),
    };

    #[cfg(feature = "dhat-heap")]
    let after_graph = dhat::HeapStats::get();

    let report = check_project(black_box(&graph));

    #[cfg(feature = "dhat-heap")]
    let after_check = dhat::HeapStats::get();

    // A file that could not be read or checked is not a diagnostic, so it is counted on
    // its own: a project workload that starts failing is not doing the same work.
    assert_eq!(
        report.failure_count(),
        0,
        "heap workload `{label}` had files that failed to check"
    );
    let diagnostics = report.diagnostic_count();
    let files = graph.len();

    #[cfg(feature = "dhat-heap")]
    {
        let graph_label = format!("{label}/graph ({files} files)");
        print_stats(&graph_label, &before_graph, &after_graph, report_peak, 0);
        let check_label = format!("{label}/check");
        print_stats(
            &check_label,
            &after_graph,
            &after_check,
            report_peak,
            diagnostics,
        );
    }
    #[cfg(not(feature = "dhat-heap"))]
    println!("{label:<40} {diagnostics:>4} diagnostics ({files} files)");

    let _ = black_box(report);
}

// Labels of the foundation workloads below, so a filter that names only one of them is
// not mistaken for "no workload matches". Empty without the features that build them.
const FOUNDATION_LABELS: &[&str] = &[
    #[cfg(feature = "module-resolution")]
    "topology_layered_201_files",
    #[cfg(feature = "module-resolution")]
    "topology_ring_50_files",
    #[cfg(feature = "scratchpad")]
    "scratchpad_1000_values",
    #[cfg(feature = "scratchpad")]
    "vec_1000_values",
];

// Allocations made by `work`, for the foundation layers, which are measured as one step
// each and have no diagnostics to report.
#[cfg(any(feature = "module-resolution", feature = "scratchpad"))]
fn measure<R>(label: &str, work: impl FnOnce() -> R) -> R {
    #[cfg(feature = "dhat-heap")]
    let before = dhat::HeapStats::get();

    let out = black_box(work());

    #[cfg(feature = "dhat-heap")]
    {
        let after = dhat::HeapStats::get();
        println!(
            "{label:<40} {:>9} blocks {:>12} bytes",
            after.total_blocks - before.total_blocks,
            after.total_bytes - before.total_bytes
        );
    }
    #[cfg(not(feature = "dhat-heap"))]
    println!("{label:<40} ran");

    out
}

// The petgraph topology and the worker scratchpad, next to the plain-`Vec` and
// hand-written-search code they are meant to compare with. Fixtures are built before the
// measured step, so only the step itself is counted.
fn run_foundation(wanted: &dyn Fn(&str) -> bool) {
    let _ = wanted;

    #[cfg(feature = "module-resolution")]
    {
        use ts_rust::{ModuleGraph, ModuleResolver};

        if wanted("topology_layered_201_files") {
            let project = project_fixtures::Project::new("heap-topology").layered(20, 10, 3);
            let graph = ModuleGraph::build(&project.entries, &ModuleResolver::new())
                .expect("heap workload graph should build");
            let topology = measure("topology_layered_201_files/from_graph", || graph.topology());
            measure("topology_layered_201_files/components", || {
                topology.components()
            });
        }

        if wanted("topology_ring_50_files") {
            let project = project_fixtures::Project::new("heap-topology-ring").ring(50);
            let graph = ModuleGraph::build(&project.entries, &ModuleResolver::new())
                .expect("heap workload graph should build");
            let topology = graph.topology();
            measure("topology_ring_50_files/cycles_graph", || graph.cycles());
            measure("topology_ring_50_files/cycles_petgraph", || {
                topology.cycles()
            });
        }
    }

    #[cfg(feature = "scratchpad")]
    {
        use ts_rust::WorkerScratch;

        if wanted("scratchpad_1000_values") {
            let mut scratch = WorkerScratch::with_capacity(16 * 1024);
            measure("scratchpad_1000_values/first_job", || {
                for value in 0..1_000u32 {
                    black_box(scratch.alloc(value));
                }
            });
            scratch.reset();
            // After a reset the same memory is reused, so this job should allocate
            // nothing from the heap.
            measure("scratchpad_1000_values/after_reset", || {
                for value in 0..1_000u32 {
                    black_box(scratch.alloc(value));
                }
            });
        }

        if wanted("vec_1000_values") {
            measure("vec_1000_values", || {
                let mut values = Vec::new();
                for value in 0..1_000u32 {
                    values.push(black_box(value));
                }
                values
            });
        }
    }
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
        ("generic_type_refs_500", generic_type_reference_source(500)),
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

    // Written to disk up front for the same reason. Each project removes its directory
    // when it is dropped at the end of `main`.
    #[cfg(feature = "module-resolution")]
    let projects: Vec<(&str, project_fixtures::Project)> = vec![
        (
            "project_layered_201_files",
            project_fixtures::Project::new("heap-layered").layered(20, 10, 3),
        ),
        (
            "project_independent_200_files",
            project_fixtures::Project::new("heap-independent").independent(200, 20),
        ),
        (
            "project_generic_200_files",
            project_fixtures::Project::new("heap-generic").independent_generic(200, 10),
        ),
        (
            "project_ring_50_files",
            project_fixtures::Project::new("heap-ring").ring(50),
        ),
        (
            "project_chain_100_files",
            project_fixtures::Project::new("heap-chain").chain(100),
        ),
    ];

    let wanted = |label: &str| {
        filter
            .as_deref()
            .is_none_or(|wanted| label.contains(wanted))
    };

    let selected: Vec<&(&str, String)> = workloads
        .iter()
        .filter(|(label, _)| wanted(label))
        .collect();

    #[cfg(feature = "module-resolution")]
    let selected_projects: Vec<&(&str, project_fixtures::Project)> =
        projects.iter().filter(|(label, _)| wanted(label)).collect();
    #[cfg(not(feature = "module-resolution"))]
    let selected_projects: Vec<&(&str, ())> = Vec::new();

    let foundation_selected = FOUNDATION_LABELS.iter().any(|label| wanted(label));

    if selected.is_empty() && selected_projects.is_empty() && !foundation_selected {
        eprintln!(
            "no workload label contains {:?}; available:",
            filter.unwrap_or_default()
        );
        for (label, _) in &workloads {
            eprintln!("  {label}");
        }
        #[cfg(feature = "module-resolution")]
        for (label, _) in &projects {
            eprintln!("  {label}");
        }
        for label in FOUNDATION_LABELS {
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

    #[cfg(feature = "module-resolution")]
    for (label, project) in selected_projects {
        run_project_and_report(label, project, single);
    }
    #[cfg(not(feature = "module-resolution"))]
    let _ = selected_projects;

    run_foundation(&wanted);
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
