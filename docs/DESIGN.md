# ts-rust: System Design

Status: V1 through V3c implemented and tested (42 integration fixtures, 29
unit tests, 1 doctest). V4 is planned (section 4.4) but not started. This
document describes what was actually built, not just a plan; where an
earlier draft described something differently from what shipped, that's
called out explicitly below rather than quietly edited away.

## 0. Purpose & scope

`ts-rust` is a TypeScript type checker written from scratch in Rust, using
Oxc for parsing and semantic analysis, targeting both native (Rust
library) and WASM (npm package). North star is broad practical parity
with `tsc`, in the spirit of `tsgo`, but this is a multi-year goal reached
through measurable, versioned phases.

Ezno is used as architectural reference only. No source code is copied
from it; this project depends on nothing from Ezno's parser or compiler.
Every design choice below either follows a lesson learned from studying
Ezno's public writeups, or explicitly departs from it with a reason.

---

## 1. Core architectural decisions

### 1.1 Arena-allocated types (`TypeId` over `Box<Type>`)

All `Type` values live in one flat, append-only store (`TypeArena`);
consumers hold a `Copy` `TypeId(u32)` handle rather than an owned/boxed
type. Rationale:

- Recursive/cyclic types (e.g. `type Tree = { children: Tree[] }`) are
  trivial with indices, painful with an owned tree. (Note: recursive
  *named* types aren't supported yet regardless, see 1.2. This point is
  about why the data structure doesn't get in the way once they are.)
- Cheap equality/hashing (`u32` compare) instead of structural comparison.
- Matches proven precedent (rustc's interner, Ezno's `TypeStore`) without
  reusing any of their code.

### 1.2 Two-pass checking: declare, then check

Pass 1 ("declare") walks a scope's top-level statements and registers the
*signature* of every function, class, and annotated variable, plus, for
`type`/`interface`/`class` names, just the *name* itself, unresolved. Pass
2 ("check") walks statement/expression bodies, resolving identifiers
against what pass 1 registered.

Named types (`type`, `interface`, `class`) resolve **lazily**, on first
use, rather than eagerly during pass 1. This is what lets `type A = B;
interface B { x: number }` work regardless of declaration order; eager
resolution of `A` would need `B` to already exist. A `resolving` flag on
each entry in `TypeNamespace` catches a name that refers back to itself
(directly or through another alias) mid-resolution and returns
`Resolution::Circular` instead of recursing forever; this is treated as
an `Unsupported` case, not a checker bug
(`tests/fixtures/v2/circular_type_reference.ts`).

This two-pass-plus-lazy-resolution shape is the same idea Ezno calls
"synthesis vs checking." We keep the *idea*, not the code.

### 1.3 Subtyping as the single relation everything routes through

One function, `is_subtype(sub, sup) -> bool`, is the sole authority for
"is X assignable where Y is expected." Variable declarations, function
calls, return statements, and `new` expressions all call into it rather
than each having their own ad hoc comparison logic. This keeps
correctness centralized and testable in one file (`subtyping.rs`).

### 1.4 Narrowing: branch-local, not a general effects system

Ezno's checker tracks side effects (assignment, calls, etc.) through a
general "events" system that its own maintainer has described as complex
and, in practice, falling short for real-world code. We deliberately do
not build a general effects system. Instead, narrowing (`bridge/narrow.rs`)
is a `SymbolId -> TypeId` override map:

1. An `if` statement's test expression is pattern-matched for one of
   three recognized shapes: `typeof x === "tag"`, `x === null`/`x ===
   undefined`, or bare truthiness (`if (x)`). Anything else narrows
   nothing.
2. A recognized shape produces two override maps, one for the `true`
   branch, one for `false`.
3. The override map is applied, the branch is checked, then the override
   map is **restored to what it was before the `if` statement**, not
   merged or joined.

**This is a smaller scope than originally planned, and smaller than real
TypeScript.** An earlier draft of this document described narrowing as
"merged back at branch join points"; that join/persistence behavior was
not built. Concretely: `if (x === null) { return; } return x.trim();`
does **not** narrow `x` for the `return x.trim();` line, even though real
TypeScript (and the earlier plan for this checker) would. This is
demonstrated, not hidden, by
`tests/fixtures/v3b/known_gap_narrowing_does_not_persist_past_if.ts`.
**Closing this gap is V4's Tier 2, item 5** (section 4.4) — it needs
control-flow analysis (recognizing that a branch always returns/throws),
not just branch-local overriding.

Discriminated unions (narrowing a union of objects by a shared
literal-typed tag property, e.g. `shape.kind === "circle"`) are also not
implemented; narrowing only recognizes a bare identifier on one side of
`===`, not a member expression.

### 1.5 Explicit "unsupported" over silent gaps

Any AST node shape the checker doesn't yet handle produces a
`Warning`-severity `Diagnostic` naming the construct, never a silent skip
or an incorrect "no error" result. Every deliberately-deferred feature in
this document has a fixture proving it degrades this way rather than
crashing or silently passing; see `tests/fixtures/*/`.

### 1.6 Decoupled diagnostics

`Diagnostic`/`Severity` are our own types, independent of Oxc's or any
internal representation, so the public API is stable even as internals
change across versions.

### 1.7 Platform split: pure core, thin platform shells

```
core (arena, types, subtyping, bridge, diagnostics), no platform deps
  |-- native shell: plain Rust API, std allowed
  `-- wasm shell: wasm-bindgen, serde, no fs/threads assumptions
```

The core never assumes filesystem access or threading; anything future
module resolution needs from disk will be injected via a trait
(`FileResolver`, not yet built) so the WASM build can supply an
in-memory/host-provided implementation instead.

### 1.8 `oxc_semantic` for identifier resolution, not a hand-rolled scope chain

V1 shipped with a hand-rolled parent-pointer scope chain (`scope.rs`).
This was replaced in V2 by `oxc_semantic`'s real binder: every binding
gets an oxc-assigned `SymbolId`, and `SymbolTypeMap` is the only thing
`ts-rust` itself owns for identifier-to-type resolution. `scope.rs` no
longer exists. This was a deliberate correctness upgrade, not a refactor
for its own sake: a hand-rolled scope chain gets `var` hoisting, the
temporal dead zone, and per-iteration `let` in `for` loops subtly wrong
in ways that are easy to miss until a real file hits the edge case;
`oxc_semantic` is the same binder oxc's own linter relies on in
production.

### 1.9 `CheckContext`: one bundle, not N parameters

By V3b, every statement/expression-checking function needed the type
arena, the type namespace, the symbol map, the diagnostics list, the file
name, and narrowing state. V3c added one more: the enclosing class's
instance type, so `this` resolves correctly inside a method or
constructor body. Threading all of this as separate parameters would mean
touching every function signature in `bridge/` every time one more thing
needs tracking. `bridge/context.rs`'s `CheckContext` bundles all of it
into one struct, passed as `&mut CheckContext`. `scoping` (from
`oxc_semantic`) stays a separate parameter since it's read-only and set
once, unlike everything else in the bundle.

Two fields follow the same save/restore pattern, via `Option::replace`:
`current_return_type` (set entering a function/method body, restored on
exit, so a `return` inside a nested `if`/block still checks against the
right type) and `current_class_instance` (set entering a class's
constructor or method body, restored on exit, so `this` resolves without
being threaded through every call). Any new per-scope state added in V4
(e.g. loop-body state) should follow this same pattern rather than a
pushed/popped stack allocation per node — see section 4.4's "general
rules for new V4 code."

### 1.10 Data structure choice follows key density and threat model, not habit

Three lookup structures, three different right answers, chosen on
purpose rather than defaulting to `HashMap` everywhere:

- **`SymbolTypeMap`** is a `Vec<Option<TypeId>>` indexed directly by
  `SymbolId::index()`, not a `HashMap`. `oxc_semantic` assigns every
  symbol in a file a dense, sequential `SymbolId` up front, and by the
  time checking finishes nearly all of them end up registered here — a
  dense key space is exactly what a `Vec` is for. This mirrors what
  `oxc_semantic` does internally for its own symbol table (`spans`,
  `names`, `flags` are all index-vectors, not hash maps). Direct
  indexing skips both the hash computation and the bucket-array
  indirection a `HashMap` still pays on every lookup.
- **`TypeNamespace`**'s `String`-keyed map uses an in-tree
  `FxHashMap` (`fxhash.rs`) instead of the stdlib default. The stdlib's
  default hasher (SipHash) pays for DoS-resistance so that
  attacker-controlled input can't be crafted into worst-case hash
  collisions; nothing hashed here is adversarial (type/interface/class
  names come from source the caller already chose to check), so paying
  that cost buys nothing. Vendored in-tree rather than pulled in as a
  dependency on the `rustc-hash` crate: it's a few dozen lines with no
  dependencies of its own.
- **`NarrowState`** stays a small linear-scanned structure keyed by
  `SymbolId`, deliberately *not* the same dense `Vec` shape as
  `SymbolTypeMap`, even though the key type is identical. A narrowing
  overlay is created fresh per branch and typically holds a handful of
  entries out of however many symbols exist in the whole file; a `Vec`
  sized to the total symbol count would allocate memory proportional to
  file size on every single branch. Same key type, opposite right
  answer: the deciding factor is whether the *use* is dense or sparse,
  not the key type alone.

`alloc_union`'s member deduplication compares `Type` values structurally
(`self.get(existing) == self.get(id)`), not by `TypeId`. Two separately
allocated `StringLiteral("a")`s get different `TypeId`s even though
they're the same type; comparing IDs directly let `"a" | "a"` through as
a two-member union instead of collapsing to `"a"`. This was a real,
fixed bug, not always the case — see `tests/subtyping.rs` for the
regression test.

---

## 2. Data model

```
TypeId(u32): handle into TypeArena

Type (V1):     Number | String | Boolean | Null | Undefined | Any | Unknown
                     | Error   -- internal sentinel, see 1.5 and TypeArena::error
                     | Function(FunctionType)
Type (V2 adds): Object(ObjectType) | Union(Vec<TypeId>) | Array(TypeId)
                     -- named aliases/interfaces are NOT a Type variant; a
                        name resolves to one of the TypeIds above via
                        TypeNamespace (see 1.2)
Type (V3a adds): StringLiteral(String) | NumberLiteral(f64) | BooleanLiteral(bool)
Type (V3b adds): Never  -- bottom type; produced when narrowing empties a union
Type (V3c adds): (none) -- a class instance IS an Object(ObjectType);
                            a constructor IS a Function(FunctionType) whose
                            return type is the instance's Object TypeId

FunctionType { params: Vec<TypeId>, return_type: TypeId }
ObjectType   { properties: Vec<PropertyEntry> }  -- sorted by name at construction,
                                                      so subtyping is a merge-join
PropertyEntry { name: String, type_id: TypeId, optional: bool }

TypeNamespace<'ast>          -- type/interface/class names -> TypeId, lazy + memoized
  entries: FxHashMap<String, TypeEntry<'ast>>    -- see 1.10
  TypeEntry { kind: DeclKind, resolved: Option<TypeId>, resolving: bool }
  DeclKind: TypeAlias(&TSType) | Interface(&TSInterfaceDeclaration) | Class(&Class)

SymbolTypeMap                -- oxc_semantic's SymbolId -> TypeId, replaces V1's Scope
  types: Vec<Option<TypeId>>  -- indexed by SymbolId::index(), see 1.10

NarrowState  -- branch-local override map keyed by SymbolId, sparse — see 1.4, 1.10

CheckContext<'ast, 'src> {
  arena: TypeArena,
  namespace: TypeNamespace<'ast>,
  symbols: SymbolTypeMap,
  diagnostics: Vec<Diagnostic>,
  file_name: &'src str,
  narrow: NarrowState,
  current_return_type: Option<TypeId>,     -- see 1.9
  current_class_instance: Option<TypeId>,  -- see 1.9, resolves `this`
}

Diagnostic { severity, message, file_name, start: u32, end: u32 }
```

This is what actually exists, not a forward-looking sketch.

---

## 3. Module map (as built)

```
src/
  lib.rs              public API (TypeChecker, CheckResult), feature-gates wasm
  arena.rs            TypeId, TypeArena (incl. alloc_union: flattens/dedupes/drops Never)
  types.rs            Type enum, PropertyEntry, widen()
  subtyping.rs        is_subtype() + unit tests
  symbol_map.rs       SymbolTypeMap (SymbolId -> TypeId), Vec-indexed
  namespace.rs        TypeNamespace: type/interface/class names, lazy resolution, FxHash-keyed
  type_annotation.rs  resolves a TSType annotation to a TypeId; shared
                       resolve_function_params/resolve_object_members helpers
  fxhash.rs           in-tree FxHash implementation, see 1.10
  diagnostics.rs      public Diagnostic/Severity types
  error.rs            CheckerError
  wasm.rs             feature-gated wasm-bindgen boundary
  bridge/             the only layer touching oxc_ast/oxc_parser/oxc_semantic directly
                       (namespace.rs and type_annotation.rs also touch oxc_ast,
                        since resolving a type annotation needs it, but neither
                        touches oxc_parser or oxc_semantic)
    mod.rs
    context.rs        CheckContext (see 1.9)
    parse.rs          Oxc Parser wrapper + oxc_semantic bind (parse + analyze)
    declare.rs        pass 1: hoist top-level types, signatures, and class constructors
    statements.rs     pass 2: statement walking, if/else narrowing dispatch,
                       function/class body checking, return-type checking, `this` scoping
    expressions.rs    pass 2: Expression -> TypeId inference and checks,
                       including call/new-expression arity checking, ThisExpression
    narrow.rs         typeof/equality/truthiness narrowing (see 1.4)
```

Not present yet, still planned: `resolve/` (a `FileResolver` trait +
impls), any generics/closures/async support, the V4 feature set below.

---

## 4. Version specs

### 4.1 V1: Core skeleton ("it walks"), done

**Shipped**
- Types: `number, string, boolean, null, undefined, any, unknown`, plain
  (non-overloaded) function signatures.
- Two-pass checking on a single file.
- Subtyping: reflexive + `any`/`unknown` special cases + function
  contravariant-params/covariant-return.
- Diagnostics: mismatch on `const/let x: T = expr`, function argument vs.
  parameter, `return expr` vs. declared return type, plus `Unsupported`
  for anything else.
- Identifier resolution originally used a hand-rolled scope chain
  (`scope.rs`); replaced in V2 by `oxc_semantic` (see 1.8). `scope.rs` no
  longer exists.

**Exit criteria met:** `tests/v1_fixtures.rs`, 7 fixtures.

### 4.2 V2: Structural types, done

**Shipped**
- `Object` type: `Vec<PropertyEntry>` sorted by name at construction, so
  `subtyping.rs` compares two objects with a merge-join instead of an
  O(n·m) `.find()` per property. Width subtyping: `sub` is a subtype of
  `sup` iff every *required* property in `sup` exists in `sub` with a
  compatible type; an optional property in `sup` doesn't need to exist in
  `sub` at all; extra properties in `sub` are fine. The excess-property
  check TypeScript applies to *fresh object literals* specifically (not
  general width subtyping) is **not implemented**; known gap, see
  `tests/fixtures/v2/excess_property_literal.ts`.
- `Union(Vec<TypeId>)`: subtype of a union iff subtype of *any* member; a
  union is a subtype of `sup` iff *all* members are. Built exclusively
  through `TypeArena::alloc_union`, which flattens nested unions,
  deduplicates members *by structural value* (see 1.10), and drops
  `Never` members (collapsing to a single type, or to `Never` itself,
  when that leaves 0-1 members).
- `Array(TypeId)`: covariant element subtyping (matches TS's
  unsound-but-practical array variance; documented as intentional).
- `type` aliases and `interface` declarations, resolved lazily through
  `TypeNamespace` (see 1.2), so forward references and either declaration
  order both work. Self-referential names are caught by the `resolving`
  flag and treated as `Unsupported`, not a crash; recursive named types
  (`type Tree = { children: Tree[] }`) are a known, deliberate gap.
- Function calling gains: excess/missing argument count errors.
- Real semantic analysis via `oxc_semantic` replaces V1's `scope.rs` (see
  1.8). `SymbolTypeMap` is the only identifier-resolution state
  `ts-rust` itself owns.

**Narrowing:** not in V2, as originally planned; it's V3b (4.3.2 below).

**Exit criteria met:** `tests/v2_fixtures.rs`, 15 fixtures.

### 4.3 V3: what actually shipped, split by sub-version

The original plan bundled narrowing, classes, generics, closures, and
async into one "V3." In practice this was split into smaller, separately
fixture-verified steps, the same discipline V1/V2 already used, applied
consistently rather than abandoned once the feature list got longer.

#### 4.3.1 V3a: Literal types, done

- `StringLiteral(String)`, `NumberLiteral(f64)`, `BooleanLiteral(bool)`
  added to `Type`. `f64`'s lack of `Eq` means `Type` itself dropped its
  `Eq` derive (kept `PartialEq`); nothing else depended on `Type: Eq`.
- Literal-vs-literal (equal value only) and literal-vs-base-primitive
  subtyping rules in `subtyping.rs`.
- `TSLiteralType` support in `type_annotation.rs`, so literal type
  *annotations* (`"up" | "down"`) resolve, not just literal *values*.
- Widening rule (`types::widen`): `const` keeps the literal type; `let`/
  `var` without an explicit annotation widen to the base primitive.
  Applied at variable-declaration time and inside object/array literal
  construction (so `const arr = [1, 1, 1]` produces `Array(Number)`, not
  a union of three separately-allocated-but-equal number literals).

**Exit criteria met:** `tests/v3a_fixtures.rs`, 6 fixtures.

#### 4.3.2 V3b: Scoped `if`/`else` narrowing, done, smaller scope than planned

See 1.4 for the full design and what's deliberately not included (no
persistence past the `if`, no discriminated unions — both planned for
V4, section 4.4). Also added in this step, since narrowing needed it:
the `Never` bottom type, and `current_return_type` on `CheckContext` so
a `return` inside a nested `if`/block checks correctly against the
enclosing function's declared return type.

**Exit criteria met:** `tests/v3b_fixtures.rs`, 7 fixtures. Two of these
are regression fixtures for bugs found after the initial implementation,
not new-feature fixtures: `local_annotated_variable_is_registered.ts` and
`local_annotated_variable_without_initializer_is_registered.ts` prove
that an annotated variable declaration registers its type for later
reference in the same body; an earlier version of `statements.rs` only
registered the *unannotated* case, silently leaving annotated bindings
unresolvable by later code in the same function.

#### 4.3.3 V3c: Classes, done, including `this`

**Shipped**
- Fields and methods, structurally: a class's instance type is an
  `Object(ObjectType)`; no separate `Class` type variant exists. This
  means class-vs-class and class-vs-interface subtyping needed zero new
  rules, V2's width subtyping just applies. No nominal class identity
  (`private` fields aren't enforced); decided, not just assumed, since a
  structural representation makes nominal identity a genuinely separate
  feature to add later, not a gap in what's here.
- `extends`: the parent's fields/methods are resolved first (same lazy
  mechanism as interfaces, so `class A extends B` and `class B extends A`
  both work regardless of file order) and flattened in; the child's own
  members override same-named parent ones. A superclass expression more
  complex than a bare name (a mixin call, for instance) isn't resolved;
  inheritance is silently skipped in that specific case, though the
  class's own members are still checked.
- Constructors are registered as a `Type::Function` in `SymbolTypeMap`
  under the class's own binding, whose return type is the instance's
  `Object` type, so `new ClassName(...)` reuses the exact same
  arity/argument-checking function (`check_callable`) as a plain function
  call.
- `CallExpression` was extended to resolve member-expression callees
  (`obj.method(...)`), not just bare identifiers; without this, methods
  would be declarable but not callable through normal syntax.
- **`this` resolves to the enclosing class's instance type.**
  `CheckContext::current_class_instance` is set on entering a
  constructor or method body and restored on exit (see 1.9);
  `Expression::ThisExpression` reads it, falling back to the error
  sentinel outside any class body. This closes what was originally a
  known gap: an earlier version of this checker left `ThisExpression`
  unhandled entirely, degrading to the generic `Unsupported`-expression
  warning. `tests/fixtures/v3c/this_expression_known_gap.ts` was a
  known-gap fixture proving that honest degradation; it's since been
  updated (not deleted) to prove `this.x` resolves correctly now, and
  `this_expression_type_mismatch.ts` proves it's a real type check, not
  just a permissive pass-through.
- Getters/setters, static members, static blocks, index signatures, and
  accessor properties are skipped entirely (not part of the instance
  shape checked here).

**Exit criteria met:** `tests/v3c_fixtures.rs`, 7 fixtures. One is a
regression fixture (`untyped_constructor_param_skips_arity_check.ts`) for
a bug found after the initial implementation: `declare.rs` registers a
class's constructor arity as `0` both when there truly is no constructor
and when there is one but a parameter is untyped. Those aren't the same
case; a real zero-arg constructor should have its `new` calls
arity-checked, one that merely isn't fully understood shouldn't be
guessed at. `expressions.rs`'s `constructor_is_unresolvable` re-derives
the correct answer from the class's AST at the point a `new` call needs
to trust it, rather than encoding a third state into `declare.rs`'s
registration.

### 4.4 V4: closing the highest-leverage real-world gaps — planned, not started

**Goal:** cover enough of real-world TypeScript that a typical
application file produces genuine diagnostics instead of "not yet
checked" warnings. **Definition of done:** run the checker over a
handful of real open-source `.ts` files (not fixtures) and the
diagnostic output is dominated by real type errors/warnings, not by the
wildcard `push_unsupported`/"not yet checked" fallback.

This redefines what "V4" means compared to earlier drafts of this
document, which used "V4" for module resolution and incremental
checking. That work still matters but is explicitly **deferred past
V4** (now tracked as "V4+", section 4.5) — see the non-goals below for
why.

**Where V3c actually leaves off**, concretely, not hypothetically: every
expression below currently falls through to `infer_expression_type`'s
wildcard arm, and every statement below falls through to
`push_unsupported`. Arrow functions and function expressions not being
typed is the single highest-leverage gap: any file using callbacks
(`arr.map(x => ...)`, `const f = (x) => ...`) loses type tracking on
them immediately.

**Expressions with no type today:** arrow functions, function
expressions, `&&`/`||`/`??`, ternary (`?:`), assignment expressions,
template literals, `as`/`!` assertions, computed member access
(`obj[key]`), optional chaining (`?.`).

**Statements with no handling today:** `for`, `while`, `switch`,
`try`/`catch`, `throw`, `import`/`export`.

#### Tier 1 — foundational, build first

| # | Feature | Why it's first |
|---|---|---|
| 1 | Arrow functions / function expressions as values | Everything else (callbacks, higher-order functions, most real code) depends on these having a `FunctionType`. `declare.rs` currently only registers top-level `function` declarations. |
| 2 | Logical expressions (`&&`, `\|\|`, `??`) | Own type is a union of operand types, but the real payoff is narrowing: `x && x.y` and `x ?? fallback` are the two most common real narrowing idioms. `narrow.rs`'s branch-splitting logic already exists for `if`; this generalizes it to short-circuit operators. |
| 3 | Ternary (`ConditionalExpression`) | Mechanically an expression-level `if`: infer the test, narrow each branch, union the two results with `arena.alloc_union`. Cheap once (2) exists. |
| 4 | Type assertions (`as`, non-null `!`) | Common escape hatches. Without these, any file using them loses tracking of the asserted value for the rest of its scope. |

#### Tier 2 — high frequency, build after Tier 1

| # | Feature | Notes |
|---|---|---|
| 5 | Narrowing survives early return | `if (!x) return; x.foo` is arguably the most common real narrowing pattern, and is currently a documented known gap (1.4, `known_gap_narrowing_does_not_persist_past_if.ts`). Needs `NarrowState` to persist past a block that provably exits (`return`/`throw`/`continue`/`break` on every path), not just live inside the `if`. |
| 6 | Computed member access (`obj[key]`) + optional chaining (`?.`) | `?.` is everywhere once any nullable type exists in a file. |
| 7 | Enums | Structurally simple, a closed set of named literal members, compared to what comes after it. |
| 8 | `for`/`while`/`switch` bodies | Even walking the body without new type logic beats degrading to unsupported. `switch` on a discriminant is a common real narrowing shape and can reuse (2)'s branch-splitting machinery. |

**Build order:** 1 → 2 → 3 → 5 → 4 → 6 → 7 → 8. Doing narrowing
persistence (5) right after short-circuit operators (2) means
`NarrowState` only needs to be generalized once, instead of once for
`if`, again for `&&`, again for early-return.

#### Explicit non-goals for V4 (deferred to V4+, not cut)

- **Generics.** Likely the largest single gap versus real TypeScript by
  volume (`Array<T>`, `Promise<T>`, generic functions/classes), but a
  different order of magnitude of work: type parameter binding,
  call-site inference, and variance interacting with the existing
  subtyping relation. Mixing this into V4 risks landing neither Tier 1/2
  nor generics solidly. Treated as its own major version.
- **Modules** (`import`/`export` resolution across files). A module
  graph and `.d.ts` handling is an infrastructure project, separable
  from the type-checking logic itself. This is what the earlier "V4:
  Ecosystem Integration" draft of this document covered; it now lives
  under section 4.5, renumbered.
- **Mapped/conditional/template-literal types.** Advanced even in real
  `tsc`; lower real-world density per file than the Tier 1/2 list.

#### Architecture rules for V4 code

V4 should keep, and tighten, the data-oriented shape already established
in V1-V3c (section 1.10) rather than introducing new patterns as new
features land:

- No `Box<dyn Trait>` anywhere in the type representation; it defeats
  the flat-arena design and adds a vtable indirection to what should be
  a `u32` compare.
- No `Rc<RefCell<_>>` for anything reachable from `TypeArena` or
  `TypeNamespace`; single-threaded and single-owner already (section 6,
  "Resolved: threading model"), reach for ordinary `&mut` and the
  existing save/restore-on-`CheckContext` pattern (1.9) instead.
- New per-branch or per-scope state (upcoming loop-body state for Tier
  2, in particular) follows the `current_return_type`/
  `current_class_instance` pattern already in `CheckContext`: a field
  swapped out and restored via `Option::replace`, not a stack of
  allocations pushed and popped per node.
- Before allocating a new `Type` variant for a Tier 1/2 feature (e.g.
  arrow-function types, enum member types), check whether it can be
  represented with an *existing* variant plus a field, the way
  `FunctionType` should be extended rather than adding a parallel type
  for "function, but from an arrow expression." Fewer `Type` variants
  means fewer match arms everywhere that touches `Type`, and a smaller
  enum overall (an enum's size is driven by its largest variant).
- Single-threaded stays correct for the reason in section 6 below: it
  isn't a performance limitation being tolerated, it's what keeps
  `namespace.rs`'s `resolving` cycle-guard and `CheckContext`'s
  save/restore fields simple, ordinary sequential control flow instead
  of a much harder concurrent problem.

**Exit criteria:** every Tier 1/2 feature above gets its own
`tests/fixtures/v4/*.ts` fixture and matching assertion in
`tests/v4_fixtures.rs`, following the same one-fixture-per-capability
discipline as V1-V3c. Every known limitation gets an explicit
`known_gap_*.ts` fixture asserting the *current* (limited) behavior, so
a gap closing later shows up as a test needing an update, not a silent
behavior change (see 4.3.3's `this_expression_known_gap.ts` for exactly
that pattern in practice). `docs/ROADMAP.md`'s gaps list and the fixture
suite should never drift apart.

### 4.5 V4+: Ecosystem integration, not started

(Renumbered from an earlier draft's "V4"; module resolution and
generics are now understood to be their own major versions after the
V4 feature set in 4.4, not part of it — see 4.4's non-goals.)

**Planned**
- `FileResolver` trait (native: reads real FS; wasm: host supplies an
  in-memory map or callback); the seam that keeps core platform-agnostic
  per section 1.7.
- Minimal `.d.ts` parsing: ambient `declare` statements, enough for a
  hand-authored stdlib subset (explicitly not full `lib.d.ts`).
- Node-style module resolution: relative imports + `package.json`
  `main`/`types` fields. Workspaces/`exports` conditions are a V5+
  candidate.
- Editor-facing API: a stable `check_source`/`check_project` surface
  shaped so an LSP server *could* be built on top later; this version
  does not ship an LSP server itself.
- Incremental re-check: cache pass-1 declarations per unchanged file,
  invalidate on content hash change. No cross-file incremental
  type-cache invalidation correctness guarantees yet; documented as
  best-effort, not silently assumed correct.
- Generics: type parameter binding, call-site inference, constraint
  checking.

**Exit criteria:** fixtures covering a two-file import/export round trip,
one ambient-declared global used and correctly type-checked, one
incremental re-check test asserting an unchanged file's declarations
aren't re-computed, and one generic function instantiation.

---

## 5. What "done" means at each stage (anti-scope-creep guardrail)

Every version's exit criteria is a fixed, checkable fixture list, not a
vibe. A version is not "done" because the code compiles; it's done when
its fixture set passes and its explicitly-deferred items are written down
as *named* gaps, in this document and in `docs/ROADMAP.md`, for the next
version, not discovered later as surprises. Every "not implemented" claim
above is backed by a fixture proving the honest degradation, not just a
sentence asserting it.

---

## 6. Open questions

1. ~~Nominal vs. structural class identity~~ — **Resolved in V3c:**
   structural. A class instance is an `Object` type; no nominal identity
   is tracked or enforced.
2. Error recovery strategy: does the checker keep going after a parse
   error (best-effort partial check), or stop at first parse failure?
   Currently: stops (`bridge::parse` returns `Err` on any parser
   diagnostic at `Error` severity). Revisit if partial-file checking
   turns out to matter for editor integration (V4+).
3. `FileResolver` trait shape (V4+): sync vs. async (wasm hosts may need
   async I/O for fetch-based resolution). Not yet decided.
4. Versioning/release cadence for the npm package vs. the crate; likely
   lockstep initially, revisit if wasm-specific patches diverge from
   native.
5. ~~Narrowing's lack of join/persistence semantics~~ — **Resolved:**
   scheduled as V4 Tier 2, item 5 (section 4.4), not left to drift
   further out.

### Resolved: threading model

Core (arena, subtyping, single-file check) is **single-threaded through
V4**. Reasons:

- Correctness comes first. The hard part of this project is getting
  subtyping/narrowing/inference right, and threading introduces
  shared-mutation and ordering bugs that actively fight section 1.5's
  "no silent wrong answers" principle before the core logic is proven
  against fixtures.
- WASM constraint. `wasm32-unknown-unknown` has no real threads without
  `wasm-bindgen-rayon` + `SharedArrayBuffer` + COOP/COEP headers, which a
  generic npm/browser consumer can't be assumed to provide.
- The real early performance win is architectural (arena allocation, no
  `Box<Type>` tree-walking, Oxc's fast parser, and the key-density-aware
  data structure choices in 1.10), not parallelism. Optimizing parallel
  execution before a real bottleneck is identified (see
  `docs/VERIFICATION.md`'s benchmark discipline) would be premature.
- If parallelism is ever wanted, it belongs *between* files (checking N
  independent files on N threads, each with its own `TypeArena`/
  `SymbolTypeMap`/`TypeNamespace`), not *inside* one file's check. That
  boundary is already natural: a `TypeChecker` run is a self-contained
  `CheckContext` with no cross-file state. `namespace.rs`'s
  `resolving`-flag cycle detection and `CheckContext`'s save/restore
  fields (1.9) both rely on ordinary sequential control flow; making
  either of those thread-safe for no current benefit would turn a
  simple problem into a much harder one.

**Where parallelism is introduced later (V4+):** scoped strictly to
*independent per-file* checking once multi-file projects exist, each file
gets its own arena, merged after checking. Native-only, behind a feature
flag (e.g. `rayon`), never required by core logic. The WASM build runs
the same loop sequentially. Threading is never applied within a single
file's arena/context mutation, at any version.

---

## 7. Explicit non-goals (all versions)

- Full `lib.d.ts`/DOM type parity: always a curated subset.
- tsc's exact error message wording: equivalent *diagnosis*, not
  byte-identical messages.
- Build/emit: this project never emits JavaScript, it is check-only.
- Decorators: not before V5 at the earliest, and not silently
  unsupported-and-ignored; produces `Unsupported` per section 1.5.
