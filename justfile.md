# TypeScript Type-System Capability Checklist

Use this checklist to track TypeScript checker coverage by semantic level.

## 🟢 Basic

- [x] Primitive types
- [x] `any`
- [x] `unknown`
- [x] `never`
- [x] `void`
- [x] `null` / `undefined`
- [x] Literal types
- [x] Variables and type annotations
- [x] Object types
- [x] Arrays
- [ ] Tuples — no `Type::Tuple` variant; only `Array(TypeId)` exists
- [x] Functions
- [x] Function return checking
- [x] Basic function call checking
- [x] Constructors / `new`
- [x] Property access
- [ ] Element / index access — only `.length` on arrays/strings is special-cased; no computed/index access into objects, no index signatures
- [x] Basic classes
- [x] Basic inheritance
- [x] Basic assignability
- [x] Basic structural typing
- [x] Basic unions
- [ ] Basic intersections — no `Type::Intersection` variant anywhere
- [x] Type aliases
- [x] Basic `this`
- [x] Basic diagnostics

## 🟡 Mid

- [x] Generic type parameters
- [x] Generic constraints
- [x] Generic inference
- [x] Generic substitution
- [x] Generic functions
- [ ] Generic classes — `namespace.rs` explicitly does not push a class's own `<T>` into scope (`declared_type_param_list` returns `None` for `DeclKind::Class`)
- [x] Recursive types
- [x] Structural subtyping
- [x] Class inheritance compatibility
- [x] Function compatibility
- [ ] Contextual typing — no expected-type-flows-into-literal/arrow-inference; arrow/function-expression bodies are checked against their own declared or inferred return, not an ambient expected type
- [~] Control-flow analysis — CFG is built and drives unreachable-code detection, but flow-sensitive typing is limited to the narrow overlay below, not a general dataflow solve
- [x] Type narrowing
- [ ] Assignment-based narrowing — narrowing only comes from conditions (`narrow_condition`); an assignment does not itself refine a variable's tracked type
- [x] Branch narrowing
- [~] Type guards — `typeof`/nullish/truthy checks only; no user-defined type-predicate functions (`x is T`)
- [ ] Definite-assignment checking — not implemented
- [x] Symbol / namespace resolution
- [x] Nested structural compatibility
- [~] Complex expression checking — binary/logical/member/call/object/conditional expressions are covered; several expression kinds fall through to an explicit "unimplemented" warning rather than being checked

## 🟠 Mid → Advanced

- [~] Advanced generic inference — union-of-remaining-members inference for `T | undefined` params and subtype-widened multi-candidate binding exist; no priority/variance-bucketed candidate resolution (see prior message)
- [ ] Generic constraints + unions/intersections — constraints against a union bound work (constraint is just another `TypeId`), but intersections don't exist at all to combine with
- [ ] Generic instantiation caching — deliberately not built yet (see `docs/generics-tier1.md` §9); every call recomputes its own bindings
- [~] Recursive generic inference — a `seen` guard prevents infinite loops on self-referential generic shapes, but this is loop protection, not an inference algorithm for recursive generics
- [ ] Advanced overload resolution — no overload sets at all; a repeated method/property name is treated as unresolvable
- [~] Function variance — contravariant params / covariant return for plain functions, bivariant params for method-declared signatures; no variance annotations or check beyond this fixed rule
- [ ] Complex contextual typing — not implemented (see Contextual typing above)
- [ ] Advanced control-flow narrowing — not implemented
- [ ] Aliasing-aware narrowing — not implemented (`obj.prop` narrowing, narrowing through a second binding to the same value)
- [ ] Property-based narrowing — `narrow_condition` only recognizes a bare identifier or `typeof`/nullish checks on one, never `obj.prop === x`
- [ ] Loop fixed-point analysis — loop bodies are checked once; narrowing established inside is discarded at the loop's close, not iterated to a fixed point
- [ ] Discriminated-union narrowing — explicitly not implemented; `check_switch_statement`'s own comment states each case is checked with no discriminant-based narrowing
- [ ] `this`-based narrowing — not implemented
- [ ] Advanced definite-assignment semantics — not implemented
- [ ] Complex class/member relationships — no static members, no abstract classes/members, no accessors, no generic classes
- [ ] Advanced structural compatibility — no excess-property edge cases beyond the basic fresh-literal check, no `readonly`, no index-signature compatibility
- [ ] Diagnostic prioritization / cause chains — diagnostics are a flat `Vec<Diagnostic>`; no related-information chains or suppression of cascading errors beyond the `Any`/`Error` universal-compatibility escape hatch

