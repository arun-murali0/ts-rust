# ts-rust

A from-scratch TypeScript type checker, written in Rust, built on
[Oxc](https://oxc.rs)'s parser, AST, and semantic analyzer. Ships as both a
native Rust library and a WASM/npm package.

> **Status: V1 through V3c implemented.** Primitives and functions (V1),
> structural types — objects, arrays, unions, aliases, interfaces (V2),
> literal types (V3a), scoped `if`/`else` narrowing (V3b), and classes
> including `this` (V3c) are in place, covered by 42 integration fixtures,
> 29 unit tests, and 1 doctest. Every Oxc field and method this crate
> touches was verified against the real `oxc` source at tag
> `crates_v0.144.0`. `oxc` 0.144.0 requires a Rust toolchain supporting the
> 2024 edition (1.85+). See [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
> for the full local verification checklist and current benchmark numbers.
> V4 is planned, not started — see the roadmap below.

## Why

TypeScript's own checker (`tsc`) is JS-based and slow on large codebases.
Microsoft's `tsgo` addresses this by porting `tsc` to Go. `ts-rust` takes a
different path: a **new implementation** in Rust, on top of Oxc's already
very fast parser/AST, rather than a port of `tsc`'s existing codebase.

[Ezno](https://github.com/kaleidawave/ezno) is a related project (a Rust
TypeScript checker with its own parser) and is used here as **architectural
reference only**. No Ezno source is vendored or depended on. Where a design
choice below follows a lesson learned from Ezno's public writeups, or
deliberately departs from it, that's called out explicitly in `DESIGN.md`.

## North star, honestly stated

Full practical TypeScript checking, in the spirit of `tsgo`'s coverage, but
this is a multi-year goal, reached through versioned, measurable phases,
not a near-term promise. Progress is tracked via fixture pass rate now, and
against TypeScript's own conformance suite from V5 onward. See
`docs/ROADMAP.md`, section "Measuring progress."

## Design principles (see `docs/DESIGN.md` for full detail)

- **Arena-allocated types** (`TypeId` handles, not `Box<Type>` trees).
  Cheap to compare, and a natural fit for recursive type shapes.
- **Two-pass checking.** Declare (hoist signatures), then check (walk
  bodies). This is what lets forward references work, including between
  interfaces, aliases, and classes declared in any order.
- **One subtyping relation** (`is_subtype`) that everything routes through.
  No scattered ad hoc comparisons across the codebase.
- **Real semantic analysis, not a hand-rolled scope chain.** Bindings are
  tracked by the AST's own `SymbolId`, assigned by `oxc_semantic`'s binder,
  so hoisting, the temporal dead zone, and per-iteration `let` in loops are
  handled correctly by Oxc itself rather than reimplemented here.
- **`CheckContext` bundles what every check function needs** (the type
  arena, the type namespace, the symbol map, diagnostics, narrowing state,
  the enclosing function's return type, the enclosing class's instance
  type for `this`) so adding one more thing to check doesn't mean touching
  every function signature in `bridge/` again.
- **Scoped narrowing, not a general effects system.** Narrowing is a
  branch-local override map that reverts once an `if` statement ends. This
  is a deliberate, smaller scope than TypeScript's real narrowing (which
  persists across an early return); the gap is demonstrated, not hidden,
  in `tests/fixtures/v3b/known_gap_narrowing_does_not_persist_past_if.ts`.
  Closing this is V4's Tier 2 item 5 — see the roadmap below.
- **Dense keys get dense storage, sparse keys get a `HashMap`, and the
  hash function is chosen for the actual threat model.** `SymbolTypeMap`
  (nearly every symbol in a file ends up registered) is a `Vec<Option<TypeId>>`
  indexed directly by `oxc_semantic`'s own sequential `SymbolId`, not a
  `HashMap` — skips hashing entirely for a key space that's already dense.
  `TypeNamespace`'s `String`-keyed map uses an in-tree FxHash
  implementation (the same non-cryptographic hash `rustc` itself uses)
  instead of the stdlib's DoS-resistant default, since nothing hashed here
  is adversarial input. `NarrowState` stays a small linear-scanned `Vec`
  rather than either of those, since a narrowing overlay is created fresh
  per branch and typically holds only a handful of entries.
- **Explicit `Unsupported` diagnostics** for anything not yet handled.
  Never a silent skip, never a wrong answer presented as correct.
- **Platform-agnostic core, thin platform shells.** Native and WASM share
  all checking logic; only the outer API surface differs.
- **Single-threaded core through V4.** Parallelism, when introduced, will
  be scoped to independent per-file checking in multi-file projects,
  native-only, behind a feature flag, and never required by core logic.

## Roadmap at a glance

| Version | Focus | Status |
|---|---|---|
| V1 | Core skeleton: primitives, functions, two-pass checking | Done |
| V2 | Structural types: objects, unions, arrays, aliases, interfaces | Done |
| V3a | Literal types, `const`/`let` widening | Done |
| V3b | Scoped `if`/`else` narrowing (`typeof`, equality, truthiness) | Done, narrower scope than real TS — see below |
| V3c | Classes: fields, methods, `extends`, `new`, `this` | Done |
| V4 | Arrow functions, `&&`/`||`/`??`/ternary, narrowing past early return, type assertions, computed/optional member access, enums, `for`/`while`/`switch` | Planned — see below |
| V4+ | Generics, module resolution, `.d.ts`, incremental checking | Not started |
| V5+ | Variance, overloads, conditional/mapped types, TS conformance suite | Not started |

Full detail, exit criteria, and explicit non-goals per version live in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## What's actually checked right now

- Primitive types (`number`, `string`, `boolean`, `null`, `undefined`,
  `any`, `unknown`) on declarations, parameters, and return types.
- Literal types (`"up"`, `5`, `true`), with TypeScript's widening rule:
  `const` keeps the literal, `let`/`var` widen to the base primitive.
- Object types, with structural (width) subtyping and optional properties.
- Array types (`T[]`), covariant in the element type.
- Union types, checked member by member in both directions, flattened and
  deduplicated by structural value (not by allocation identity) at
  construction.
- `type` aliases and `interface` declarations, including forward
  references between them in either declaration order.
- Classes: fields, methods, `extends` (flattened structurally — a
  subclass instance is just an `Object` type with the parent's properties
  merged in), `new` expressions arity- and argument-checked the same way
  a plain function call is, and **`this`**, which resolves to the
  enclosing class's instance type inside a constructor or method body.
- Method calls through member access (`obj.method(...)`), not just bare
  function calls.
- `if`/`else` narrowing: `typeof x === "..."`, `x === null`/`undefined`,
  and truthiness (`if (x)`), including literal-precision cases like
  `0`/`""` being recognized as definitely falsy.
- Arithmetic and comparison binary operators, function call arity and
  argument types.

**Known, deliberate gaps — not silent, each backed by a fixture proving
the honest degradation, not a crash or wrong answer. V4 (below) closes
several of these:**

- **Narrowing doesn't persist past the `if` statement.** TypeScript
  recognizes that `if (x === null) return; ...use x as non-null...` narrows
  `x` for the rest of the function. ts-rust's narrowing is scoped strictly
  to inside the branch that earned it. See
  `tests/fixtures/v3b/known_gap_narrowing_does_not_persist_past_if.ts`.
  **Planned for V4** (Tier 2).
- **No discriminated unions.** Narrowing only recognizes a bare identifier
  on one side of `===`, not a member expression like `shape.kind === "circle"`.
- **No arrow functions or function expressions as values.** Any file using
  callbacks (`arr.map(x => ...)`) loses type tracking on them immediately —
  the single highest-leverage gap right now. **Planned for V4** (Tier 1,
  first item).
- **No logical (`&&`/`||`/`??`) or ternary expressions, type assertions
  (`as`/`!`), computed member access (`obj[key]`), or optional chaining
  (`?.`).** **Planned for V4.**
- **No `for`/`while`/`switch`/`try`/`throw`/`import`/`export` handling.**
  **`for`/`while`/`switch` planned for V4** (Tier 2, last); modules and
  `.d.ts` are a V4+ item.
- **No generics.** Deferred past V4 on purpose — type parameter binding,
  call-site inference, and variance are a different order of magnitude of
  work from the rest of V4's scope; see `docs/DESIGN.md` section 4.4.
- **The excess-property check TypeScript applies to fresh object
  literals** (rejecting extra keys at the call site) isn't implemented;
  see `tests/fixtures/v2/excess_property_literal.ts`.
- Classes' getters/setters, static members, and index signatures aren't
  checked. Encountering any unhandled construct produces a
  `Warning`-severity diagnostic naming it, not a silent pass.

## Repository layout

```
docs/
  DESIGN.md           full system design (architecture, data model, version specs)
  ROADMAP.md          versioned checklist and contributor entry points
  VERIFICATION.md     local verification checklist (build, test, bench, lint)
  WASM_BUILD.md       wasm-pack build and npm publish instructions
src/
  lib.rs              public API (TypeChecker, CheckResult)
  arena.rs            flat TypeId-based type storage
  types.rs            the type vocabulary (primitives, literals, objects, arrays, unions, never)
  subtyping.rs        the single is_subtype relation
  symbol_map.rs       SymbolId to TypeId map, Vec-indexed, backed by oxc_semantic
  namespace.rs        type/interface/class namespace, lazy and memoized, FxHash-keyed
  type_annotation.rs  resolves a TSType annotation to a TypeId
  fxhash.rs           in-tree FxHash (rustc's own hasher) for namespace.rs's name lookups
  diagnostics.rs      public Diagnostic/Severity types
  error.rs            CheckerError
  wasm.rs             wasm-bindgen boundary (feature-gated)
  bridge/             the only layer that touches oxc_ast/oxc_parser/oxc_semantic
    context.rs        CheckContext: bundles arena/namespace/symbols/diagnostics/narrow/this state
    parse.rs          Oxc parser + oxc_semantic bind
    declare.rs        pass 1: hoist top-level types and signatures
    statements.rs     pass 2: statement walking, including if/else narrowing dispatch and classes
    expressions.rs    pass 2: expression -> TypeId inference and checks
    narrow.rs         typeof/equality/truthiness narrowing
tests/
  v1_fixtures.rs .. v3c_fixtures.rs   fixture-driven integration tests, one file per version
  fixtures/v1/ .. fixtures/v3c/       .ts input files paired with expected diagnostics
benches/
  checker_benchmark.rs   Criterion benchmarks (tiny, 50-fn, 1000-fn, parse+bind isolation)
pkg-template/
  package.json        npm metadata reference for the published WASM package
```

## Building

```sh
cargo test                                    # native, see docs/VERIFICATION.md
cargo bench                                   # performance, see docs/VERIFICATION.md
wasm-pack build --target web --features wasm  # npm/WASM
```

See `docs/WASM_BUILD.md` for full build/publish steps and
`docs/VERIFICATION.md` for what to check before trusting a first green run.

## Contributing

This project is designed to be contributable in small, scoped pieces. Each
version in `docs/ROADMAP.md` lists "good first issues" tied to specific
modules. Please read `docs/DESIGN.md` first: architectural decisions and
their rationale are documented there so PRs can be reviewed against an
agreed design rather than re-litigated per PR. Every feature ships with a
fixture and a matching test; every known limitation gets its own
`known_gap_*.ts` fixture asserting the *current* behavior, so a gap
closing shows up as a test needing an update, not a silent behavior change.

## License

Apache-2.0.
