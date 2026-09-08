# ts-rust Architecture Case Study

These documents describe the **actual development workflow of ts-rust**, reconstructed from the implementation and the regression fixtures in `tests/fixtures`.

They are intentionally not generic TypeScript compiler overviews. Each stage answers:

- What problem were we solving?
- Which tests define the milestone?
- What did we implement?
- Which architecture was chosen?
- Why was it chosen?
- Which design patterns emerged?
- How does the code connect to earlier stages?
- What did the milestone unlock?

## Historical stages

```text
v1  Foundation
 ↓
v2  Structural types, declarations, resolution
 ↓
v3a Literal types and widening
 ↓
v3b Control-flow narrowing
 ↓
v3c Classes and inheritance
 ↓
v4  Expression and program checking
 ↓
v5  Parameters and destructuring
 ↓
v6  Generics and inference (WIP)
```

## Documents

1. [v1: Initial Type-Checking Foundation](v1-foundation.md)
2. [v2: Structural Types, Declarations, and Resolution](v2-structural-types.md)
3. [v3a: Literal Types and Widening](v3a-literals-and-widening.md)
4. [v3b: Control-Flow Narrowing](v3b-control-flow-narrowing.md)
5. [v3c: Classes, Inheritance, and `this`](v3c-classes-and-inheritance.md)
6. [v4: Expression and Program Checking](v4-expression-and-program-checking.md)
7. [v5: Parameter Semantics and Destructuring](v5-parameters-and-destructuring.md)

## Source of truth for the history

The stage folders under `tests/fixtures/` are the historical specification:

```text
tests/fixtures/v1/
tests/fixtures/v2/
tests/fixtures/v3a/
tests/fixtures/v3b/
tests/fixtures/v3c/
tests/fixtures/v4/
tests/fixtures/v5/
tests/fixtures/v6/
```

A fixture normally represents one semantic claim. The stage docs explain the architecture behind those claims.

## Architectural progression

```text
Oxc parser + semantic information
          ↓
      bridge layer
          ↓
 Type / TypeId / TypeArena
          ↓
 namespace + symbol/type map
          ↓
   centralized subtyping
          ↓
 literal semantics
          ↓
   NarrowState overlays
          ↓
 expression + statement checking
          ↓
 reusable binding semantics
```

The project does not completely rely on Oxc for type checking. Oxc provides the expensive front-end capabilities. ts-rust owns the TypeScript semantic model and checker behavior.

## v6 policy

v6 contains the current generics/inference work. Its architecture case study should be written after generics are considered complete, using the same historical format rather than a generic feature overview.

## Forward roadmap

The roadmap is intentionally separate from the historical stages. See [`purpose-and-overdesign.md`](purpose-and-overdesign.md) for planned work such as generics completion, modules, declaration files, `keyof`, indexed access, mapped types, conditional types, overloads, stricter control flow, JSX, and project-scale performance.
