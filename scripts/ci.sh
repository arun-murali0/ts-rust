#!/usr/bin/env bash
set -euo pipefail

# One gate for local runs and CI. CI sets two variables to pick what it covers:
#   CI_FEATURE_ARGS  cargo feature flags (default: --all-features)
#   CI_FULL          1 (default) also runs formatting, every test and rustdoc;
#                    0 runs only clippy, which type-checks every target for that
#                    feature set, so a feature that builds only with another on fails.
read -r -a FEATURE_ARGS <<<"${CI_FEATURE_ARGS:---all-features}"
FULL="${CI_FULL:-1}"

# Every warning is an error, here and in CI, so a local run cannot pass what CI rejects.
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export RUSTDOCFLAGS="${RUSTDOCFLAGS:--D warnings}"

if [ "$FULL" = "1" ]; then
  # Locally this rewrites files so formatting never blocks a commit. In CI (GitHub
  # sets CI=true) it only checks: formatting a throwaway checkout would let
  # unformatted code pass.
  echo "==> cargo fmt"
  if [ -n "${CI:-}" ]; then
    cargo fmt --all -- --check
  else
    cargo fmt --all
  fi
fi

# --locked: a Cargo.lock that is out of date with Cargo.toml fails the build instead
# of being rewritten quietly. clippy type-checks every target itself, so a separate
# `cargo check` would only repeat that work.
echo "==> cargo clippy (${FEATURE_ARGS[*]})"
cargo clippy --locked --all-targets "${FEATURE_ARGS[@]}" -- -D warnings

if [ "$FULL" = "1" ]; then
  # Benches and examples are already compiled by clippy above. Building them again in
  # the test profile pulls in criterion and iai-callgrind and adds no test coverage.
  echo "==> cargo test"
  cargo test --locked "${FEATURE_ARGS[@]}"

  echo "==> cargo doc"
  cargo doc --locked --no-deps "${FEATURE_ARGS[@]}"
fi

echo "==> CI PASSED"
