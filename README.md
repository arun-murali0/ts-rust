# ts-rust

> A Rust-based experimental TypeScript type checker built on top of Oxc.

## ⚠️ Experimental, not production-ready

**ts-rust is experimental software. Do not use it as a production TypeScript compiler or as a drop-in replacement for `tsc`.**

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
- future generics, inference, and advanced type-level semantics

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

The completed historical stages are:

```text
v1  Initial type-checking foundation
v2  Structural types, declarations, and resolution
v3a Literal types and widening
v3b Control-flow narrowing
v3c Classes and inheritance
v4  Expression and program checking
v5  Parameters and destructuring
v6  Generics and inference       ← WIP
```

The exact behavior of each stage is documented from the actual regression fixtures in `tests/fixtures/`.

## Repository structure

```text
src/
├── arena.rs             # Arena-backed TypeId storage
├── types.rs             # Semantic type representation
├── subtyping.rs         # Assignability and subtype relations
├── namespace.rs         # Declaration/type namespace
├── symbol_map.rs        # Oxc SymbolId → ts-rust type mapping
├── type_annotation.rs   # TypeScript annotation → TypeId
├── diagnostics.rs       # Structured diagnostics
├── line_index.rs        # Source position utilities
├── error.rs             # Error types
├── wasm.rs              # WASM-facing API
└── bridge/
    ├── context.rs       # Checker semantic context
    ├── declare.rs       # Declaration handling
    ├── expressions.rs   # Expression type inference
    ├── narrow.rs        # TypeScript-aware flow narrowing
    ├── statements.rs    # Statement checking and bindings
    └── parse.rs         # Oxc parsing bridge

tests/
├── fixtures/v1-v6/      # Incremental semantic regression suites
├── v1_fixtures.rs       # Stage test runners
├── v2_fixtures.rs
├── v3_fixtures.rs
├── v4_fixtures.rs
├── v5_fixtures.rs
└── v6_fixtures.rs

docs/
├── README.md
├── v1-foundation.md
├── v2-structural-types.md
├── v3a-literals-and-widening.md
├── v3b-control-flow-narrowing.md
├── v3c-classes-and-inheritance.md
├── v4-expression-and-program-checking.md
├── v5-parameters-and-destructuring.md
└── purpose-and-overdesign.md

scripts/
├── ci.sh               # Local/CI quality gate
├── harness.sh          # Experimental compiler comparison harness
└── enable-hooks.sh     # Git hook setup

benches/                 # Benchmarks
examples/                # Small usage examples
bin/                     # CLI target
```

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

## Tests and development history

The most important development record is under:

```text
tests/fixtures/
```

The progression from v1 to v5 is documented as an architecture case study, with each document explaining the actual code structure, design choices, and reasons behind them.

Start here:

- [Architecture case-study index](docs/README.md)
- [v1 foundation](docs/v1-foundation.md)
- [v2 structural types](docs/v2-structural-types.md)
- [v3a literals and widening](docs/v3a-literals-and-widening.md)
- [v3b control-flow narrowing](docs/v3b-control-flow-narrowing.md)
- [v3c classes and inheritance](docs/v3c-classes-and-inheritance.md)
- [v4 expression and program checking](docs/v4-expression-and-program-checking.md)
- [v5 parameters and destructuring](docs/v5-parameters-and-destructuring.md)
- [Purpose, design boundaries, and future plan](docs/purpose-and-overdesign.md)

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

See [LICENSE](LICENSE).
