# Roadmap

This roadmap describes the semantic capabilities the project is building toward. The names are capability-based rather than numbered releases, so the repository remains understandable without knowing its development history.

## Current baseline

**Generics Tier 1** is the current semantic frontier.

The completed foundation includes:

- Oxc-backed parsing and semantic information;
- primitive and function types;
- structural objects, arrays, unions, aliases, and interfaces;
- declaration resolution and circular-reference protection;
- literal widening;
- control-flow narrowing;
- classes, inheritance, constructors, methods, and `this`;
- expression and statement checking;
- parameters and destructuring;
- first-tier generic function inference and substitution.

## Next: Generic Semantic Core

The next work should strengthen the existing generic implementation without another repository-wide rewrite.

Targets:

1. type-parameter constraints;
2. explicit type arguments;
3. reusable substitution and instantiation;
4. inference from nested arrays and object properties;
5. multiple inference candidates;
6. generic methods and classes;
7. constraint validation;
8. recursive inference protection;
9. regression coverage for every semantic rule.

The generic algorithms belong in the semantic layer. AST traversal belongs in `bridge/`.

## After generic foundations

The preferred order is:

```text
Generics Tier 1
    ↓
constraints + stronger inference
    ↓
keyof + indexed access
    ↓
overloads + contextual typing
    ↓
mapped types
    ↓
conditional types + infer
    ↓
utility types
    ↓
stronger control-flow semantics
```

The order is based on semantic dependencies, not on TypeScript syntax order.

## Semantic facts

Once inferred types and symbol information are stable, the checker should expose reusable semantic facts:

```text
checker
  ↓
semantic result
  ├── diagnostics
  ├── symbol information
  ├── inferred types
  └── semantic facts
```

Those facts can later power hover, CodeLens, inlay hints, and LSP features without coupling editor presentation to the checker.

## Typed IR

A typed intermediate representation should come after the semantic model is mature:

```text
Oxc AST
  ↓
checked semantic model
  ↓
typed IR
  ↓
HIR / SIR where justified
  ↓
future backends
```

The IR should represent information the semantic core can actually guarantee. It should not become a second, parallel type system.

## Project scale and parallelism

Project-level work should eventually separate immutable indexed information from worker-local checking state:

```text
Project
  ↓
immutable project index
  ├── file A → CheckContext
  ├── file B → CheckContext
  └── file C → CheckContext
```

Parallelism belongs at the file/program/session boundary. The current checker should not gain locks merely in anticipation of this future.

## Performance

Every major semantic change should be checked against:

- Criterion benchmarks;
- IAI callgrind benchmarks;
- heap profiling where relevant;
- representative fixtures;
- the full regression suite.

Correctness comes first. Once a semantic operation is stable, profiling should identify whether caching, canonicalization, allocation reduction, or algorithmic changes are justified.

## Reference projects

`tsz` is useful as a reference for semantic algorithms, scaling problems, solver boundaries, and architectural lessons.

It is not the architecture specification for ts-rust.

Oxc remains the front-end foundation. ts-rust owns the semantic layer built on top of Oxc.
