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
Generics (functions, interfaces, aliases, classes, recursion, builtins) ← current baseline, substantially complete
    ↓
Narrowing completion, sessions and file identity (joins, discriminated unions, `in`/`instanceof`, `CheckSession`)
```

These names describe capabilities. They are not release versions and should not be interpreted as a versioning scheme.

For the full, item-by-item picture of what's checked and what isn't across the entire TypeScript type system (not just this milestone list), see [`checklist.md`](../checklist.md) at the repository root — it's the actively maintained source of truth for current coverage, kept more granular than this milestone list.

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
| Generics | Type parameters on functions, interfaces, aliases, and classes: inference, substitution, constraints, defaults, explicit type arguments, recursive generic shapes, and builtin generics (`Array<T>`, opaque `Promise<T>`). One consolidated document, not split by tier — see it for why. | `tests/generics_tier1.rs`, `tests/generics_tier2.rs`, `tests/generic_classes.rs`, `tests/recursive_types.rs`, `tests/builtin_generics.rs`, `tests/interface_methods.rs` | [generics](generics-tier.md) |

Later work extends these milestones rather than adding a tier: `tests/narrowing_advanced.rs` and `tests/narrowing_complete.rs` (switch, `&&`/`||`, loops, joins, `in`, `instanceof`), `tests/generics_bounds.rs` (literal-preserving inference, explicit type arguments), `tests/declaration_collisions.rs`, `tests/union_member_access.rs` and `tests/check_session.rs` (sessions, metrics, file identity).

A handful of smaller, cross-cutting suites support these milestones rather than being their own tier: `tests/call_arity.rs`, `tests/void_and_never.rs`, `tests/unreachable_code.rs`, `tests/named_types_in_messages.rs`, `tests/diagnostic_codes.rs`, `tests/misc_fixes.rs`, and `tests/subtype_cache_stress.rs` (a performance/correctness regression suite, not a feature milestone).

## Why it is built this way

[case-study.md](case-study.md) collects the design decisions in one place: the problem each solves, what was rejected, and how it was checked. Start there when a choice looks odd.

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

[roadmap.md](roadmap.md) covers the project's longer-term direction (semantic facts, typed IR, project-scale checking, performance philosophy) that doesn't change week to week.

For the actual current build order — what's next and why, in priority order — see [`BUILD.md`](../BUILD.md) at the repository root, which is derived from and stays in sync with `checklist.md`. Both are updated more frequently than this documentation and are the ones to check for "what's actually being worked on right now."

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
