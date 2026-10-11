#![cfg(feature = "module-resolution")]

use std::fs;
use std::path::{Path, PathBuf};

use ts_rust::{Diagnostic, FileOutcome, ModuleGraph, ModuleResolver, Severity, check_project};

// Step 6c: check_project passes what each file exports to the files that import it.
// Every test writes a small project into its own directory under the target tree, builds
// the graph with a root (so files get stable keys) and checks it, which is the path the
// command line takes.

struct Checked {
    graph: ModuleGraph,
    report: ts_rust::ProjectReport,
}

fn write(test: &str, files: &[(&str, &str)]) -> (PathBuf, Vec<PathBuf>) {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("project-imports-{test}"));
    let _ = fs::remove_dir_all(&root);
    let mut paths = Vec::new();
    for (name, source) in files {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        paths.push(path);
    }
    (root, paths)
}

fn run(test: &str, files: &[(&str, &str)]) -> Checked {
    let (root, paths) = write(test, files);
    let graph = ModuleGraph::build(&paths, &ModuleResolver::new())
        .expect("the project builds")
        .with_root(root);
    let report = check_project(&graph);
    Checked { graph, report }
}

impl Checked {
    fn diagnostics(&self, file: &str) -> &[Diagnostic] {
        for report in &self.report.files {
            let path = self.graph.files().path(report.file_id).unwrap();
            if path.file_name().unwrap().to_string_lossy() == file {
                return match &report.outcome {
                    FileOutcome::Checked(d) => d,
                    other => panic!("{file} was not checked: {other:?}"),
                };
            }
        }
        panic!("{file} is not in the report");
    }

    fn errors(&self, file: &str) -> Vec<&Diagnostic> {
        self.diagnostics(file)
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    fn all_clean(&self) {
        for report in &self.report.files {
            if let FileOutcome::Checked(d) = &report.outcome {
                assert!(d.is_empty(), "{d:?}");
            }
        }
    }
}

const USER: &str = "export interface User { name: string }";

#[test]
fn an_imported_interface_misused_is_reported_as_ts2322_in_the_importing_file() {
    // The first fixture the plan named: a.ts exports an interface, b.ts misuses it.
    let project = run(
        "first-fixture",
        &[
            ("a.ts", USER),
            (
                "b.ts",
                "import { User } from './a'; const u: User = { name: 1 };",
            ),
        ],
    );
    assert!(project.diagnostics("a.ts").is_empty());
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code.tsc_code(), Some(2322));
}

#[test]
fn a_correct_use_of_an_import_is_clean() {
    run(
        "correct-use",
        &[
            ("a.ts", USER),
            (
                "b.ts",
                "import { User } from './a'; const u: User = { name: 'a' }; const n: string = u.name;",
            ),
        ],
    )
    .all_clean();
}

#[test]
fn types_flow_through_a_chain_of_files() {
    let project = run(
        "chain",
        &[
            ("a.ts", USER),
            ("b.ts", "export { User } from './a';"),
            (
                "c.ts",
                "import { User } from './b'; const u: User = { name: 1 };",
            ),
        ],
    );
    assert!(project.diagnostics("b.ts").is_empty());
    assert_eq!(project.errors("c.ts").len(), 1);
}

#[test]
fn a_function_signature_crosses_files() {
    let project = run(
        "function",
        &[
            (
                "math.ts",
                "export function add(a: number, b: number): number { return a + b; }",
            ),
            (
                "app.ts",
                "import { add } from './math'; const n: number = add(1, 2); const s: string = add(1, 2); add(1);",
            ),
        ],
    );
    assert_eq!(
        project.errors("app.ts").len(),
        2,
        "{:?}",
        project.diagnostics("app.ts")
    );
}

#[test]
fn a_generic_crosses_files_and_is_instantiated_there() {
    let project = run(
        "generic",
        &[
            ("box.ts", "export interface Box<T> { value: T }"),
            (
                "app.ts",
                "import { Box } from './box';
                 const n: Box<number> = { value: 1 };
                 const s: Box<string> = { value: 'x' };
                 const bad: Box<number> = { value: 'x' };",
            ),
        ],
    );
    assert_eq!(
        project.errors("app.ts").len(),
        1,
        "{:?}",
        project.diagnostics("app.ts")
    );
}

