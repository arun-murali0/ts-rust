# Verifying this build locally

Every AST and semantic-analysis field or method this crate calls into
(`ParserReturn::diagnostics`, `BindingPattern` as a direct enum,
`VariableDeclarator::type_annotation`, `FormalParameter::type_annotation`,
`BinaryOperator::as_str`, `Argument::as_expression`, the `TSType` keyword
variants, `SemanticBuilder::build`, `Scoping::get_reference`,
`IdentifierReference::reference_id`, `BindingIdentifier::symbol_id`) was
checked against the real `oxc` source at tag `crates_v0.144.0`, not assumed
from an older or unrelated version. It has not been built with a full
`cargo test` run in this environment, so do that before merging. The steps
below are listed in the order worth running them.

## 1. Build

```sh
cargo check --all-targets
cargo check --features wasm --target wasm32-unknown-unknown
```

`oxc` 0.144.0 requires a toolchain new enough to support the 2024 edition
(1.85+). If `cargo check` fails with `feature edition2024 is required`,
that's the toolchain, not this crate: update Rust first.

## 2. Tests

```sh
cargo test --all-targets -- --nocapture
```

The six fixtures in `tests/v1_fixtures.rs` and the fixtures in
`tests/v2_fixtures.rs` should all pass. A failure here after a clean build
is a logic bug, not an API mismatch. For a first signal on *why* a
diagnostic didn't fire, or fired wrong, run with tracing:

```sh
RUST_LOG=trace cargo test primitive_mismatch_is_caught -- --nocapture
```

`resolve_identifier_type` in `bridge/expressions.rs` logs a `warn`-level
line whenever oxc's binder resolves a reference to a real symbol that has
no entry in `SymbolTypeMap`. This is expected for a handful of known gaps
(an unannotated variable, a destructured parameter), but it is also
exactly what a missing declaration step for some binding kind looks like.
If a test's diagnostic count looks off, run it with `RUST_LOG=warn` first;
it will usually point straight at the binding that never got registered.
This class of bug already happened once: an earlier version of
`bridge/statements.rs` never declared function parameters into the symbol
map, so every parameter reference inside a function body silently
resolved to the internal error sentinel. Anything routed through
`is_subtype` swallowed that silently; the `+` operator, which compares
types directly rather than through `is_subtype`, turned it into a false
type-mismatch diagnostic on ordinary arithmetic. Keep an eye on this
whenever a new binding kind (destructuring, catch clauses, loop variables)
is added to declare.rs or statements.rs: it needs a registration step
before anything inside its scope is checked.

## 3. Benchmarks

```sh
cargo bench
```

Runs the tiny / 50-function / 1000-function cases in
`benches/checker_benchmark.rs`. Watch for non-linear scaling between the
50- and 1000-function runs. `SymbolTypeMap` is a flat `HashMap` keyed by
`SymbolId`, which is fine at V1/V2's scale but worth revisiting if this
benchmark shows trouble as later versions add more bindings per file.
No fixed performance target is set yet; the first real run becomes the
baseline.

### Flamegraphs (Linux only)

```sh
echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid   # once per boot
cargo bench -- --profile-time=10
```

`--profile-time` switches Criterion into profiling mode instead of a
normal statistical run: it samples each benchmark with `pprof` for the
given number of seconds and writes an SVG to
`target/criterion/<benchmark name>/profile/flamegraph.svg` (for example
`target/criterion/check_source/large_1000_functions/profile/flamegraph.svg`).
Open it in a browser; wide frames are where time is actually going.
Skip the `perf_event_paranoid` step if it's already `-1` or you're
running as root (some containers set this already).

## 4. Lint / format

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Not run in this environment. Expect some initial clippy noise, most likely
around the nested `let-else` chains in `declare.rs` and `statements.rs`.
