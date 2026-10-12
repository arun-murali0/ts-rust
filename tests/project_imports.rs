#![cfg(feature = "module-resolution")]

use std::fs;
use std::path::{Path, PathBuf};

use ts_rust::{Diagnostic, FileOutcome, ModuleGraph, ModuleResolver, Severity, check_project};

// check_project passes what each file exports to the files that import it.
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

// An exported declaration may name one that is not exported. The hidden one
// leaves the file under its own key, so the exporter is complete and its signature
// crosses files with the hidden type intact.
#[test]
fn an_exported_function_naming_an_unexported_interface_crosses_files() {
    let project = run(
        "hidden-interface",
        &[
            (
                "a.ts",
                "interface Hidden { id: number }\nexport function f(h: Hidden): number { return h.id }",
            ),
            (
                "b.ts",
                "import { f } from './a';\nf({ id: 1 });\nf({ id: 'x' });",
            ),
        ],
    );
    assert!(project.diagnostics("a.ts").is_empty());
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
    // A property of an object literal argument is reported on the property, as tsc does.
    assert_eq!(errors[0].code.tsc_code(), Some(2322));
}

#[test]
fn a_hidden_declaration_is_reached_through_a_chain_of_hidden_ones() {
    let project = run(
        "hidden-chain",
        &[
            (
                "a.ts",
                "interface Inner { n: number }\ninterface Outer { inner: Inner }\nexport const o: Outer = { inner: { n: 1 } };",
            ),
            (
                "b.ts",
                "import { o } from './a';\nconst s: string = o.inner.n;",
            ),
        ],
    );
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code.tsc_code(), Some(2322));
}

#[test]
fn a_hidden_declaration_cannot_be_imported_by_name() {
    let project = run(
        "hidden-not-importable",
        &[
            (
                "a.ts",
                "interface Hidden { id: number }\nexport function f(h: Hidden): number { return h.id }",
            ),
            ("b.ts", "import { Hidden } from './a';"),
        ],
    );
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code.tsc_code(), Some(2305));
}

