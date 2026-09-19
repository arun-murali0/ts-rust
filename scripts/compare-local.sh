#!/usr/bin/env bash
set -u
set -o pipefail

# Fast, local, no-network diagnostic comparison for a single file or a fixture
# directory, for use while iterating on a feature — not the CI/zustand harness.
#
# Unlike scripts/harness.sh, this script:
#   - never clones anything or installs dependencies
#   - takes a path you already have on disk (defaults to tests/fixtures)
#   - skips timing entirely; it only diffs diagnostic output
#   - prints a unified diff per compiler pair instead of a pass/fail summary
#
# Usage:
#   ./scripts/compare-local.sh                        # every fixture under tests/fixtures
#   ./scripts/compare-local.sh path/to/one_file.ts     # a single file
#   ./scripts/compare-local.sh path/to/some/dir        # every .ts file under a dir
#
# Environment:
#   TSC_BIN        tsc command override (default: npx --no-install tsc)
#   TSGO_BIN       tsgo command override (default: npx --no-install tsgo)
#   TS_RUST_BIN    ts-rust command override (default: target/release/ts-rust,
#                  falls back to a debug build if release is missing)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-${ROOT}/tests/fixtures}"
TSC_BIN="${TSC_BIN:-npx --no-install tsc}"
TSGO_BIN="${TSGO_BIN:-npx --no-install tsgo}"
TS_RUST_BIN="${TS_RUST_BIN:-}"

RESULTS_DIR="${ROOT}/.harness/local"
rm -rf "${RESULTS_DIR}"
mkdir -p "${RESULTS_DIR}"

die() {
  echo "error: $*" >&2
  exit 2
}

have() {
  command -v "$1" >/dev/null 2>&1
}

# Resolve ts-rust: prefer an explicit override, then release, then debug, then
# build the debug binary on the spot — debug because this is a fast local
# loop, not a benchmark.
if [ -z "${TS_RUST_BIN}" ]; then
  if [ -x "${ROOT}/target/release/ts-rust" ]; then
    TS_RUST_BIN="${ROOT}/target/release/ts-rust"
  elif [ -x "${ROOT}/target/debug/ts-rust" ]; then
    TS_RUST_BIN="${ROOT}/target/debug/ts-rust"
  else
    echo "ts-rust binary not found, building debug build..."
    (cd "${ROOT}" && cargo build --bin ts-rust) || die "cargo build failed"
    TS_RUST_BIN="${ROOT}/target/debug/ts-rust"
  fi
fi

[ -e "${TARGET}" ] || die "no such file or directory: ${TARGET}"

if [ -f "${TARGET}" ]; then
  mapfile -t FILES <<< "${TARGET}"
else
  mapfile -t FILES < <(find "${TARGET}" -type f -name '*.ts' | sort)
fi

[ "${#FILES[@]}" -gt 0 ] || die "no .ts files found under ${TARGET}"

TSC_AVAILABLE=0
if bash -lc "${TSC_BIN} --version" >/dev/null 2>&1; then
  TSC_AVAILABLE=1
else
  echo "warning: tsc unavailable (${TSC_BIN}); skipping tsc comparisons" >&2
fi

TSGO_AVAILABLE=0
if bash -lc "${TSGO_BIN} --version" >/dev/null 2>&1; then
  TSGO_AVAILABLE=1
else
  echo "warning: tsgo unavailable (${TSGO_BIN}); skipping tsgo comparisons" >&2
fi

[ "${TSC_AVAILABLE}" -eq 1 ] || [ "${TSGO_AVAILABLE}" -eq 1 ] ||
  die "neither tsc nor tsgo is available locally; nothing to compare against"

normalize() {
  # Same normalization intent as harness.sh: strip absolute paths and
  # compiler-specific summary/timing noise so the diff is about diagnostics,
  # not incidental formatting.
  sed \
    -e "s#${ROOT}#<repo>#g" \
    -e 's/[[:space:]]*$//' \
    -e '/^[[:space:]]*$/d' \
    "$1" |
    grep -Ev '^(Version|Files:|Lines:|Identifiers:|Symbols:|Types:|Instantiations:|Memory used:|I/O read:|I/O write:|Parse time:|Bind time:|Check time:|Emit time:|Total time:)' \
    || true
}

any_diff=0

for file in "${FILES[@]}"; do
  name="$(basename "${file}")"
  echo "== ${file} =="

  bash -lc "${TS_RUST_BIN} '${file}'" >"${RESULTS_DIR}/${name}.ts-rust.txt" 2>&1
  normalize "${RESULTS_DIR}/${name}.ts-rust.txt" > "${RESULTS_DIR}/${name}.ts-rust.norm.txt"

  if [ "${TSC_AVAILABLE}" -eq 1 ]; then
    bash -lc "${TSC_BIN} --noEmit '${file}'" >"${RESULTS_DIR}/${name}.tsc.txt" 2>&1
    normalize "${RESULTS_DIR}/${name}.tsc.txt" > "${RESULTS_DIR}/${name}.tsc.norm.txt"
    if ! diff -u "${RESULTS_DIR}/${name}.tsc.norm.txt" "${RESULTS_DIR}/${name}.ts-rust.norm.txt" \
         > "${RESULTS_DIR}/${name}.tsc-vs-ts-rust.diff" 2>&1; then
      any_diff=1
      echo "  tsc vs ts-rust: DIFFERS -- see ${RESULTS_DIR}/${name}.tsc-vs-ts-rust.diff"
    else
      echo "  tsc vs ts-rust: match"
    fi
  fi

  if [ "${TSGO_AVAILABLE}" -eq 1 ]; then
    bash -lc "${TSGO_BIN} --noEmit '${file}'" >"${RESULTS_DIR}/${name}.tsgo.txt" 2>&1
    normalize "${RESULTS_DIR}/${name}.tsgo.txt" > "${RESULTS_DIR}/${name}.tsgo.norm.txt"
    if ! diff -u "${RESULTS_DIR}/${name}.tsgo.norm.txt" "${RESULTS_DIR}/${name}.ts-rust.norm.txt" \
         > "${RESULTS_DIR}/${name}.tsgo-vs-ts-rust.diff" 2>&1; then
      any_diff=1
      echo "  tsgo vs ts-rust: DIFFERS -- see ${RESULTS_DIR}/${name}.tsgo-vs-ts-rust.diff"
    else
      echo "  tsgo vs ts-rust: match"
    fi
  fi
done

echo
echo "Results kept in ${RESULTS_DIR} (overwritten on next run)."

if [ "${any_diff}" -eq 1 ]; then
  echo "One or more files disagree with a reference compiler. This is informational for local dev; exiting nonzero so it's visible in a script chain."
  exit 1
fi

echo "All compared files agree."
