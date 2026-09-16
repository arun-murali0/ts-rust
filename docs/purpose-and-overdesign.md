# Purpose, Design Boundaries, and Future Plan

This document is the forward-looking companion to the historical semantic milestone documents.

The project has two deliberate architectural boundaries:

1. **Oxc is the front-end foundation.** ts-rust consumes Oxc parsing, AST, spans, and semantic infrastructure where those components fit.
2. **ts-rust owns TypeScript semantic behavior.** Type representation, assignability, narrowing, generic inference, and future typed IR belong to this project.

The goal is not to recreate another complete TypeScript front end. The goal is to build a clean semantic layer that can grow into tooling and, later, compilation.

## Completed foundation

The historical milestones establish:

- primitive types and basic assignability;
- structural object, array, union, alias, and interface types;
- declaration/type resolution and circular-reference protection;
- literal types and widening;
- branch-local control-flow narrowing;
- classes, inheritance, methods, constructors, and `this`;
- expression and statement checking;
- parameters and destructuring;
- first-tier generic functions and argument inference.

The executable contracts live in the named fixture suites under `tests/fixtures/`.

## Immediate engineering plan

### 1. Strengthen Generics Tier 1

Turn the current generic-function implementation into a reusable semantic subsystem while preserving the existing Tier 1 regression contract.

Target capabilities:

- type-parameter identity and scope;
- generic function signatures;
- inference from arguments;
- multiple type parameters;
- inference through arrays and object properties;
- explicit type arguments;
- substitution;
- constraint checking;
- type-parameter defaults where supported;
- instantiated return types;
- recursive inference protection;
- generic methods and classes when their existing declaration model can support them cleanly.

The important design rule is that generic inference should be a semantic operation, not a collection of special cases inside call-expression checking.

### 2. Grow the semantic query boundary

A small read-only query boundary already exists. Grow it only when direct calls into `TypeArena`, `TypeNamespace`, subtyping, and generic helpers begin to make feature modules difficult to change.

The intended direction is:

```text
bridge feature
      ↓
semantic query
      ↓
TypeArena / Namespace / Subtyping / Generics / Flow
```

Example semantic operations include:

- `is_assignable(source, target)`;
- `property_type(receiver, key)`;
- `call_signature(type)`;
- `construct_signature(type)`;
- `resolve_type(...)`;
- `infer_generic_call(...)`;
- `instantiate(...)`.

This should be a small Rust API, not a giant abstraction hierarchy.

### 3. Preserve a clean semantic core

Keep these responsibilities separate:

| Responsibility | Owner |
| --- | --- |
| AST traversal and Oxc details | `bridge/` |
| Type storage and `TypeId` allocation | `TypeArena` |
| Declaration/name/type resolution | `TypeNamespace` |
| Symbol-to-type association | `SymbolTypeMap` |
| Assignability and subtype relations | `subtyping.rs` |
| Control-flow refinement | `NarrowState` / future flow module |
| Generic inference and substitution | semantic generic subsystem |
| Diagnostics | diagnostics layer |

Do not turn `TypeArena` or `CheckContext` into a universal service container.

## Type-system expansion order

The preferred semantic dependency order is:

```text
Generics Tier 1 completion
        ↓
stronger inference and constraints
        ↓
keyof + indexed access
        ↓
overloads and contextual typing
        ↓
mapped types
        ↓
conditional types + infer
        ↓
utility types
        ↓
stronger control-flow semantics
```

This order is intentional. Mapped and conditional types become much easier to model once generic substitution, indexed access, and type relationships are already reliable.

## Semantic facts and tooling

After the core produces stable types, expose reusable semantic facts instead of coupling editor features to checking code.

```text
checker
   ↓
semantic result
   ├── diagnostics
   ├── inferred types
   ├── symbol information
   └── semantic facts
             ↓
       hover / CodeLens / inlay hints / LSP
```

Editor presentation should remain outside the checker.

## Typed IR and future compilation

A typed intermediate representation should be introduced only after the semantic model is strong enough to describe checked programs consistently.

```text
Oxc AST
   ↓
checked semantic model
   ↓
typed IR
   ↓
HIR / SIR as justified by backend needs
   ↓
native / WASM / other backends
```

There is no value in building a sophisticated IR that merely preserves an immature type system.

## Project-scale architecture

Future project checking should separate immutable project information from per-check mutable state:

```text
                    Project
                       │
                 ModuleGraph
                       │
          immutable indexed information
                       │
       ┌───────────────┼───────────────┐
       ▼               ▼               ▼
  CheckContext     CheckContext    CheckContext
     file A           file B          file C
```

Worker-local state should include diagnostics, narrowing state, current function/class state, and temporary inference state. Shared state should be immutable or explicitly snapshot-based.

Parallelism belongs at the file/program/session boundary. Do not put locks around the current type-system hot path merely to advertise future multithreading.

## Performance plan

Keep the existing Criterion, IAI, and heap-profiling infrastructure as regression signals.

Measure separately:

- parsing;
- semantic indexing;
- module resolution;
- type resolution;
- checking;
- allocations;
- arena growth;
- memory peak;
- project-cache effectiveness.

Optimize only after representative workloads identify a real bottleneck.

## Reference projects

Other Rust TypeScript implementations are useful references for difficult semantic and architectural problems. In particular, tsz is valuable for observing how a much larger checker separates checking, flow, inference, solver operations, and tooling boundaries.

It is a **reference implementation, not our architecture specification**.

The rule is simple: adopt an idea because it solves a problem in ts-rust, not because another compiler happens to contain a similarly named module.

## Non-goals

For the current stage, do not:

- replace Oxc's parser with a custom parser;
- recreate Oxc's semantic front end without a concrete need;
- introduce inheritance-heavy traits solely for SOLID compliance;
- split files merely to make line counts look small;
- add locks before shared ownership is actually required;
- build a complete SIR/backend before the type system is mature;
- chase every TypeScript feature without preserving a coherent semantic model.
