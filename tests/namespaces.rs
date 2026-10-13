use ts_rust::{FileId, Severity, UnitContext, Worker};

// A `namespace N { ... }` is a scope for the names written in it, a place its exported
// types live (`N.I`), and, when every exported value in it has a type, an object value.

fn errors(source: &str) -> Vec<(Option<u32>, String)> {
    let ctx = UnitContext::new(FileId::ROOT, "a.ts", source);
    Worker::new()
        .check_unit(&ctx)
        .unwrap()
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.code.tsc_code(), d.message.to_string()))
        .collect()
}

#[test]
fn an_exported_function_of_a_namespace_is_a_property_of_its_value() {
    let source = "namespace N { export function f(n: number): number { return n } }
        const a: number = N.f(1);
        const b: string = N.f(1);";
    let found = errors(source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Some(2322));
}

#[test]
fn an_exported_type_of_a_namespace_is_named_with_a_dotted_path() {
    let source = "namespace N { export interface P { x: number } }
        const ok: N.P = { x: 1 };
        const bad: N.P = { x: 'a' };";
    let found = errors(source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Some(2322));
}

#[test]
fn a_sibling_type_written_without_the_prefix_is_found_inside_the_namespace() {
    let source = "namespace N {
            export interface P { x: number }
            export function make(): P { return { x: 1 } }
            export function bad(): P { return { x: 'a' } }
        }
        const p: N.P = N.make();";
    let found = errors(source);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn nested_namespaces_resolve_outward() {
    let source = "namespace A {
            export interface T { a: number }
            export namespace B {
                export function f(t: T): number { return t.a }
            }
        }
        const n: string = A.B.f({ a: 1 });";
    let found = errors(source);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_name_that_is_not_exported_is_not_a_property() {
    let source =
        "namespace N { function hidden(): number { return 1 } export const x: number = 1; }
        N.hidden;";
    let found = errors(source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Some(2339));
}

#[test]
fn an_ambient_namespace_exports_every_member() {
    let source = "declare namespace Lib { function parse(s: string): number; const VERSION: string; interface Opts { strict: boolean } }
        const n: number = Lib.parse('1');
        const v: number = Lib.VERSION;
        const o: Lib.Opts = { strict: 1 };";
    let found = errors(source);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn a_namespace_with_an_unannotated_exported_variable_stays_unknown_not_wrong() {
    // `x` is typed from its initializer only after the body is checked, so the object is
    // not built: using the namespace must not claim a property is missing.
    let source = "namespace N { export const x = 1; export function f(): number { return x } }
        N.x;
        N.f();";
    assert!(errors(source).is_empty(), "{:?}", errors(source));
}

#[test]
fn a_namespace_merged_with_a_function_stays_unknown() {
    // The namespace is not declared as a value, so nothing claims `extra` is an object
    // property. (The function keeps its own type, as it did before namespaces were read.)
    let source = "function f(): number { return 1 }
        namespace f { export const extra: number = 2; }
        const n: number = f();";
    assert!(errors(source).is_empty(), "{:?}", errors(source));
}

#[test]
fn two_blocks_of_the_same_namespace_stay_unknown() {
    let source = "namespace N { export const a: number = 1; }
        namespace N { export const b: number = 2; }
        N.a;
        N.b;";
    assert!(errors(source).is_empty(), "{:?}", errors(source));
}

#[test]
fn an_enum_and_a_class_inside_a_namespace_are_values_of_it() {
    let source = "namespace N {
            export enum Color { Red }
            export class K { n: number = 1 }
        }
        const c: string = N.Color.Red;
        const k: string = N.K;";
    let found = errors(source);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn an_error_inside_a_namespace_body_is_reported() {
    let found = errors("namespace N { export const x: number = 'a'; }");
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_dotted_namespace_name_is_still_unchecked_and_silent() {
    let source = "namespace A.B { export const x: number = 1; }
        A.B.x;";
    assert!(errors(source).is_empty(), "{:?}", errors(source));
}
