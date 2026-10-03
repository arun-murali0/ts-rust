#![cfg(feature = "module-resolution")]

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ts_rust::{
    FileId, FileOutcome, ModuleEdge, ModuleError, ModuleGraph, ModuleResolver, check_project,
};

// Module resolution and the module graph. The fixtures are small projects on disk under
// tests/module-resolution-fixtures/, not under tests/fixtures/: they are directory
// trees whose imports are meant to resolve, and the tsc comparison harness checks every
// .ts file under tests/fixtures/ on its own, where those imports would all be errors.

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/module-resolution-fixtures")
        .join(relative)
}

fn build(entries: &[&str]) -> ModuleGraph {
    let entries: Vec<PathBuf> = entries.iter().map(|entry| fixture(entry)).collect();
    ModuleGraph::build(&entries, &ModuleResolver::new()).expect("fixture graph should build")
}

fn id_of(graph: &ModuleGraph, relative: &str) -> FileId {
    graph
        .files()
        .id(&fixture(relative))
        .unwrap_or_else(|| panic!("{relative} is not in the graph"))
}

fn edge<'a>(graph: &'a ModuleGraph, from: &str, specifier: &str) -> &'a ModuleEdge {
    graph
        .edges(id_of(graph, from))
        .iter()
        .find(|edge| edge.specifier == specifier)
        .unwrap_or_else(|| panic!("{from} has no edge for `{specifier}`"))
}

fn target_path(graph: &ModuleGraph, from: &str, specifier: &str) -> PathBuf {
    let target = edge(graph, from, specifier)
        .target
        .unwrap_or_else(|| panic!("`{specifier}` in {from} did not resolve"));
    graph
        .files()
        .path(target)
        .expect("a resolved target has a path")
        .to_path_buf()
}

fn names(graph: &ModuleGraph, ids: &[FileId]) -> Vec<String> {
    ids.iter()
        .map(|id| {
            graph
                .files()
                .path(*id)
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .expect("every id has a file name")
        })
        .collect()
}

#[test]
fn an_extensionless_relative_specifier_resolves_to_the_source_file() {
    let graph = build(&["basic/main.ts"]);
    assert_eq!(
        target_path(&graph, "basic/main.ts", "./math"),
        fixture("basic/math.ts")
    );
}

#[test]
fn a_directory_specifier_resolves_to_its_index_file() {
    let graph = build(&["basic/main.ts"]);
    assert_eq!(
        target_path(&graph, "basic/main.ts", "./shapes"),
        fixture("basic/shapes/index.ts")
    );
}

#[test]
fn a_js_specifier_resolves_to_the_typescript_file_next_to_it() {
    let graph = build(&["basic/main.ts"]);
    assert_eq!(
        target_path(&graph, "basic/main.ts", "./legacy.js"),
        fixture("basic/legacy.ts")
    );
}

#[test]
fn a_type_only_import_is_marked_and_resolved() {
    let graph = build(&["basic/main.ts"]);
    let import = edge(&graph, "basic/main.ts", "./point");
    assert!(import.is_type);
    assert!(import.is_import);
    assert_eq!(
        target_path(&graph, "basic/main.ts", "./point"),
        fixture("basic/point.ts")
    );
    assert!(!edge(&graph, "basic/main.ts", "./math").is_type);
}

#[test]
fn a_re_export_is_an_edge_that_binds_nothing() {
    let graph = build(&["basic/main.ts"]);
    let re_export = edge(&graph, "basic/main.ts", "./reexported");
    assert!(!re_export.is_import);
    assert_eq!(
        target_path(&graph, "basic/main.ts", "./reexported"),
        fixture("basic/reexported.ts")
    );
}

#[test]
fn an_unresolved_import_stays_as_an_edge_without_a_target() {
    let graph = build(&["basic/main.ts"]);
    assert_eq!(edge(&graph, "basic/main.ts", "./missing").target, None);

    let unresolved: Vec<_> = graph
        .unresolved()
        .map(|(from, edge)| (from, edge.specifier.as_str()))
        .collect();
    assert_eq!(
        unresolved,
        vec![(id_of(&graph, "basic/main.ts"), "./missing")]
    );
    // The file is not mistaken for one with fewer imports: all six statements are there.
    assert_eq!(graph.edges(id_of(&graph, "basic/main.ts")).len(), 6);
}

#[test]
fn package_exports_prefer_the_types_condition() {
    let graph = build(&["package-exports/app.ts"]);
    assert_eq!(
        target_path(&graph, "package-exports/app.ts", "widget"),
        fixture("package-exports/node_modules/widget/types/index.d.ts")
    );
}

#[test]
fn package_exports_subpaths_resolve() {
    let graph = build(&["package-exports/app.ts"]);
    assert_eq!(
        target_path(&graph, "package-exports/app.ts", "widget/gadget"),
        fixture("package-exports/node_modules/widget/types/gadget.d.ts")
    );
}

#[test]
fn a_hash_specifier_uses_the_nearest_package_imports_field() {
    let graph = build(&["package-imports/src/main.ts"]);
    assert_eq!(
        target_path(&graph, "package-imports/src/main.ts", "#internal/helper"),
        fixture("package-imports/src/internal/helper.ts")
    );
}

#[test]
fn a_nested_node_modules_package_shadows_the_hoisted_one() {
    let graph = build(&["nested-node-modules/app.ts"]);

    let from_app = target_path(&graph, "nested-node-modules/app.ts", "b");
    assert_eq!(
        from_app,
        fixture("nested-node-modules/node_modules/b/index.d.ts")
    );

    let from_a = target_path(&graph, "nested-node-modules/node_modules/a/index.d.ts", "b");
    assert_eq!(
        from_a,
        fixture("nested-node-modules/node_modules/a/node_modules/b/index.d.ts")
    );
}

