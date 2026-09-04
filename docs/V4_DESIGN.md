# V4 — Scope & Architecture

**Goal of V4:** cover enough of real-world TypeScript that a typical
application file produces real diagnostics instead of "not yet checked"
warnings. This is the version before 1.0.0 — anything not in scope here
is explicitly deferred, not forgotten.

**Definition of done for V4:** run the checker over a handful of real
open-source `.ts` files (not fixtures) and the diagnostic output is
dominated by genuine type errors/warnings, not by the wildcard
`"This expression kind is not yet checked by ts-rust."` /
`push_unsupported` fallback.

---

## 1. Where V3c actually leaves off

Everything below currently falls through to the wildcard arm in
`infer_expression_type` (`bridge/expressions.rs`) or the unsupported
statement path in `bridge/statements.rs`. This is the real gap surface —
not a hypothetical one:

**Expressions with no type at all today:**
arrow functions, function expressions, `&&` / `||` / `??`,
`?:` (ternary), assignment expressions, template literals,
`as` / `!` assertions, computed member access (`obj[key]`),
optional chaining (`?.`).

**Statements with no handling today:**
`for`, `while`, `switch`, `try`/`catch`, `throw`, `import`/`export`.

Because arrow functions and function expressions aren't typed, any file
using callbacks (`arr.map(x => ...)`, `const f = (x) => ...`) loses
tracking immediately. This is the single highest-leverage gap.

---

## 2. V4 feature scope

### Tier 1 — foundational, build first

| # | Feature | Why it's first |
|---|---|---|
| 1 | Arrow functions / function expressions as values | Everything else (callbacks, higher-order functions, most real code) depends on these having a `FunctionType`. `declare.rs` currently only registers top-level `function` declarations. |
| 2 | Logical expressions (`&&`, `||`, `??`) | Own type is a union of operand types, but the real payoff is narrowing: `x && x.y` and `x ?? fallback` are the two most common real narrowing idioms. `narrow.rs`'s branch-splitting logic already exists for `if` — this generalizes it to short-circuit operators. |
| 3 | Ternary (`ConditionalExpression`) | Mechanically an expression-level `if`: infer the test, narrow each branch, union the two results with `arena.alloc_union`. Cheap once (2) exists. |
| 4 | Type assertions (`as`, non-null `!`) | Common escape hatches. Without these, any file using them loses tracking of the asserted value for the rest of its scope. |

### Tier 2 — high frequency, build after Tier 1

| # | Feature | Notes |
|---|---|---|
| 5 | Narrowing survives early return | `if (!x) return; x.foo` is arguably the most common real narrowing pattern, and is currently a documented known gap (`known_gap_narrowing_does_not_persist_past_if.ts`). Needs `NarrowState` to persist past a block that provably exits (`return`/`throw`/`continue`/`break` on every path), not just live inside the `if`. |
| 6 | Computed member access (`obj[key]`) + optional chaining (`?.`) | `?.` is everywhere once any nullable type exists in a file. |
| 7 | Enums | Structurally simple — a closed set of named literal members — compared to what comes after it. |
| 8 | `for` / `while` / `switch` bodies | Even walking the body without new type logic beats degrading to unsupported. `switch` on a discriminant is a common real narrowing shape and can reuse (2)'s branch-splitting machinery. |

**Suggested build order:** 1 → 2 → 3 → 5 → 4 → 6 → 7 → 8. Doing
narrowing-persistence (5) right after short-circuit operators (2) means
`NarrowState` only needs to be generalized once, instead of once for
`if`, again for `&&`, again for early-return.

### Explicit non-goals for V4 (deferred past 1.0.0, not cut)

- **Generics.** Likely the largest single gap versus real TypeScript by
  volume (`Array<T>`, `Promise<T>`, generic functions/classes), but a
  different order of magnitude of work — type parameter binding,
  call-site inference, and variance interacting with the existing
  subtyping relation. Mixing this into V4 risks landing neither
  Tier 1/2 nor generics solidly before 1.0. Treat as its own major
  version.
- **Modules** (`import`/`export` resolution across files) — a module
  graph and `.d.ts` handling is an infrastructure project, separable
  from the type-checking logic itself.
- **Mapped / conditional / template-literal types** — advanced even in
  real `tsc`; lower real-world density per file than the Tier 1/2 list.

---

## 3. Architecture: single-threaded, in-memory, cache-friendly

V4 should keep — and tighten — the data-oriented shape already
established in V1–V3c, rather than introducing new patterns as new
features land.

### Why single-threaded is the right choice here, not a limitation