#[test]
fn a_diamond_of_imports_checks_every_file_against_its_dependencies() {
    let project = run(
        "diamond",
        &[
            ("base.ts", USER),
            (
                "left.ts",
                "import { User } from './base'; export function left(u: User): string { return u.name; }",
            ),
            (
                "right.ts",
                "import { User } from './base'; export function right(u: User): number { return 1; }",
            ),
            (
                "top.ts",
                "import { User } from './base';
                 import { left } from './left';
                 import { right } from './right';
                 const u: User = { name: 'a' };
                 const a: string = left(u);
                 const b: number = right(u);
                 const wrong: number = left(u);",
            ),
        ],
    );
    assert_eq!(
        project.errors("top.ts").len(),
        1,
        "{:?}",
        project.diagnostics("top.ts")
    );
    assert!(project.diagnostics("left.ts").is_empty());
    assert!(project.diagnostics("right.ts").is_empty());
}

#[test]
fn imports_resolve_through_directories_and_parent_paths() {
    let project = run(
        "paths",
        &[
            ("shared/types.ts", USER),
            ("shared/index.ts", "export { User } from './types';"),
            (
                "app/main.ts",
                "import { User } from '../shared'; const u: User = { name: 1 };",
            ),
        ],
    );
    assert_eq!(
        project.errors("main.ts").len(),
        1,
        "{:?}",
        project.diagnostics("main.ts")
    );
}

#[test]
fn an_import_of_a_file_that_is_not_there_gives_no_false_mismatch() {
    let project = run(
        "missing",
        &[(
            "b.ts",
            "import { User } from './nowhere'; const u: User = { name: 1 };",
        )],
    );
    assert!(
        project
            .errors("b.ts")
            .iter()
            .all(|d| !d.message.contains("not assignable")),
        "{:?}",
        project.diagnostics("b.ts")
    );
}

#[test]
fn a_cycle_is_checked_without_looping_and_without_false_errors() {
    let project = run(
        "cycle",
        &[
            (
                "a.ts",
                "import { B } from './b'; export interface A { b: B | null }",
            ),
            (
                "b.ts",
                "import { A } from './a'; export interface B { a: A | null }",
            ),
        ],
    );
    assert!(
        project.errors("a.ts").is_empty(),
        "{:?}",
        project.diagnostics("a.ts")
    );
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.diagnostics("b.ts")
    );
}

#[test]
fn a_file_importing_a_cycle_still_sees_what_the_cycle_exports() {
    let project = run(
        "after-cycle",
        &[
            (
                "a.ts",
                "import { B } from './b'; export interface A { n: number }",
            ),
            (
                "b.ts",
                "import { A } from './a'; export interface B { s: string }",
            ),
            ("c.ts", "import { A } from './a'; const a: A = { n: 'x' };"),
        ],
    );
    assert_eq!(
        project.errors("c.ts").len(),
        1,
        "{:?}",
        project.diagnostics("c.ts")
    );
}

#[test]
fn the_report_does_not_depend_on_thread_timing() {
    // Many files in one layer, all importing one base, checked repeatedly.
    let mut files: Vec<(String, String)> = vec![("base.ts".into(), USER.into())];
    for i in 0..40 {
        let value = if i % 3 == 0 { "1" } else { "'a'" };
        files.push((
            format!("f{i}.ts"),
            format!("import {{ User }} from './base'; const u: User = {{ name: {value} }};"),
        ));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();

    let first = run("timing-0", &borrowed);
    let summary = |c: &Checked| -> Vec<usize> {
        c.report
            .files
            .iter()
            .map(|r| match &r.outcome {
                FileOutcome::Checked(d) => d.len(),
                _ => usize::MAX,
            })
            .collect()
    };
    let expected = summary(&first);
    assert_eq!(
        expected.iter().filter(|n| **n == 1).count(),
        14,
        "{expected:?}"
    );
    for round in 1..6 {
        let again = run(&format!("timing-{round}"), &borrowed);
        assert_eq!(summary(&again), expected);
    }
}

#[test]
fn a_file_with_a_syntax_error_does_not_stop_its_importers() {
    let project = run(
        "syntax-error",
        &[
            ("bad.ts", "export interface Broken { x: number"),
            (
                "ok.ts",
                "import { Broken } from './bad'; const b: Broken = { x: 1 };",
            ),
        ],
    );
    assert!(project.report.failure_count() >= 1);
    // The importer is still checked, and nothing is invented about the broken file.
    assert!(matches!(
        project
            .report
            .files
            .iter()
            .find(|r| {
                let path = project.graph.files().path(r.file_id).unwrap();
                path.file_name().unwrap() == "ok.ts"
            })
            .unwrap()
            .outcome,
        FileOutcome::Checked(_)
    ));
}