#[test]
fn dependents_mirror_dependencies() {
    let graph = build(&["cycles/entry.ts"]);
    let entry = id_of(&graph, "cycles/entry.ts");
    let ring_a = id_of(&graph, "cycles/ring_a.ts");
    let ring_c = id_of(&graph, "cycles/ring_c.ts");

    assert_eq!(graph.dependencies(entry), [ring_a]);
    // ring_a is imported by the entry and by ring_c, which closes the ring.
    assert_eq!(graph.dependents(ring_a), [entry, ring_c]);
}

#[test]
fn a_ring_is_one_cycle_and_its_importer_is_not_part_of_it() {
    let graph = build(&["cycles/entry.ts"]);
    let cycles = graph.cycles();
    assert_eq!(cycles.len(), 1);
    assert_eq!(
        names(&graph, &cycles[0]),
        ["ring_a.ts", "ring_b.ts", "ring_c.ts"]
    );
}

#[test]
fn a_file_that_imports_itself_is_a_cycle() {
    let graph = build(&["self-import/self_import.ts"]);
    assert_eq!(graph.cycles(), vec![vec![FileId::new(0)]]);
}

#[test]
fn layers_run_dependencies_first_and_keep_a_cycle_together() {
    let graph = build(&["cycles/entry.ts"]);
    let layers: Vec<Vec<String>> = graph
        .layers()
        .iter()
        .map(|layer| names(&graph, layer))
        .collect();
    assert_eq!(
        layers,
        vec![
            vec!["leaf.ts"],
            vec!["ring_a.ts", "ring_b.ts", "ring_c.ts"],
            vec!["entry.ts"],
        ]
    );
}

#[test]
fn a_relative_entry_is_rejected() {
    let result = ModuleGraph::build(&[PathBuf::from("main.ts")], &ModuleResolver::new());
    assert!(matches!(result, Err(ModuleError::RelativeEntry(_))));
}

#[test]
fn a_missing_entry_is_a_read_error() {
    let result = ModuleGraph::build(&[fixture("basic/absent.ts")], &ModuleResolver::new());
    assert!(matches!(result, Err(ModuleError::Read { .. })));
}

#[test]
fn the_report_names_every_file_including_the_ones_that_could_not_be_checked() {
    let graph = build(&[
        "project-check/clean.ts",
        "project-check/type_error.ts",
        "project-check/syntax_error.ts",
    ]);
    assert!(graph.has_syntax_errors(id_of(&graph, "project-check/syntax_error.ts")));

    let report = check_project(&graph);
    assert_eq!(report.files.len(), 3);

    let ids: Vec<u32> = report
        .files
        .iter()
        .map(|file| file.file_id.index())
        .collect();
    assert_eq!(ids, [0, 1, 2], "reports are sorted by file id");

    assert!(matches!(
        &report.files[0].outcome,
        FileOutcome::Checked(diagnostics) if diagnostics.is_empty()
    ));
    assert!(matches!(
        &report.files[1].outcome,
        FileOutcome::Checked(diagnostics) if !diagnostics.is_empty()
    ));
    assert!(matches!(
        &report.files[2].outcome,
        FileOutcome::CheckFailed(_)
    ));
    assert_eq!(report.failure_count(), 1);
    assert!(report.diagnostic_count() >= 1);
}

#[test]
fn dependency_declarations_are_in_the_graph_but_are_not_checked() {
    let graph = build(&["package-exports/app.ts"]);
    let declaration = id_of(
        &graph,
        "package-exports/node_modules/widget/types/index.d.ts",
    );
    assert!(!graph.is_checkable(declaration));

    let report = check_project(&graph);
    assert_eq!(report.files.len(), 1);
    assert_eq!(
        report.files[0].file_id,
        id_of(&graph, "package-exports/app.ts")
    );
}

// A scratch project for the tests that have to change files. Each test gets its own
// directory so they can run in parallel.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ts-rust-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory should be creatable");
    dir
}

// Moves a file's modification time ahead, so the test does not depend on how fine the
// filesystem's clock is.
fn push_mtime_forward(path: &Path, seconds: u64) {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("file should open for writing");
    file.set_modified(SystemTime::now() + Duration::from_secs(seconds))
        .expect("modification time should be settable");
}

#[test]
fn a_rewrite_with_the_same_bytes_is_not_a_change_but_new_bytes_are() {
    let dir = scratch_dir("changed-files");
    let path = dir.join("a.ts");
    fs::write(&path, "const a: number = 1;").expect("write should succeed");

    let graph = ModuleGraph::build(std::slice::from_ref(&path), &ModuleResolver::new())
        .expect("scratch graph should build");
    assert!(graph.changed_files().is_empty());

    push_mtime_forward(&path, 60);
    assert!(
        graph.changed_files().is_empty(),
        "a new timestamp over the same bytes is not a change"
    );

    // Same length, different content, and a timestamp that differs from both before.
    fs::write(&path, "const a: number = 2;").expect("write should succeed");
    push_mtime_forward(&path, 120);
    assert_eq!(graph.changed_files(), vec![FileId::new(0)]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_file_counts_as_changed() {
    let dir = scratch_dir("deleted-file");
    let path = dir.join("a.ts");
    fs::write(&path, "const a: number = 1;").expect("write should succeed");

    let graph = ModuleGraph::build(std::slice::from_ref(&path), &ModuleResolver::new())
        .expect("scratch graph should build");
    fs::remove_file(&path).expect("remove should succeed");
    assert_eq!(graph.changed_files(), vec![FileId::new(0)]);

    let _ = fs::remove_dir_all(&dir);
}
