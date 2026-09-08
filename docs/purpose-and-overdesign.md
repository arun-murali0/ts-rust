# Purpose, Design Boundaries, and Future Plan

This document is the forward-looking companion to the historical v1-v5 architecture case studies.

The stage documents answer:

> What did we actually build, and why was it designed that way?

This document answers:

> What is complete, what remains immediately necessary, and what belongs to the longer-term architecture?

The plan is intentionally grouped into three categories so future work does not get mixed with completed behavior.

---

## Category 1: Completed Foundation

The current implementation already provides the semantic foundation needed to continue building the checker.

### v1: Initial type-checking foundation

- primitive type representation
- basic assignability
- function argument and return checking
- binary expression checking
- structured diagnostics
- explicit unsupported-feature reporting

See `v1-foundation.md`.

### v2: Structural type system

- object types
- structural subtyping
- required and optional properties
- arrays
- unions
- aliases
- interfaces
- forward references
- circular-reference handling
- function arity

See `v2-structural-types.md`.

### v3: Semantic refinement

#### v3a
- literal types
- literal unions
- `const` preservation
- `let` widening

#### v3b
- symbol-aware flow state
- `NarrowState`
- `typeof` narrowing
- nullish narrowing
- truthiness narrowing
- branch-local state

#### v3c
- classes
- inherited fields
- methods
- constructors
- class `this` context
- conservative class recovery

See the three v3 documents.

### v4: Expression and program checking

- functions and arrows
- calls
- member access
- indexed access
- logical expressions
- conditional expressions
- optional chaining
- non-null assertions
- assertions
- loops
- switch
- early-return flow
- enums
- static class members

See `v4-expression-and-program-checking.md`.

### v5: Parameter and binding semantics

- optional parameters
- rest parameters
- rest bindings
- object destructuring
- array destructuring
- renamed bindings
- destructuring defaults
- destructured function parameters

See `v5-parameters-and-destructuring.md`.

### Current engineering foundation

The repository also has:

- Oxc parser/AST/semantic integration
- arena-backed `TypeId` representation
- centralized subtyping
- namespace/declaration resolution
- symbol/type mapping
- custom TypeScript-aware narrowing
- fixture-based regression testing
- benchmark and comparison harness
- CI quality checks
- security audit workflow
- dependency update automation

### v6 status

Generics and inference are under active development.

They are deliberately **not marked complete** until the generic semantic model and regression suite are finished.

---

# Category 2: Immediate Build Plan

These are the capabilities that should be completed before spending serious effort on large-scale performance or ecosystem breadth.

## 1. Complete generics and inference

Finish the current v6 work:

- type-parameter identity
- generic function signatures
- inference from arguments
- multiple type parameters
- inference through arrays and objects
- explicit type arguments
- substitution
- constraints
- defaults
- contextual typing
- recursive inference protection
- generic return-type inference

The goal is a reusable generic semantic model, not special cases inside individual expression handlers.

---

## 2. Module and project resolution

Introduce a project-level module layer.

Preferred direction:

```text
source files
    ↓
module resolver
    ↓
ModuleGraph
    ↓
file semantic state
    ↓
ts-rust checker
```

`oxc_resolver` is the natural infrastructure candidate because module resolution is expensive to reproduce and includes TypeScript-relevant resolution behavior.

ts-rust should own the resulting project/module semantics rather than duplicating resolver internals.

---

## 3. Declaration files

Make `.d.ts` a first-class input.

Prioritize:

- interfaces
- type aliases
- function declarations
- overloads
- ambient declarations
- module declarations
- namespaces where required
- standard library declarations

This is one of the largest steps toward real application compatibility.

---

## 4. `keyof` and indexed access

Build these as reusable type operations:

```text
keyof T
T[K]
T["property"]
T[number]
```

They should compose with the existing object, array, union, and `TypeId` model.

---

## 5. Stronger control-flow semantics

Continue evolving the existing `NarrowState` architecture.

Add, in dependency order:

- logical negation
- richer `&&` / `||` flow
- assignment invalidation
- switch discrimination
- discriminated unions
- property narrowing
- alias narrowing
- loop convergence
- closure/captured-variable behavior

Do not replace the existing TypeScript-aware narrowing system merely to introduce a generic CFG abstraction.

---

## 6. Function overloads

Introduce overload sets:

```text
function declaration
       ↓
candidate signatures
       ↓
argument compatibility
       ↓
selected signature
       ↓
return type
```

This should build on the existing `FunctionType` and call-checking architecture.

---

## 7. Compiler options and strictness

Introduce an explicit compiler-options model instead of scattering configuration checks throughout the checker.

Prioritize:

- strict null checking
- implicit `any`
- module options
- declaration behavior
- JSX options

---

## 8. Mapped and conditional types

After generics, `keyof`, and indexed access are stable, implement:

```text
{ [K in keyof T]: ... }

T extends U ? X : Y
```

Then add `infer` as a general conditional-type mechanism.

