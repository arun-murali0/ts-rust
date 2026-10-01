#!/usr/bin/env bash
set -euo pipefail

# Locally this rewrites files so formatting never blocks a commit. In CI (GitHub
# sets CI=true) it only checks: formatting a throwaway checkout would let
# unformatted code pass.
echo "==> cargo fmt"
if [ -n "${CI:-}" ]; then
  cargo fmt --all -- --check
else
  cargo fmt --all
fi

# clippy type-checks every target itself, so a separate `cargo check` would only
# repeat that work.
echo "==> cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

# Benches and examples are already compiled by clippy above. Building them again in
# the test profile pulls in criterion and iai-callgrind and adds no test coverage.
echo "==> cargo test"
cargo test --all-features

echo "==> CI PASSED"
