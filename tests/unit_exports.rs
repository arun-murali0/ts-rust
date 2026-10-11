use ts_rust::{DeclKey, DeclKind, FileId, ModuleKey, UnitContext, UnitOutput, Worker};

// Step 6a: a unit run with a module key hands back the file's annotated exports as
// portable declarations, plus a name -> DeclKey table for the importer to look names up in.

fn module() -> ModuleKey {
    ModuleKey::project("a.ts")
}

fn run(source: &str) -> UnitOutput {
    let ctx = UnitContext::new(FileId::ROOT, "a.ts", source).with_module(module());
    Worker::new().check_unit(&ctx).unwrap()
}

fn key(name: &str, kind: DeclKind) -> DeclKey {
    DeclKey::new(module(), name, kind, 0)
}

fn names(out: &UnitOutput) -> Vec<&str> {
    let mut v: Vec<&str> = out.exports.iter().map(|d| &*d.name).collect();
    v.sort_unstable();
    v
}

#[test]
fn a_unit_without_a_module_key_exports_nothing() {
    let ctx = UnitContext::new(FileId::ROOT, "a.ts", "export interface P { x: number }");
    let out = Worker::new().check_unit(&ctx).unwrap();
    assert!(out.exports.is_empty());
    assert!(out.export_names.is_empty());
    assert!(!out.incomplete);
}

#[test]
fn an_exported_interface_becomes_one_portable_declaration() {
    let out = run("export interface Point { x: number; y: number }");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(out.exports.len(), 1);
    let decl = &out.exports[0];
    assert_eq!(&*decl.name, "Point");
    assert_eq!(decl.kind, DeclKind::Interface);
    assert_eq!(decl.key, key("Point", DeclKind::Interface));
    assert_eq!(
        out.export_names,
        vec![("Point".to_string(), decl.key)],
        "the table maps the name to the declaration's key"
    );
}

#[test]
fn a_declaration_that_is_not_exported_is_not_in_the_output() {
    let out = run("interface Hidden { x: number } export interface Shown { y: string }");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(names(&out), vec!["Shown"]);
}

#[test]
fn aliases_functions_and_annotated_constants_are_exported() {
    let out = run("export type Id = string;
         export type Pair = { a: number; b: string };
         export function add(a: number, b: number): number { return a + b; }
         export const limit: number = 3;");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(names(&out), vec!["Id", "Pair", "add", "limit"]);
    let kind = |n: &str| out.exports.iter().find(|d| &*d.name == n).unwrap().kind;
    assert_eq!(kind("Id"), DeclKind::Alias);
    assert_eq!(kind("Pair"), DeclKind::Alias);
    assert_eq!(kind("add"), DeclKind::Function);
    assert_eq!(kind("limit"), DeclKind::Var);
}

#[test]
fn a_generic_interface_keeps_its_type_parameters() {
    let out = run("export interface Box<T extends string> { value: T }");
    assert!(!out.incomplete, "{out:?}");
    let decl = &out.exports[0];
    assert_eq!(decl.type_params.len(), 1);
    assert_eq!(&*decl.type_params[0].name, "T");
    assert!(decl.type_params[0].bound.is_some(), "the bound is exported");
}

#[test]
fn a_generic_function_keeps_its_type_parameters() {
    let out = run("export function id<T>(x: T): T { return x; }");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(out.exports[0].type_params.len(), 1);
}

#[test]
fn a_self_referencing_interface_is_exported() {
    let out = run("export interface Node { value: number; next: Node | null }");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(out.exports.len(), 1);
}

#[test]
fn an_exported_declaration_may_name_another_exported_one() {
    let out = run("export interface User { name: string }
         export function greet(u: User): string { return u.name; }
         export const me: User = { name: 'a' };");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(names(&out), vec!["User", "greet", "me"]);
}

#[test]
fn an_export_that_names_an_unexported_declaration_is_unsupported() {
    // The importer would be handed a reference to a key nobody exports.
    let out = run("interface Hidden { x: number }
         export function f(h: Hidden): number { return h.x; }");
    assert!(
        out.incomplete,
        "an unsupported export marks the unit incomplete"
    );
    assert!(names(&out).is_empty(), "{out:?}");
}

#[test]
fn an_unannotated_export_is_unsupported_not_guessed() {
    let out = run("export interface P { x: number } export const n = 1;");
    assert!(out.incomplete);
    assert_eq!(
        names(&out),
        vec!["P"],
        "the supported export still comes through"
    );
}

#[test]
fn a_function_without_a_return_annotation_is_unsupported() {
    let out = run("export function f(a: number) { return a; }");
    assert!(out.incomplete);
    assert!(out.exports.is_empty());
}

#[test]
fn exports_do_not_change_the_diagnostics() {
    let source = "export interface P { x: number } const bad: number = 'x';";
    let with = run(source);
    let without = Worker::new()
        .check_unit(&UnitContext::new(FileId::ROOT, "a.ts", source))
        .unwrap();
    assert!(!with.diagnostics.is_empty());
    assert_eq!(with.diagnostics.len(), without.diagnostics.len());
}

#[test]
fn the_same_source_gives_the_same_keys_and_signatures_in_any_worker() {
    let source = "export interface Node { next: Node | null } export function f(n: Node): Node { return n; }";
    let one = run(source);
    let mut worker = Worker::new();
    // Warm the worker's arena with something unrelated so slot numbers differ.
    let _ = worker.check_unit(&UnitContext::new(
        FileId::ROOT,
        "z.ts",
        "interface Z { z: number } const z: Z = { z: 1 };",
    ));
    let two = worker
        .check_unit(&UnitContext::new(FileId::ROOT, "a.ts", source).with_module(module()))
        .unwrap();
    let ids = |o: &UnitOutput| {
        let mut v: Vec<_> = o.exports.iter().map(|d| (d.key, d.sig)).collect();
        v.sort();
        v
    };
    assert_eq!(ids(&one), ids(&two));
}

#[test]
fn a_second_function_overload_is_unsupported() {
    let out = run("export function f(a: number): number;
         export function f(a: string): string;
         export function f(a: any): any { return a; }");
    assert!(out.incomplete);
}

#[test]
fn a_function_body_refers_to_the_exported_interface_by_its_key() {
    use ts_rust::PNode;
    let out = run("export interface User { name: string }
         export function greet(u: User): string { return u.name; }");
    assert!(!out.incomplete, "{out:?}");
    let greet = out.exports.iter().find(|d| &*d.name == "greet").unwrap();
    let user = key("User", DeclKind::Interface);
    assert!(
        greet.body.nodes.iter().any(
            |n| matches!(n, PNode::DeclRef { key, name } if *key == user && &**name == "User")
        ),
        "{greet:?}"
    );
}

#[test]
fn a_type_parameter_is_keyed_to_the_declaration_that_declares_it() {
    use ts_rust::PNode;
    let out = run("export interface Box<T> { value: T }");
    let decl = &out.exports[0];
    assert!(
        decl.body
            .nodes
            .iter()
            .any(|n| matches!(n, PNode::TypeParam { key, index: 0, .. } if *key == decl.key)),
        "{decl:?}"
    );
}

#[test]
fn builtin_generics_in_a_signature_are_exportable() {
    let out = run("export function load(): Promise<number> { return null as any; }");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(names(&out), vec!["load"]);
}

#[test]
fn an_alias_to_an_exported_interface_exports() {
    let out = run("export interface U { n: number } export type V = U; export type W = V[];");
    assert!(!out.incomplete, "{out:?}");
    assert_eq!(names(&out), vec!["U", "V", "W"]);
}
