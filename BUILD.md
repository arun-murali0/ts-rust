# ts-rust - Build Stages (Real-World-First Order)

Derived from checklist.md. Reordered to prioritize the type-system features and
real-world code patterns most commonly hit in everyday TypeScript, ahead of
theoretically "foundational" but less-frequently-used advanced type operators.

Module resolution uses `oxc_resolver` for path/specifier resolution (node_modules
walk, package.json exports/main/types, tsconfig paths/baseUrl, extension probing).
Everything checker-side (symbol tables, cross-file lookup, .d.ts parsing,
import-cycle handling) is still ts-rust's own work.

---

## Stage 1 - Generics completion

Section 6, remaining items (most of §6 is already `[x]`). Shipped since: inference
that keeps literals under a primitive or literal bound, explicit type arguments that
stay in position when one cannot be resolved, and a fingerprint-keyed instantiation
memo.
- Inference from return positions (full)
- Inference priority / multiple candidates (full algorithm)
- Contextual inference into generic calls
- Higher-order function inference
- Constraint checking under substitution (full)
- Generic type parameters used as type arguments to other generics
- `const` type parameters
- Variance modifiers (`in`/`out`)

Why first: highest real-world density. Everyday generic code (`map`, `reduce`,
curried functions, factory functions) routinely hits inference edge cases that
are currently unhandled. No new `Type` variant needed - this deepens the
existing substitution/inference code in `semantic/generics.rs`.

Dependencies: none. Extends existing code.

---

## Stage 2 - Module resolution (foundation)

Section 11, core subset:
- ES module imports/exports (types + values)
- Re-exports, default exports
- `import type` / `export type`
- Cross-file symbol lookup

Path/specifier resolution delegated to `oxc_resolver`. Remaining work is
checker-side:
1. Module graph/driver: resolve each import via `oxc_resolver`, decide check
   order, handle import cycles (file-level cycle-guard, parallel to the
   existing type-level `seen` stack pattern).
2. Extend `namespace.rs`'s symbol-table concept so each checked file exposes
   an export table, and import sites look into another file's table instead
   of only their own scope.
3. `import type` as a checker-side distinction (not resolver-side).

Why here: unblocks every other stage from being single-file-only, which is
the single biggest realism gap in real-world usage - almost no real
TypeScript project is one file.

Dependencies: none structurally, but benefits from Stage 1 being stable so
generic types resolve correctly once they can cross file boundaries.

---

## Stage 3 - Tuples & intersections

Section 1.4:
- Tuple types, readonly tuples, optional/rest elements
- Intersection types, intersection reduction
- Union reduction / subsumption

Why here: extremely common in real code (function signatures with fixed-shape
arrays, `type A = B & C` patterns, discriminated union members). Builds on
existing `Array` type and subtyping/cycle-guard code; no dependency on
Stages 1-2, but ordered after them since generics and modules touch more of
the codebase's core flow.

Dependencies: none.

---

## Stage 4 - Narrowing upgrades

Section 9. Shipped: `instanceof`, `in`, discriminated unions (a property of an
identifier, in `if` and `switch`), `switch` grouped labels / default / `typeof`,
`&&` / `||` conditions, assignment narrowing, branch joins and loop exit
narrowing. See docs/control-flow-narrowing.md.

Remaining:
- Switch exhaustiveness
- User-defined type predicates
- Assertion functions (`asserts x is T`)
- Property-path narrowing (`a.b.kind`, `a.b !== null`), which needs the narrowing
  key to become a path instead of a symbol
- Equality between two variables, optional-chain narrowing, `switch (true)`
- A join after `switch`, and narrowing of captured variables inside closures

Why here: the shipped forms cover the everyday patterns (error handling, API
response shapes, class hierarchies). The remaining items each need something new
(a path-shaped key, a predicate type on function types, an exhaustiveness check
against `never`) so they are separate pieces of work, not more handlers.

Dependencies: property paths and predicates are independent of Stage 3.

---

## Stage 5 - `keyof` / indexed access

Section 8.1 (partial):
- `keyof T`
- `T[K]`

Why here: common in real code (`Pick`-style helper types, generic property
accessors), but less frequently hand-written directly than tuples/narrowing.
Hard prerequisite for Stage 6.

Dependencies: Stage 3 (tuples/intersections need keys/indices to describe).

---

## Stage 6 - Mapped & conditional types

Section 8.2, remainder of 8.1:
- `T extends U ? X : Y`, distributive conditionals, `infer`
- Homomorphic / non-homomorphic mapped types
- Key remapping (`as`), `+readonly`/`-readonly`/`+?`/`-?`

Why here: powers most of the standard library (Stage 7) but is less
frequently hand-authored directly in application code compared to
Stages 1-5. Heaviest recursion; will need to generalize the existing
cycle-guard pattern from `subtyping.rs`/`semantic/generics.rs`.

Dependencies: Stage 5.

---

## Stage 7 - Standard library utility types

Section 14:
- `Pick`, `Omit`, `Partial`, `Required`, `Record`, `Readonly`
- `ReturnType`, `Parameters`, `InstanceType`, `Awaited`
- `NonNullable`, `Extract`, `Exclude`

Why here: extremely common in real code, but mechanically just named
mapped/conditional types - cheap once Stage 6 exists, one-off hacks if
attempted earlier.

Dependencies: Stage 6.

---

## Stage 8 - Overload resolution

Section 3.3, 3.4, 4, 5:
- Multiple call/construct signatures
- Method overloads, overload declarations, implementation checking, resolution
- Generic call signatures

Why here: real-world-common (especially in library-style code and DOM-like
APIs) but touches the most call sites (functions, methods, constructors) -
placed after the type-shape stages so overload candidates compare against a
richer type system.

Dependencies: none required; ordered after Stage 6 for practicality.

---

## Stage 9 - Contextual typing

Section 10:
- Contextual typing of parameters, return, object/array literal
- Best common type / candidate unions
- Generic contextual inference

Why last: real-world-important (this is what makes callback parameter types
"just work" without annotations) but the largest cross-cutting pipeline
change - requires threading an expected type through expression inference
everywhere, not a localized addition. Already flagged as unimplemented in
existing code comments.

Dependencies: none required; ordered last due to size/risk.

---

## Set aside - full module ecosystem features

Beyond the Stage 2 foundation:
- Ambient modules (`declare module`), wildcard ambient modules
- Namespace merging, module/global augmentation
- `.d.ts` parsing for node_modules packages
- UMD / global scripts vs modules

Why separate: `oxc_resolver` gets you a file path; parsing real `.d.ts`
ambient declarations and handling augmentation/merging across packages is
substantially more work than the Stage 2 foundation and isn't needed to
check most everyday application code (as opposed to library-consuming code).

---

## Also excluded from staging - JSX/TSX (Section 12), JS interop (Section 13)

Depend on module/scope concepts; picked up after the module ecosystem work
above, not sequenced here.

---

## Stage dependency summary

| Stage | Depends on | Parallel with |
|---|---|---|
| 1 - Generics completion | none | 2 |
| 2 - Module resolution (foundation) | none | 1 |
| 3 - Tuples & intersections | none | 4 |
| 4 - Narrowing upgrades | none (loosely 3) | 3 |
| 5 - keyof / indexed access | 3 | - |
| 6 - Mapped & conditional types | 5 | - |
| 7 - Stdlib utility types | 6 | - |
| 8 - Overload resolution | none (ordered after 6) | 3-6 |
| 9 - Contextual typing | none (ordered last) | any |
| Full module ecosystem (.d.ts, augmentation) | 2 | tracked separately |
| JSX / JS interop | module ecosystem | tracked separately |