// Across files: an inferred export is used as if it had been annotated.
#[test]
fn an_inferred_export_crosses_files() {
    let project = run(
        "inferred",
        &[
            (
                "a.ts",
                "export const count = 1;\nexport let label = 'x';\nexport function double(n: number) { return n * 2; }\nexport const origin = { x: 0, y: 0 };",
            ),
            (
                "b.ts",
                "import { count, label, double, origin } from './a';\nconst a: string = count;\nconst b: number = label;\nconst c: string = double(2);\nconst d: string = origin.x;",
            ),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(codes, vec![Some(2322); 4], "{:?}", project.errors("b.ts"));
}

#[test]
fn a_correct_use_of_an_inferred_export_is_clean() {
    let project = run(
        "inferred-clean",
        &[
            (
                "a.ts",
                "export let label = 'x';\nexport function double(n: number) { return n * 2; }",
            ),
            (
                "b.ts",
                "import { label, double } from './a';\nconst a: string = label;\nconst c: number = double(2);",
            ),
        ],
    );
    assert!(project.errors("b.ts").is_empty());
}

// `export * from` re-exports what the other module exports, under the
// keys it already has, except `default`.
#[test]
fn a_star_re_export_carries_the_other_modules_exports() {
    let project = run(
        "star",
        &[
            (
                "a.ts",
                "export interface User { name: string }\nexport function id(n: number): number { return n }",
            ),
            ("barrel.ts", "export * from './a';"),
            (
                "b.ts",
                "import { User, id } from './barrel';\nconst u: User = { name: 1 };\nconst n: string = id(1);",
            ),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(
        codes,
        vec![Some(2322), Some(2322)],
        "{:?}",
        project.errors("b.ts")
    );
    assert!(project.errors("barrel.ts").is_empty());
}

#[test]
fn a_star_re_export_chains_through_several_barrels() {
    let project = run(
        "star-chain",
        &[
            ("a.ts", USER),
            ("one.ts", "export * from './a';"),
            ("two.ts", "export * from './one';"),
            (
                "b.ts",
                "import { User } from './two';\nconst u: User = { name: 1 };",
            ),
        ],
    );
    assert_eq!(project.errors("b.ts").len(), 1);
}

#[test]
fn a_star_re_export_does_not_carry_the_default() {
    let project = run(
        "star-default",
        &[
            (
                "a.ts",
                "export default function f(): number { return 1 }\nexport interface User { name: string }",
            ),
            ("barrel.ts", "export * from './a';"),
            ("b.ts", "import f from './barrel';"),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(codes, vec![Some(1192)]);
}

#[test]
fn a_local_export_wins_over_a_star_re_export_of_the_same_name() {
    let project = run(
        "star-local-wins",
        &[
            ("a.ts", "export interface Shape { a: number }"),
            (
                "barrel.ts",
                "export * from './a';\nexport interface Shape { b: string }",
            ),
            (
                "b.ts",
                "import { Shape } from './barrel';\nconst s: Shape = { b: 'x' };",
            ),
        ],
    );
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_star_re_export_of_a_partial_module_makes_the_barrel_partial() {
    // a.ts has a class the portable form cannot carry yet, so the barrel cannot claim to
    // list everything: a missing name is not reported against it.
    let project = run(
        "star-partial",
        &[
            (
                "a.ts",
                "export interface User { name: string }\nexport class K {}",
            ),
            ("barrel.ts", "export * from './a';"),
            ("b.ts", "import { K } from './barrel';"),
        ],
    );
    assert!(project.errors("b.ts").is_empty());
}

#[test]
fn a_star_re_export_of_a_missing_file_is_ts2307() {
    let project = run("star-missing", &[("barrel.ts", "export * from './gone';")]);
    let codes: Vec<_> = project
        .errors("barrel.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(codes, vec![Some(2307)]);
}

// The members of a cycle see what each other exports.
#[test]
fn members_of_a_cycle_use_each_others_exports() {
    let project = run(
        "cycle-sees-members",
        &[
            (
                "a.ts",
                "import { B } from './b';\nexport interface A { b: B }\nconst x: A = { b: { n: 'x' } };",
            ),
            (
                "b.ts",
                "import { A } from './a';\nexport interface B { n: number }\nexport function peek(a: A): number { return a.b.n }",
            ),
        ],
    );
    let a = project.errors("a.ts");
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].code.tsc_code(), Some(2322));
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_misspelled_name_inside_a_cycle_is_reported_once_it_settles() {
    let project = run(
        "cycle-typo",
        &[
            (
                "a.ts",
                "import { Bee } from './b';\nexport interface A { n: number }",
            ),
            (
                "b.ts",
                "import { A } from './a';\nexport interface B { a: A }",
            ),
        ],
    );
    let codes: Vec<_> = project
        .errors("a.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(codes, vec![Some(2305)]);
    assert!(project.errors("b.ts").is_empty());
}

#[test]
fn a_file_that_imports_a_cycle_sees_the_settled_exports() {
    let project = run(
        "cycle-importer",
        &[
            (
                "a.ts",
                "import { B } from './b';\nexport interface A { b: B }",
            ),
            (
                "b.ts",
                "import { A } from './a';\nexport interface B { n: number }",
            ),
            (
                "c.ts",
                "import { A } from './a';\nconst a: A = { b: { n: 'x' } };",
            ),
        ],
    );
    assert_eq!(project.errors("c.ts").len(), 1);
}

// The report says which declarations of other files a file's check read, which
// is what an incremental run needs to know a file does not depend on a change.
fn reads_of(project: &Checked, file: &str) -> Vec<ts_rust::DeclKey> {
    for report in &project.report.files {
        let path = project.graph.files().path(report.file_id).unwrap();
        if path.file_name().unwrap().to_string_lossy() == file {
            return report.reads.clone();
        }
    }
    panic!("{file} is not in the report");
}

fn key_in(project: &Checked, file: &str, name: &str, kind: ts_rust::DeclKind) -> ts_rust::DeclKey {
    for report in &project.report.files {
        let path = project.graph.files().path(report.file_id).unwrap();
        if path.file_name().unwrap().to_string_lossy() == file {
            let module = project.graph.module_key(report.file_id).unwrap();
            return ts_rust::DeclKey::new(module, name, kind, 0);
        }
    }
    panic!("{file} is not in the report");
}

#[test]
fn a_file_reads_only_the_declarations_it_imports() {
    let project = run(
        "reads",
        &[
            (
                "a.ts",
                "export interface User { name: string }\nexport interface Unused { n: number }",
            ),
            (
                "b.ts",
                "import { User } from './a';\nconst u: User = { name: 'x' };",
            ),
        ],
    );
    let reads = reads_of(&project, "b.ts");
    assert!(reads.contains(&key_in(
        &project,
        "a.ts",
        "User",
        ts_rust::DeclKind::Interface
    )));
    assert!(!reads.contains(&key_in(
        &project,
        "a.ts",
        "Unused",
        ts_rust::DeclKind::Interface
    )));
    assert!(reads_of(&project, "a.ts").is_empty());
}

#[test]
fn a_file_reads_the_declarations_its_imports_reach() {
    let project = run(
        "reads-transitive",
        &[
            (
                "a.ts",
                "interface Hidden { id: number }\nexport function f(h: Hidden): number { return h.id }",
            ),
            ("b.ts", "import { f } from './a';\nf({ id: 1 });"),
        ],
    );
    let reads = reads_of(&project, "b.ts");
    assert!(reads.contains(&key_in(&project, "a.ts", "f", ts_rust::DeclKind::Function)));
    assert!(reads.contains(&key_in(
        &project,
        "a.ts",
        "Hidden",
        ts_rust::DeclKind::Interface
    )));
}

#[test]
fn the_reads_are_sorted_and_the_same_on_every_run() {
    let files = [
        (
            "a.ts",
            "export interface A { a: number }\nexport interface B { b: number }",
        ),
        ("b.ts", "import { B, A } from './a';\nlet x: A | B;"),
    ];
    let first = reads_of(&run("reads-order-1", &files), "b.ts");
    let second = reads_of(&run("reads-order-2", &files), "b.ts");
    assert_eq!(first.len(), 2);
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(first, sorted);
    assert_eq!(first, second);
}

// Step 7d, enums: one declaration carries both names, the type (the union of the member
// literals, printed as the enum) and the value (an object of the members).
#[test]
fn an_enum_crosses_files_as_a_type_and_a_value() {
    let project = run(
        "enum-basic",
        &[
            ("a.ts", "export enum Color { Red, Green }"),
            (
                "b.ts",
                "import { Color } from './a';\nconst ok: Color = Color.Red;\nconst bad: string = Color.Green;\nconst wrong: Color = 'x';",
            ),
        ],
    );
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0].code.tsc_code(), Some(2322));
    assert!(
        errors[0].message.contains("'Color'"),
        "{}",
        errors[0].message
    );
    assert_eq!(errors[1].code.tsc_code(), Some(2322));
    assert!(project.errors("a.ts").is_empty());
}

#[test]
fn a_string_enum_member_keeps_its_value_across_files() {
    let project = run(
        "enum-string",
        &[
            ("a.ts", "export enum Dir { Up = 'UP', Down = 'DOWN' }"),
            (
                "b.ts",
                "import { Dir } from './a';\nconst d: Dir = Dir.Up;\nconst n: number = Dir.Down;",
            ),
        ],
    );
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
}

#[test]
fn an_enum_does_not_make_the_exporter_partial() {
    // The module's list is complete, so a name it does not export is reported.
    let project = run(
        "enum-complete",
        &[
            (
                "a.ts",
                "export enum Color { Red }\nexport interface P { c: Color }",
            ),
            ("b.ts", "import { Nope } from './a';"),
        ],
    );
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code.tsc_code(), Some(2305));
}

#[test]
fn an_enum_named_by_an_exported_interface_still_checks() {
    let project = run(
        "enum-in-interface",
        &[
            (
                "a.ts",
                "export enum Color { Red, Green }\nexport interface P { c: Color }",
            ),
            (
                "b.ts",
                "import { Color, P } from './a';\nconst p: P = { c: Color.Red };\nconst q: P = { c: 'x' };",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn an_enum_is_re_exported_through_a_barrel() {
    let project = run(
        "enum-barrel",
        &[
            ("a.ts", "export enum Color { Red, Green }"),
            ("one.ts", "export { Color } from './a';"),
            ("two.ts", "export * from './one';"),
            (
                "b.ts",
                "import { Color } from './two';\nconst bad: string = Color.Red;",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn an_imported_enum_passed_on_by_the_importing_file_still_works() {
    let project = run(
        "enum-pass-on",
        &[
            ("a.ts", "export enum Color { Red }"),
            ("one.ts", "import { Color } from './a';\nexport { Color };"),
            (
                "b.ts",
                "import { Color } from './one';\nconst bad: string = Color.Red;",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn an_aliased_import_of_an_enum_binds_both_names() {
    let project = run(
        "enum-alias",
        &[
            ("a.ts", "export enum Color { Red }"),
            (
                "b.ts",
                "import { Color as C } from './a';\nconst c: C = C.Red;\nconst bad: string = C.Red;",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn an_enum_with_a_computed_member_stays_out_and_makes_the_unit_partial() {
    let project = run(
        "enum-computed",
        &[
            (
                "a.ts",
                "declare function f(): number;\nexport enum E { A = f() }",
            ),
            ("b.ts", "import { Nope } from './a';"),
        ],
    );
    assert!(project.errors("b.ts").is_empty());
}

// Step 7d, classes: the instance type and the constructor are two declarations that share
// the class's name, and an import of the name binds both.
#[test]
fn a_class_crosses_files_as_a_type_and_a_constructor() {
    let project = run(
        "class-basic",
        &[
            (
                "a.ts",
                "export class Point {\n  x: number;\n  constructor(x: number) { this.x = x; }\n  dist(): number { return this.x; }\n}",
            ),
            (
                "b.ts",
                "import { Point } from './a';\nconst p: Point = new Point(1);\nconst bad: string = p.x;\nconst worse = new Point('a');",
            ),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(
        codes,
        vec![Some(2322), Some(2345)],
        "{:?}",
        project.errors("b.ts")
    );
    assert!(
        project.errors("a.ts").is_empty(),
        "{:?}",
        project.errors("a.ts")
    );
}

#[test]
fn a_default_exported_class_crosses_files() {
    let project = run(
        "class-default",
        &[
            ("a.ts", "export default class K { n: number = 1; }"),
            (
                "b.ts",
                "import K from './a';\nconst k: K = new K();\nconst s: string = k.n;",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_generic_class_crosses_files() {
    let project = run(
        "class-generic",
        &[
            (
                "a.ts",
                "export class Box<T> {\n  value: T;\n  constructor(v: T) { this.value = v; }\n}",
            ),
            (
                "b.ts",
                "import { Box } from './a';\nconst b: Box<number> = new Box(1);\nconst s: string = b.value;",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_class_does_not_make_the_exporter_partial() {
    let project = run(
        "class-complete",
        &[
            ("a.ts", "export class K { n: number = 1; }"),
            ("b.ts", "import { Nope } from './a';"),
        ],
    );
    let errors = project.errors("b.ts");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code.tsc_code(), Some(2305));
}

#[test]
fn a_class_is_re_exported_through_a_barrel() {
    let project = run(
        "class-barrel",
        &[
            ("a.ts", "export class K { n: number = 1; }"),
            ("barrel.ts", "export * from './a';"),
            (
                "b.ts",
                "import { K } from './barrel';\nconst s: string = new K().n;",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn an_exported_function_can_name_an_unexported_class() {
    let project = run(
        "class-hidden",
        &[
            (
                "a.ts",
                "class Hidden { id: number = 1; }\nexport function f(h: Hidden): number { return h.id; }",
            ),
            ("b.ts", "import { f } from './a';\nf(5);"),
        ],
    );
    assert!(
        project.errors("a.ts").is_empty(),
        "{:?}",
        project.errors("a.ts")
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_default_exported_identifier_crosses_files() {
    let project = run(
        "default-identifier",
        &[
            ("a.ts", "const v = 1;\nexport default v;"),
            ("b.ts", "import v from './a';\nconst s: string = v;"),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_class_that_extends_another_does_not_invent_errors() {
    let project = run(
        "class-extends",
        &[
            (
                "a.ts",
                "class A { a: number = 1; }\nexport class B extends A { b: string = 'x'; }",
            ),
            (
                "b.ts",
                "import { B } from './a';\nconst x: B = new B();\nconst n: number = x.b.length;",
            ),
        ],
    );
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.errors("b.ts")
    );
}

// Step 7d, `import * as ns`: the module's value exports as one object.
#[test]
fn a_namespace_import_is_an_object_of_the_modules_value_exports() {
    let project = run(
        "star-as",
        &[
            (
                "a.ts",
                "export interface User { name: string }\nexport function f(n: number): number { return n }\nexport const k = 1;\nexport enum Color { Red }",
            ),
            (
                "b.ts",
                "import * as ns from './a';\nconst r: string = ns.f(1);\nconst c: string = ns.Color.Red;\nconst ok: number = ns.f(2);\nns.missing;",
            ),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(
        codes,
        vec![Some(2322), Some(2322), Some(2339)],
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_namespace_import_of_a_partial_module_stays_unbound() {
    // `namespace N` is not readable yet, so a.ts's list may be missing exports and `ns`
    // must not claim a property does not exist.
    let project = run(
        "star-as-partial",
        &[
            (
                "a.ts",
                "export const k = 1;\nexport namespace N { export const x = 1; }",
            ),
            ("b.ts", "import * as ns from './a';\nns.anything;"),
        ],
    );
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_type_only_namespace_import_binds_no_value() {
    let project = run(
        "star-as-type",
        &[
            ("a.ts", "export const k = 1;"),
            ("b.ts", "import type * as ns from './a';"),
        ],
    );
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.errors("b.ts")
    );
}

// `import x = require(...)` binds the module's value exports like `import * as x`.
#[test]
fn an_import_equals_require_binds_the_modules_value_exports() {
    let project = run(
        "require",
        &[
            (
                "a.ts",
                "export const k = 1;\nexport function f(n: number): number { return n }",
            ),
            (
                "b.ts",
                "import a = require('./a');\nconst s: string = a.f(1);\nconst ok: number = a.f(2);",
            ),
        ],
    );
    let errors: Vec<_> = project
        .errors("b.ts")
        .into_iter()
        .filter(|d| d.code.tsc_code() == Some(2322))
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", project.errors("b.ts"));
}

#[test]
fn an_import_equals_require_of_a_missing_file_is_ts2307() {
    let project = run(
        "require-missing",
        &[("b.ts", "import a = require('./gone');")],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(codes, vec![Some(2307)]);
}

#[test]
fn a_property_missing_from_an_export_equals_value_is_reported() {
    let project = run(
        "require-export-equals",
        &[
            ("a.ts", "const k = 1;\nexport = k;"),
            ("b.ts", "import a = require('./a');\na.anything;"),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(codes, vec![Some(2339)], "{:?}", project.errors("b.ts"));
}

// A dynamic import with a string argument is a dependency edge like any other.
#[test]
fn a_dynamic_import_is_a_dependency_of_the_importing_file() {
    let project = run(
        "dynamic",
        &[
            ("a.ts", "export const k = 1;"),
            (
                "b.ts",
                "export async function g() { return import('./a'); }",
            ),
        ],
    );
    let id_of = |name: &str| {
        project
            .report
            .files
            .iter()
            .map(|r| r.file_id)
            .find(|id| {
                project
                    .graph
                    .files()
                    .path(*id)
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    == name
            })
            .unwrap()
    };
    assert_eq!(project.graph.dependencies(id_of("b.ts")), &[id_of("a.ts")]);
}

// `export = x`: the module is that one declaration.
#[test]
fn an_import_equals_require_of_an_export_equals_module_is_that_value() {
    let project = run(
        "export-equals-fn",
        &[
            (
                "a.ts",
                "function pad(s: string, n: number): string { return s }\nexport = pad;",
            ),
            (
                "b.ts",
                "import pad = require('./a');\nconst a: string = pad('x', 1);\nconst b: number = pad('x', 1);\npad(1, 1);",
            ),
        ],
    );
    let codes: Vec<_> = project
        .errors("b.ts")
        .iter()
        .map(|d| d.code.tsc_code())
        .collect();
    assert_eq!(
        codes,
        vec![Some(2322), Some(2345)],
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn a_default_import_of_an_export_equals_module_is_that_value() {
    let project = run(
        "export-equals-default",
        &[
            (
                "a.ts",
                "function pad(s: string): string { return s }\nexport = pad;",
            ),
            (
                "b.ts",
                "import pad from './a';\nconst n: number = pad('x');",
            ),
        ],
    );
    assert_eq!(
        project.errors("b.ts").len(),
        1,
        "{:?}",
        project.errors("b.ts")
    );
}

#[test]
fn an_export_equals_module_still_never_reports_a_missing_name() {
    let project = run(
        "export-equals-named",
        &[
            (
                "a.ts",
                "function pad(s: string): string { return s }\nexport = pad;",
            ),
            ("b.ts", "import { length } from './a';"),
        ],
    );
    assert!(
        project.errors("b.ts").is_empty(),
        "{:?}",
        project.errors("b.ts")
    );
}