These should be semantic primitives, not utility-type-specific hacks.

---

## 9. Common utility types

Once the underlying primitives exist, support common utilities such as:

```text
Partial
Required
Readonly
Pick
Omit
Record
Exclude
Extract
NonNullable
ReturnType
Parameters
```

The utilities should emerge from mapped, conditional, indexed-access, and inference machinery.

---

# Category 3: Future Architecture and Scale

These are important, but they should follow semantic correctness. Performance engineering a checker that still lacks the type semantics applications need is a particularly elegant way to optimize the wrong thing.

## 1. Standard library and ecosystem types

Load useful library declarations rather than hard-coding APIs:

- Array
- Promise
- Object
- String
- Number
- Boolean
- common ES APIs

This becomes the basis for realistic application checking.

---

## 2. JSX and modern application patterns

Add:

- JSX elements
- intrinsic elements
- component call signatures
- props
- children
- JSX namespace declarations
- `.tsx` checking

Then support common async patterns:

- `Promise<T>`
- `async`
- `await`
- thenable compatibility

These should build on generics and declaration files.

---

## 3. Incremental project checking

Move from file-at-a-time checking toward a project engine:

```text
project
  ↓
ModuleGraph
  ↓
dependency graph
  ↓
cached semantic state
  ↓
changed-file detection
  ↓
affected-file checking
```

The objective is to avoid redoing work for unchanged files.

A future cache should distinguish at least:

- parsed AST state
- semantic symbol state
- resolved declaration state
- type-check results

---

## 4. Multithreading and parallel checking

Parallelism should be introduced only after semantic and project boundaries are stable.

Good candidates include:

```text
independent file parsing
independent semantic analysis
independent project regions
fixture/harness execution
```

The design should avoid putting shared mutable semantic state behind locks unnecessarily.

The arena/`TypeId` architecture is useful here because semantic references are compact handles rather than large owned graphs.

Potential future architecture:

```text
             Project
                │
          ModuleGraph
                │
      ┌─────────┼─────────┐
      ▼         ▼         ▼
    File A    File B    File C
      │         │         │
    worker    worker    worker
      └─────────┼─────────┘
                ▼
       shared/cached results
```

Parallelism is an implementation strategy, not a semantic layer.

---

## 5. Performance engineering

Measure before optimizing.

Track:

- parse time
- semantic-analysis time
- module-resolution time
- type-resolution time
- checking time
- allocations
- arena growth
- memory usage
- cache hit rate

Optimize based on real measurements from representative repositories.

---

## 6. Incremental and parallel architecture together

The eventual project engine can combine both:

```text
                  Project
                     │
                ModuleGraph
                     │
             changed-file analysis
                     │
          ┌──────────┼──────────┐
          ▼          ▼          ▼
       worker      worker      worker
          │          │          │
          └──────────┼──────────┘
                     ▼
              semantic cache
                     │
                     ▼
             affected checking
```

The important constraint is determinism.

Parallel execution must not change:

- type results
- diagnostic meaning
- diagnostic ordering policy
- cache correctness

---

## 7. Compatibility and real-world coverage

The existing harness should continue comparing:

```text
tsc
tsgo
ts-rust
```

using:

- exit codes
- normalized diagnostics
- timing
- repeated runs
- output hashes

The harness is an experimental signal, not a correctness oracle.

Real-world coverage should eventually be measured against representative repositories rather than by counting implemented TypeScript syntax features.

---

# Dependency order

The broad dependency chain is:

```text
Complete generics/inference
        ↓
Module/project resolution
        ↓
.d.ts infrastructure
        ↓
keyof + indexed access
        ↓
Overloads
        ↓
Mapped types
        ↓
Conditional types + infer
        ↓
Compiler options / strictness
        ↓
Utility types
        ↓
Advanced control-flow semantics
        ↓
Standard library declarations
        ↓
JSX + async ecosystem
        ↓
Incremental project engine
        ↓
Multithreading
        ↓
Performance optimization
        ↓
Large real-world compatibility
```

Some items can proceed in parallel, but the semantic dependencies should remain explicit.

# Design principles for future work

1. **Use Oxc where it saves large amounts of implementation effort.**
   Parser, AST, semantic analysis, and resolution infrastructure are good examples.

2. **Keep TypeScript type semantics in ts-rust.**
   The value of this project is the semantic checker, not another wrapper around an existing front end.

3. **Prefer composable semantic primitives.**
   `TypeId`, arena storage, subtyping, unions, symbol mapping, narrowing, substitution, and contextual typing should compose.

4. **Keep flow semantics in `NarrowState`.**
   A generic control-flow graph does not automatically provide TypeScript narrowing semantics.

5. **Add regression fixtures with semantic changes.**
   Tests are part of the language contract.

6. **Prefer explicit unsupported behavior to unsound guesses.**

7. **Do not suppress compiler/linter warnings to hide design problems.**

8. **Correctness before performance.**

9. **Measure real repositories.**

10. **Keep historical implementation docs separate from future plans.**
