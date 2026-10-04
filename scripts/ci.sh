#!/usr/bin/env bash
set -euo pipefail

# One gate for local runs and CI: `bash scripts/ci.sh` (or `./scripts/ci.sh` once the file
# is executable: chmod +x scripts/ci.sh). The steps are ordered so the cheapest ones fail
# first, and they reuse each other's build output.

# Every warning is an error, here and in CI, so a local run cannot pass what CI rejects.
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export RUSTDOCFLAGS="${RUSTDOCFLAGS:--D warnings}"

# Locally this rewrites files so formatting never blocks a commit. In CI (GitHub sets
# CI=true) it only checks: formatting a throwaway checkout would let unformatted code pass.
echo "==> cargo fmt"
if [ -n "${CI:-}" ]; then
  cargo fmt --all -- --check
else
  cargo fmt --all
fi

# --locked: a Cargo.lock that is out of date with Cargo.toml fails the build instead of
# being rewritten quietly. One clippy run over all targets and features covers library,
# binary, tests, benches and examples, so no separate `cargo check` or `cargo build` is
# needed to see their warnings.
echo "==> cargo clippy (all targets, all features)"
cargo clippy --locked --all-targets --all-features -- -D warnings

# Each feature on its own, and none: catches code that builds only because another
# feature happens to be on (a `use` of an item that exists under a different feature).
# `check` is enough here; nothing is run or linked.
echo "==> cargo hack check (each feature alone)"
if command -v cargo-hack >/dev/null 2>&1; then
  cargo hack check --locked --each-feature --no-dev-deps
else
  if [ -n "${CI:-}" ]; then
    echo "cargo-hack is not installed in CI"
    exit 1
  fi
  echo "skipped: cargo-hack is not installed (cargo install cargo-hack)"
fi

echo "==> cargo test"
cargo test --locked --all-features

echo "==> cargo doc"
cargo doc --locked --no-deps --all-features

echo "==> CI PASSED"
