# Roadmap

## North star

Full practical TypeScript type-checking, in the spirit of `tsgo` (Microsoft's
native port of `tsc`), but built fresh in Rust on top of Oxc's parser/AST,
rather than a port of tsc's existing implementation.

**This is a multi-year goal.** V1-V4 below are the honest, incremental path
toward it. We do not claim tsc parity until we can measure it, see
"Measuring progress" at the bottom.

Ezno (kaleidawave/ezno) is used as architectural reference only. No source
code is copied from it; this project depends on nothing from Ezno's parser
or compiler. Where relevant, design decisions note what we learned from
Ezno's public writeups and what we deliberately do differently.

---

## V1. Core skeleton ("it walks")

Status: implemented and passing its fixture suite (see `docs/VERIFICATION.md`).

- [x] Oxc-based parsing entry point
- [x] Type arena (`TypeId` + `Type` enum): `number`, `string`, `boolean`,
      `null`, `undefined`, `any`, `unknown`, plus an internal `Error`
      sentinel to stop failed inference from cascading into unrelated
      mismatches
- [x] Two-pass checking: hoist top-level declarations, then check bodies
- [x] Subtyping for primitives + `any`/`unknown`
- [x] Diagnostics: variable declaration mismatch, function param/return
      mismatch, binary operator type errors
- [x] Explicit unsupported-construct diagnostics for anything not yet
      handled, never silently pass or panic

Out of scope for V1: generics, classes, objects, unions, narrowing, imports,
async.

**Good first issues:** add a new primitive check; add a new binary operator
rule; add a test fixture pair.

---

## V2. Structural types

Status: implemented and covered by `tests/v2_fixtures.rs`.

- [x] Object types with property maps and structural (width) subtyping
- [x] Union types, checked member by member in both directions
- [x] Array types (`T[]`), covariant in the element type
- [x] Type aliases and interfaces registered as named types, resolved
      lazily so forward references work in either declaration order
- [x] Function call checking: arity and parameter types (no overloads yet)
- [ ] Narrowing (`typeof`, equality checks): deferred to V3 along with the
      rest of flow-sensitive control flow; V2's unions and objects are
      static types only

**Good first issues:** implement subtyping for one new shape (tuples, one
union case), isolated in `subtyping.rs` with fixtures.

---

## V3. Control flow & functions as first-class

- [ ] Classes: fields, methods, structural subtyping incl. inheritance
- [ ] Control-flow narrowing: `if`/`else`, truthiness, discriminated unions
- [ ] Basic generics (parametric functions/types, no variance edge cases)
- [ ] Closures: captured-variable type tracking across scopes
- [ ] `async`/`await`, `Promise<T>`

**Good first issues:** own one control-flow construct (if/else, switch,
loop) and its narrowing algorithm, documented in its own module.

---

## V4. Ecosystem integration

- [ ] `.d.ts` parsing + a `lib.d.ts` subset (not full parity)
- [ ] Module resolution (Node-style, minimal `package.json` support)
- [ ] Editor-facing diagnostics API (LSP-shaped, not a full LSP server)
- [ ] Incremental re-checking on file change

This is expected to be the largest phase. Full `.d.ts`/ambient type support
and module resolution are where scope grows fastest.

---

## V5+ (post-V4, toward the north star)

- [ ] Generics with proper variance
- [ ] Overload resolution
- [ ] Conditional / mapped / template-literal types
- [ ] Run against TypeScript's own conformance suite
      (`microsoft/TypeScript` `tests/cases/compiler`) and track pass rate

From this point on, divergence from tsc's documented behavior is treated as
a bug, not a design choice. Compatibility becomes the goal, not elegance.

---

## Measuring progress

Rather than promising a timeline, progress is tracked via:

1. **Fixture pass rate**. `tests/fixtures/vN/*.ts` + expected diagnostics,
   grown every PR.
2. **TS conformance suite pass rate** (from V5 onward), the same metric any
   tsc-alternative should report, and the only honest measure of "how close
   to tsgo are we."

No version above claims a completion date. Checkboxes are the unit of
progress, not calendar time.