## 🔴 Advanced Type System

- [ ] Conditional types — no `Type::Conditional` variant
- [ ] Distributive conditional types
- [ ] `infer`
- [ ] Mapped types — no `Type::Mapped` variant
- [ ] Key remapping
- [ ] Template-literal types
- [ ] Recursive conditional types
- [ ] Advanced indexed-access types — no indexed-access type (`T[K]`) support at all, basic or advanced
- [ ] Advanced `keyof` — no `keyof` support at all, basic or advanced
- [ ] Indexed assignment semantics
- [ ] Complex union reduction — union construction does flatten nested unions, drop `never`, and dedupe structurally-equal members (`alloc_union`), but nothing beyond that (no subsumption of a wider literal by its base type, etc.)
- [ ] Complex intersection reduction — no intersections exist to reduce
- [ ] Advanced variance
- [ ] Deep generic inference
- [ ] Complex overload selection
- [ ] Advanced contextual inference

One declaration merging note relevant here even though it's listed under Compiler/Project Level below: `namespace.rs` already merges repeated `interface Foo { ... }` declarations for the same name (`merged_interface_parts`) — that one piece of §Compiler/Project Level is done at the single-file level, just not the cross-file/module-graph version.

## 🔴 Compiler / Project Level

- [ ] Module resolution
- [ ] Module graph
- [ ] `node_modules` / package resolution
- [ ] `.d.ts` semantics
- [~] Declaration merging — implemented for `interface` re-declarations within one file (see note above); not implemented for merging across files, namespace-with-value merging, or any other merging kind
- [ ] Project-wide symbol graph
- [ ] Cross-file type propagation — `TypeChecker::check_source` takes one file in isolation; there is no multi-file program concept yet
- [ ] Incremental checking
- [ ] Project references
- [ ] Full `tsc`-compatible diagnostics
- [ ] Large-project caching

## Overall Status

- **Basic:** mostly covered — gaps are tuples, general index/element access, and intersections
- **Mid:** partial coverage, not "substantial" — generics-on-functions, inference, narrowing, and symbol resolution are solid; generic classes, contextual typing, assignment-based narrowing, definite assignment, and general type guards are all missing
- **Mid → Advanced:** early, not "WIP" in a load-bearing sense — a few individual pieces (multi-candidate inference, bivariant method params, recursion guards) exist, but the advanced narrowing and overload/variance items are uniformly unimplemented
- **Advanced Type System:** entirely remaining — no conditional types, mapped types, template-literal types, `keyof`, or indexed-access types exist in the `Type` enum at all
- **Compiler / Project Level:** almost entirely remaining — single-file-only today; the one exception is in-file interface declaration merging

> Goal: Track semantic capability, not implementation roadmap.
>
> This pass was filled in directly against the `ts-rust` source (`src/`, excluding `docs/`) as of this conversation, not against the docs, which are stale in places relative to the code (e.g. `docs/generics-tier1.md` undersells the current generics-on-interfaces support). Re-check after any change to `types.rs`, `namespace.rs`, `subtyping.rs`, `semantic/generics.rs`, or `bridge/narrow.rs`, since those five files are where nearly every checked/unchecked box above actually lives.
