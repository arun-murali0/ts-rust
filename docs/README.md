# ts-rust Documentation

This directory is the maintained technical guide for ts-rust.

The repository uses **semantic milestone names** instead of opaque numeric development labels. A contributor should be able to understand what a test suite or document covers from its name alone.

## Current semantic baseline

```text
Foundation
    ↓
Structural Types
    ↓
Literal Widening
    ↓
Control-Flow Narrowing
    ↓
Classes and Inheritance
    ↓
Expression and Program Checking
    ↓
Parameters and Destructuring
    ↓
Generics Tier 1  ← current baseline
```

These names describe capabilities. They are not release versions and should not be interpreted as a versioning scheme.

## Milestones

| Milestone | Purpose | Tests | Documentation |
| --- | --- | --- | --- |
| Foundation | Establish the end-to-end checker, primitive types, basic functions, diagnostics, and honest unsupported-feature handling. | `tests/foundation.rs` | [foundation](foundation.md) |
| Structural Types | Add objects, arrays, unions, aliases, interfaces, declaration resolution, structural subtyping, and function arity. | `tests/structural_types.rs` | [structural-types](structural-types.md) |
| Literal Widening | Preserve literal information while modeling widening behavior. | `tests/literal_widening.rs` | [literal-widening](literal-widening.md) |
| Control-Flow Narrowing | Refine types across branches and flow paths. | `tests/control_flow_narrowing.rs` | [control-flow-narrowing](control-flow-narrowing.md) |
| Classes and Inheritance | Model class members, constructors, inheritance, static state, and `this`. | `tests/classes_inheritance.rs` | [classes-inheritance](classes-inheritance.md) |
| Expression and Program Checking | Expand expression inference, calls, members, logical operations, loops, switches, assertions, and program-level checking. | `tests/expression_program_checking.rs` | [expression-program-checking](expression-program-checking.md) |
| Parameters and Destructuring | Model parameter metadata and binding/destructuring semantics. | `tests/parameters_destructuring.rs` | [parameters-destructuring](parameters-destructuring.md) |
| Generics Tier 1 | Establish reusable generic function inference, substitution, and type-parameter scoping. | `tests/generics_tier1.rs` | [generics-tier1](generics-tier1.md) |

## Architecture

Read [architecture.md](architecture.md) for the current ownership model.

The short version is:

```text
Oxc
  ↓
bridge
  ↓
semantic layer
  ↓
TypeArena / TypeNamespace / Subtyping / Generics / Narrowing
  ↓
diagnostics + future semantic facts
  ↓
tooling / typed IR
```

Oxc remains the front-end foundation. ts-rust implements the semantic behavior that sits on top of it.

## Roadmap

[roadmap.md](roadmap.md) describes the intended progression after Generics Tier 1:

```text
stronger generic constraints/inference
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
semantic facts
        ↓
typed IR
```

This is a dependency-oriented plan, not a promise that every TypeScript feature will arrive in that exact order.

## Testing and performance

Read [testing.md](testing.md) for:

- semantic fixture conventions;
- regression policy;
- formatting/check/lint/test commands;
- Criterion and IAI benchmarks;
- heap profiling;
- compatibility harness expectations.

## Design boundaries

Read [purpose-and-overdesign.md](purpose-and-overdesign.md) for:

- why ts-rust builds on Oxc;
- what ts-rust owns;
- what it deliberately does not rebuild;
- how larger projects such as tsz are used as references;
- why premature abstraction and premature multithreading are avoided.