- `TypeArena` and `SymbolTypeMap` are both mutated throughout a single
  check pass via `&mut`. Making that `Sync` would mean interior
  mutability (`RwLock`/`Mutex`) on every type allocation and symbol
  lookup — paying a synchronization cost on the hottest path in the
  program for a benefit V4 doesn't need.
- If parallelism is ever wanted, it belongs **between** files (checking
  N independent files on N threads, each with its own arena), not
  **inside** one file's check. That boundary is already natural here:
  a `TypeChecker` run is a self-contained `TypeArena` + `SymbolTypeMap`
  + `Namespace` with no cross-file state. Keep it that way — don't let
  any V4 feature (module resolution eventually) introduce shared
  mutable state between checker instances.
- Single-threaded also keeps the existing correctness argument simple:
  `resolving: false` cycle-detection in `namespace.rs`, and the
  save/restore pattern for `current_return_type` /
  `current_class_instance` in `context.rs`, both rely on ordinary
  sequential control flow. Concurrency would turn each of those into a
  much harder problem for no current benefit.

### Cache-friendliness: what's already right, what to fix in V4

**Already correct — keep doing this:**
- `TypeArena` is a flat `Vec<Type>` indexed by a 4-byte `TypeId(u32)`.
  Copying a `TypeId` around (function returns, `HashMap` keys, narrowed
  branch maps) is a register-sized copy, not a pointer chase.
- `ObjectType.properties` is kept sorted at construction so
  `object_is_subtype` does a single linear merge-join instead of a
  lookup per property. Any new object-shaped feature (index signatures
  in Tier 2's enum work, if they end up sharing representation) should
  preserve this invariant rather than reach for a `HashMap`.
- `alloc_union`'s dedup now compares `Type` values structurally
  (`self.get(existing) == self.get(id)`), not by `TypeId` — correctness
  fix already landed, keep it that way as more `Type` variants arrive
  (Tier 1's function-value types will go through the same union paths
  once narrowing unions callback-shaped values).

**Concrete thing to fix as part of V4, not after:**
- `SymbolTypeMap` (`symbol_map.rs`) and `NarrowState`
  (`bridge/narrow.rs`) are both `HashMap<SymbolId, TypeId>`. `SymbolId`
  from `oxc_semantic` is a dense, sequential index — every symbol in a
  file gets the next integer as it's bound. That means a `HashMap` here
  is doing hash-then-probe work, and scattering entries across the
  heap, for a key space that's already a perfect fit for a flat
  `Vec<Option<TypeId>>` indexed directly by the symbol's underlying
  index: O(1) direct indexing instead of hashing, sequential memory
  instead of scattered buckets, and no `Hash` impl needed on the key at
  all. `NarrowState` gets created and thrown away per-branch far more
  often than `SymbolTypeMap` does, so it's the higher-value target —
  but both should move together since they're the same shape of
  problem. This directly serves V4: every new Tier 1/2 feature adds
  more branches (`&&`, ternary, `switch`) that allocate a fresh
  `NarrowState`, so fixing the representation now avoids compounding
  the cost as narrowing sites multiply.
- Before allocating a new `Type` variant for a Tier 1/2 feature (e.g.
  arrow-function types, enum member types), check whether it can be
  represented with an *existing* variant plus a flag, the way
  `FunctionType.is_untyped` extended `FunctionType` rather than adding
  a parallel type — fewer `Type` variants means fewer match arms
  everywhere that touches `Type`, and a smaller, more cache-friendly
  enum (enum size is driven by its largest variant).

### General rules for new V4 code

- No `Box<dyn Trait>` anywhere in the type representation — it defeats
  the flat-arena design and adds a vtable indirection to what should be
  a `u32` compare.
- No `Rc<RefCell<_>>` for anything reachable from `TypeArena` or
  `Namespace` — single-threaded and single-owner already; reach for
  ordinary `&mut` and the existing save/restore-on-`CheckContext`
  pattern instead.
- New per-branch or per-scope state (narrowing, upcoming loop-body
  state for Tier 2) follows the `current_return_type` /
  `current_class_instance` pattern already in `CheckContext`: a field
  that's swapped out and restored via `std::mem::replace` /
  `Option::replace`, not a stack of allocations pushed and popped
  per node.

---

## 4. Testing philosophy carried into V4

Continue the existing pattern exactly: every feature ships with a
`tests/fixtures/v4/*.ts` fixture and a matching assertion in
`tests/v4_fixtures.rs`, and every known limitation gets an explicit
`known_gap_*.ts` fixture that asserts the *current* (limited) behavior
— so a gap closing shows up as a test that needs updating, not a
silent behavior change. `docs/ROADMAP.md`'s gaps list and the fixture
suite should never drift apart.
