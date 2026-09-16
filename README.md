# ts-rust

> A Rust-based experimental TypeScript type checker built on top of Oxc.

## ⚠️ Experimental, not production-ready

**ts-rust is experimental software. Do not use it as a production TypeScript compiler**

The project is actively evolving. TypeScript compatibility is incomplete, generic inference is still under development, and the compatibility harness is an experimental comparison tool rather than a correctness proof.

## What is ts-rust?

ts-rust explores how much of the TypeScript type system can be implemented in Rust while reusing the mature front-end infrastructure provided by [Oxc](https://oxc.rs/).

We deliberately do **not** try to rebuild everything.

Oxc handles the expensive front-end pieces:

- TypeScript parsing
- AST representation
- semantic symbols/scopes/references
- source information
- module resolution where adopted

ts-rust owns the part that is the reason this project exists:

- semantic type representation
- `TypeId` and arena-backed type storage
- structural subtyping and assignability
- declaration/type resolution
- expression type inference
- TypeScript-specific control-flow narrowing
- parameter and binding semantics
- diagnostics
- generic inference and substitution, with advanced type-level semantics planned

The design goal is a clean boundary:

```text
                    TypeScript source
                           │
                           ▼
                    ┌─────────────┐
                    │     Oxc     │
                    │ parser / AST│
                    │  semantic   │
                    └──────┬──────┘
                           │
                           ▼
                    ┌─────────────┐
                    │  ts-rust    │
                    │   bridge    │
                    └──────┬──────┘
                           │
                           ▼
              ┌────────────────────────┐
              │    Semantic Type Core  │
              │ Type / TypeId / Arena  │
              │ namespace / subtyping │
              └───────────┬────────────┘
                          │
                    semantic queries
                          │
             ┌────────────┼────────────┐
             ▼            ▼            ▼
        inference     narrowing    diagnostics
             │            │
             └────────────┘
                    │
                    ▼
              checker result
```

## Why build it?

The project exists as an engineering and research exercise around a difficult question:

> Can a fast Rust implementation build a useful, understandable TypeScript semantic checker while consuming mature compiler front-end infrastructure instead of recreating it?

The project is also an architecture study. The implementation is intentionally built in small, testable milestones so each semantic decision can be inspected, measured, and changed without turning the checker into a collection of AST-specific special cases.

## Current implementation

The project records progress using **semantic milestone names**, not opaque version labels. The current baseline is **Generics Tier 1**.

```text
Foundation
Structural Types
Literal Widening
Control-Flow Narrowing
Classes and Inheritance
Expression and Program Checking
Parameters and Destructuring
Generics Tier 1                 <- current baseline
```

These names are deliberately descriptive so a contributor can infer the purpose of a test suite or document without first learning an arbitrary version history.

## Repository structure

```text
src/
├── arena.rs
├── types.rs
├── subtyping.rs
├── namespace.rs
├── symbol_map.rs
├── type_annotation.rs
├── diagnostics.rs
├── line_index.rs
├── error.rs
├── wasm.rs
├── semantic/
│   ├── mod.rs
│   ├── generics.rs
│   └── queries.rs
└── bridge/
    ├── context.rs
    ├── declare.rs
    ├── narrow.rs
    ├── parse.rs
    ├── expressions/
    │   ├── mod.rs
    │   ├── core.rs
    │   ├── binary.rs
    │   ├── calls.rs
    │   ├── functions.rs
    │   ├── generics.rs
    │   ├── logical.rs
    │   ├── members.rs
    │   └── objects.rs
    └── statements/
        ├── mod.rs
        ├── functions.rs
        ├── classes.rs
        ├── control_flow.rs
        ├── variables.rs
        ├── patterns.rs
        └── support.rs

tests/
├── foundation.rs
├── structural_types.rs
├── literal_widening.rs
├── control_flow_narrowing.rs
├── classes_inheritance.rs
├── expression_program_checking.rs
├── parameters_destructuring.rs
└── generics_tier1.rs

tests/fixtures/
├── foundation/
├── structural-types/
├── literal-widening/
├── control-flow-narrowing/
├── classes-inheritance/
├── expression-program-checking/
├── parameters-destructuring/
└── generics-tier1/

docs/
├── README.md
├── architecture.md
├── roadmap.md
├── testing.md
├── purpose-and-overdesign.md
└── <semantic milestone documents>

scripts/
├── ci.sh
├── harness.sh
└── enable-hooks.sh

benches/                 # Criterion and IAI benchmarks
examples/                # Small usage and profiling examples
bin/                     # CLI target
```

## Architecture direction

The current implementation deliberately builds on Oxc instead of recreating its parser or front end. ts-rust owns the TypeScript semantic layer.

The next architectural boundary is the small `semantic::queries` API. It is intentionally read-only and narrow today. As generic inference, indexed access, overloads, and advanced type operations grow, semantic queries can expand without forcing AST-facing feature modules to know how the underlying algorithms are stored.

The future direction is:

```text
Oxc
  ↓
bridge feature modules
  ↓
semantic queries
  ↓
type relations / generics / flow / resolution
  ↓
semantic facts + diagnostics
  ↓
tooling or typed IR
```

This is an incremental boundary, not a second compiler architecture.

## Development

Format:

```bash
cargo fmt --all
```

Check:

```bash
cargo check --all-targets --all-features
```

Lint:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Test:

```bash
cargo test --all-targets --all-features
```

Run the repository quality gate:

```bash
./scripts/ci.sh
```

## Semantic milestones and regression contracts

Each semantic milestone maps directly to an executable regression suite. These are development contracts, not package release versions.

Start with the [documentation index](docs/README.md), then inspect the relevant fixture directory and integration test together.

| Milestone | Test runner | Fixture directory | Documentation |
| --- | --- | --- | --- |
| Foundation | `tests/foundation.rs` | `tests/fixtures/foundation/` | [Foundation](docs/foundation.md) |
| Structural Types | `tests/structural_types.rs` | `tests/fixtures/structural-types/` | [Structural Types](docs/structural-types.md) |
| Literal Widening | `tests/literal_widening.rs` | `tests/fixtures/literal-widening/` | [Literal Widening](docs/literal-widening.md) |
| Control-Flow Narrowing | `tests/control_flow_narrowing.rs` | `tests/fixtures/control-flow-narrowing/` | [Control-Flow Narrowing](docs/control-flow-narrowing.md) |
| Classes and Inheritance | `tests/classes_inheritance.rs` | `tests/fixtures/classes-inheritance/` | [Classes and Inheritance](docs/classes-inheritance.md) |
| Expression and Program Checking | `tests/expression_program_checking.rs` | `tests/fixtures/expression-program-checking/` | [Expression and Program Checking](docs/expression-program-checking.md) |
| Parameters and Destructuring | `tests/parameters_destructuring.rs` | `tests/fixtures/parameters-destructuring/` | [Parameters and Destructuring](docs/parameters-destructuring.md) |
| Generics Tier 1 | `tests/generics_tier1.rs` | `tests/fixtures/generics-tier1/` | [Generics Tier 1](docs/generics-tier1.md) |

The current Generics Tier 1 suite is intentionally the active semantic frontier. It should grow with generic constraints, substitution, instantiation, and inference rather than being replaced by another opaque numbered stage.

## Documentation

Start with [docs/README.md](docs/README.md) for the milestone map and architecture.

- [Architecture](docs/architecture.md): ownership, dependencies, semantic boundaries, and future scaling.
- [Roadmap](docs/roadmap.md): the next semantic capabilities and their intended order.
- [Testing](docs/testing.md): fixture conventions, regression policy, benchmarks, and compatibility harness.
- [Purpose and boundaries](docs/purpose-and-overdesign.md): why the project exists and what it deliberately does not rebuild.

Each milestone document explains the semantic rule, implementation choice, and protected behavior for that capability.

## Compatibility harness

The repository contains an experimental harness for comparing ts-rust with TypeScript implementations such as `tsc` and `tsgo`.

It records information such as:

- exit codes
- normalized diagnostics
- timing
- repeated-run statistics
- output hashes

The harness is useful for finding compatibility gaps and performance questions.

**It is not a semantic-equivalence oracle.**

A matching result does not prove that two compilers implement the same TypeScript semantics.

### Running it locally

```bash
./scripts/harness.sh
```

This clones a fixture project (Zustand, by default), installs its dependencies, builds `ts-rust` in release mode if needed, and benchmarks whichever of `tsc`/`tsgo`/`ts-rust` are available. Results are written to `.harness/results/<timestamp>/summary.json`.

Common variations:

```bash
# Benchmark your own project instead of Zustand
./scripts/harness.sh /path/to/your/ts/project

# Skip dependency install if node_modules is already set up
SKIP_INSTALL=1 ./scripts/harness.sh

# More timed runs for steadier medians
HARNESS_RUNS=10 ./scripts/harness.sh

# Reuse whatever fixture is already cached instead of auto-refreshing it
HARNESS_REFRESH=0 ./scripts/harness.sh
```

### Running it in CI

The harness is **not** part of the `ci.sh` quality gate and never runs automatically on push or pull request — it hits the network, depends on an external fixture repository, and is expected to report diagnostics that disagree with `tsc`/`tsgo` given the current state of the project (see [Category 2/3 of the roadmap](docs/purpose-and-overdesign.md)), so it must not block merges.

It can be run on demand from GitHub Actions instead: go to **Actions → Compatibility Harness → Run workflow**. Results are uploaded as a downloadable artifact (`harness-results`), not committed to the repository.

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting and security expectations.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

When changing compiler behavior:

1. identify the semantic rule
2. inspect the relevant stage fixtures
3. implement the smallest coherent semantic change
4. add or update a regression fixture
5. run the full quality gate

## License

See [MIT License](LICENSE-MIT) or [Apache License](LICENSE-APACHE).
