#!/usr/bin/env bash
set -u
set -o pipefail

# Fast, local diagnostic comparison against real tsc -- for the whole
# tests/fixtures/ suite by default, or a narrower path when given one.
#
# All of the actual comparison logic (running tsc via the TypeScript
# compiler API, merging it against ts-rust's own output, rendering the
# MATCH/GAP/FALSE-POSITIVE table) lives in scripts/ts-diag-tool/compare.js.
# This script only builds ts-rust and invokes that.
#
# Why the compiler API instead of shelling `tsc` and text-parsing its
# output: a type name can itself contain parens or colons, which broke the
# old awk-based line parser. ts.Diagnostic gives code/category/message as
# real structured fields -- see scripts/ts-diag-tool/check-fixtures.js.
#
# Unlike bin/compare-tsc.rs (which is in-process, errors-only, and scoped to
# the small dedicated tests/tsc-conformance/ set), this script:
#   - defaults to every .ts file under tests/fixtures/ -- the real, full
#     feature-tier suite, not a curated subset
#   - is informational, not pass/fail: many fixtures under tests/fixtures/
#     deliberately exercise this checker's OWN recovery behavior (e.g. one
#     unresolvable class member making the whole class unsupported) and are
#     expected to diverge from tsc -- this never exits nonzero for that
#
# Usage:
#   ./scripts/compare-local.sh                        # all of tests/fixtures
#   ./scripts/compare-local.sh tests/fixtures/generics-tier1
#   ./scripts/compare-local.sh path/to/one_file.ts
#   ./scripts/compare-local.sh tests/fixtures --strict-only   # old flag set
#   ./scripts/compare-local.sh tests/fixtures --json          # machine-readable
#
# Environment:
#   TS_RUST_BIN    ts-rust command override (default: target/release/ts-rust,
#                  falls back to a debug build if release is missing)
#   NO_COLOR       set to disable colored output (also auto-disabled when
#                  stdout is not a terminal)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}" || exit 2

TARGET="${1:-tests/fixtures}"
shift || true
EXTRA_ARGS=("$@")

DIAG_TOOL_DIR="scripts/ts-diag-tool"

if [ ! -f "${DIAG_TOOL_DIR}/node_modules/typescript/package.json" ]; then
  echo "Installing scripts/ts-diag-tool dependencies (typescript@5.9.3, pinned -- see its package.json)..." >&2
  (cd "${DIAG_TOOL_DIR}" && npm install --no-audit --no-fund) || {
    echo "error: npm install failed in ${DIAG_TOOL_DIR}" >&2
    exit 2
  }
fi

if [ -z "${TS_RUST_BIN:-}" ]; then
  if [ -x "target/release/ts-rust" ]; then
    TS_RUST_BIN="target/release/ts-rust"
  else
    echo "No release build found; building target/release/ts-rust..." >&2
    cargo build --release --bin ts-rust || {
      echo "error: cargo build failed" >&2
      exit 2
    }
    TS_RUST_BIN="target/release/ts-rust"
  fi
fi

if [ ! -x "${TS_RUST_BIN}" ]; then
  echo "error: ts-rust binary not found or not executable at ${TS_RUST_BIN}" >&2
  exit 2
fi

if [ ! -e "${TARGET}" ]; then
  echo "error: no such file or directory: ${TARGET}" >&2
  exit 2
fi

exec node "${DIAG_TOOL_DIR}/compare.js" "${TARGET}" "${TS_RUST_BIN}" "${EXTRA_ARGS[@]}"
