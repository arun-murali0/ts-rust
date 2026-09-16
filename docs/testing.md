# Testing and Performance

The test suite is organized by semantic capability. A test name should tell a contributor what behavior is protected without requiring knowledge of an internal development version.

## Test organization

Each semantic milestone has:

1. a Rust integration test in `tests/`;
2. focused TypeScript fixtures in `tests/fixtures/`;
3. a matching document in `docs/`.

For example:

```text
Generics Tier 1
├── tests/generics_tier1.rs
├── tests/fixtures/generics-tier1/
└── docs/generics-tier1.md
```

The Rust test defines the executable contract. The fixture demonstrates the TypeScript behavior. The documentation explains the semantic and architectural reason.

## Naming

Use semantic capability names:

```text
foundation
structural_types
literal_widening
control_flow_narrowing
classes_inheritance
expression_program_checking
parameters_destructuring
generics_tier1
```

Do not introduce numeric release-style labels for semantic test suites. Cargo package releases may still use normal semantic versioning; that is separate from development milestones.

## What a good fixture tests

A fixture should make one semantic claim whenever practical.

Prefer:

```text
generic_constraint_rejects_invalid_argument.ts
```

over a large fixture that combines unrelated generic, class, and control-flow behavior.

When a bug spans multiple semantic systems, a focused regression should still identify the interaction in its filename or test name.

## Required validation

Before merging a semantic change:

```bash
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The repository shortcut is:

```bash
./scripts/ci.sh
```

## Performance validation

The repository contains:

- Criterion benchmarks in `benches/checker_benchmark.rs`;
- IAI callgrind benchmarks in `benches/checker_iai.rs`;
- shared complex fixtures in `benches/support/complex_fixtures.rs`;
- heap profiling in `examples/dhat_heap.rs`.

The shared benchmark generators intentionally exercise:

- unions and discriminant narrowing;
- nested object resolution;
- class inheritance;
- generic inference/substitution;
- destructuring;
- mixed realistic workloads.

A semantic change that passes tests but creates a significant benchmark regression should be investigated before merging.

## Compatibility harness

`scripts/harness.sh` compares ts-rust with TypeScript implementations such as `tsc` and `tsgo`.

It is a compatibility investigation tool, not a semantic-equivalence proof. A matching diagnostic or exit code does not establish that two implementations have identical type semantics.

The harness records reproducibility information such as compiler versions, source revision, timings, normalized diagnostics, and output hashes.

## Adding a new semantic capability

Use this sequence:

```text
1. Define the semantic rule.
2. Add a focused fixture.
3. Add the integration assertion.
4. Implement the smallest reusable semantic primitive.
5. Keep Oxc-specific traversal in bridge/.
6. Run the complete regression suite.
7. Run representative benchmarks.
8. Update the matching documentation.
```

This keeps tests, implementation, and documentation synchronized.
