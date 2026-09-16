#!/usr/bin/env bash
set -u
set -o pipefail

# Experimental compatibility/benchmark harness. The default fixture is cached so
# repeated development runs do not reclone the repository or reinstall dependencies.
#
# Usage:
#   ./scripts/harness.sh
#   ./scripts/harness.sh /path/to/zustand
#
# Environment:
#   ZUSTAND_REF          Git ref used when creating/refreshing the cached fixture (main)
#   HARNESS_REFRESH      Refresh the harness-owned fixture from its remote (0)
#   HARNESS_RUNS         Timed runs per compiler (3)
#   TSC_BIN              tsc command override
#   TSGO_BIN             tsgo command override
#   TS_RUST_BIN          ts-rust command override
#   SKIP_INSTALL         Skip dependency installation (0)
#   CLEANUP              Remove the harness-owned fixture after the run (0)
#
# Diagnostic agreement is a regression signal, not a semantic-equivalence claim.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="${1:-${ROOT}/.harness/zustand}"
REF="${ZUSTAND_REF:-main}"
RUNS="${HARNESS_RUNS:-3}"
TSC_BIN="${TSC_BIN:-}"
TSGO_BIN="${TSGO_BIN:-}"
TS_RUST_BIN="${TS_RUST_BIN:-${ROOT}/target/release/ts-rust}"
HARNESS_REFRESH="${HARNESS_REFRESH:-0}"
SKIP_INSTALL="${SKIP_INSTALL:-0}"
CLEANUP="${CLEANUP:-0}"

RESULTS_DIR="${ROOT}/.harness/results"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${RESULTS_DIR}/${RUN_ID}"
mkdir -p "${RUN_DIR}"

die() {
  echo "error: $*" >&2
  exit 2
}

have() {
  command -v "$1" >/dev/null 2>&1
}

# Run a shell command, capture stdout/stderr and wall time in milliseconds.
run_case() {
  local name="$1"
  local cmd="$2"
  local out="${RUN_DIR}/${name}.output.txt"
  local err="${RUN_DIR}/${name}.error.txt"
  local meta="${RUN_DIR}/${name}.meta"

  local start end rc
  start="$(date +%s%N)"
  bash -lc "${cmd}" >"${out}" 2>"${err}"
  rc=$?
  end="$(date +%s%N)"

  local elapsed_ms=$(( (end - start) / 1000000 ))
  local out_hash err_hash
  if have sha256sum; then
    out_hash="$(sha256sum "${out}" | awk '{print $1}')"
    err_hash="$(sha256sum "${err}" | awk '{print $1}')"
  else
    out_hash="$(shasum -a 256 "${out}" | awk '{print $1}')"
    err_hash="$(shasum -a 256 "${err}" | awk '{print $1}')"
  fi

  {
    echo "exit_code=${rc}"
    echo "elapsed_ms=${elapsed_ms}"
    echo "stdout_sha256=${out_hash}"
    echo "stderr_sha256=${err_hash}"
  } > "${meta}"

  return 0
}

extract_field() {
  awk -F= -v key="$2" '$1 == key {print $2}' "$1"
}

normalize_diagnostics() {
  # Normalize compiler-specific timing/summary noise and paths so the comparison
  # focuses on diagnostic shape. Keep the original outputs for manual review.
  sed \
    -e "s#${FIXTURE}#<fixture>#g" \
    -e 's/[[:space:]]*$//' \
    -e '/^[[:space:]]*$/d' \
    "$1" |
    grep -Ev '^(Version|Files:|Lines:|Identifiers:|Symbols:|Types:|Instantiations:|Memory used:|I/O read:|I/O write:|Parse time:|Bind time:|Check time:|Emit time:|Total time:)' \
    || true
}

echo "== ts-rust experimental harness =="
echo "fixture: ${FIXTURE}"
echo "ref:     ${REF}"
echo "runs:    ${RUNS}"
echo "results: ${RUN_DIR}"
echo

HARNESS_OWNED=0
if [ -z "${1:-}" ]; then
  HARNESS_OWNED=1
  mkdir -p "$(dirname "${FIXTURE}")"
  if [ ! -d "${FIXTURE}/.git" ]; then
    echo "Fetching Zustand ${REF} (first run only)..."
    git clone --depth 1 --branch "${REF}" https://github.com/pmndrs/zustand.git "${FIXTURE}" ||
      die "could not clone Zustand; check network access or pass an existing local checkout"
  elif [ "${HARNESS_REFRESH}" = "1" ]; then
    echo "Refreshing cached Zustand fixture to ${REF}..."
    git -C "${FIXTURE}" fetch --depth 1 origin "${REF}" || die "could not refresh Zustand ${REF}"
    git -C "${FIXTURE}" checkout --detach FETCH_HEAD || die "could not update cached Zustand fixture"
  fi
fi

cd "${FIXTURE}" || die "cannot enter fixture"

hash_file() {
  if have sha256sum; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

install_dependencies() {
  [ "${SKIP_INSTALL}" = "1" ] && return 0

  local manager lockfile marker current fixture_key
  if [ -f pnpm-lock.yaml ] && have pnpm; then
    manager="pnpm"
    lockfile="pnpm-lock.yaml"
  elif [ -f package-lock.json ] && have npm; then
    manager="npm"
    lockfile="package-lock.json"
  elif [ -f yarn.lock ] && have yarn; then
    manager="yarn"
    lockfile="yarn.lock"
  else
    echo "warning: no supported package manager/lockfile found; skipping dependency install" >&2
    return 0
  fi

  fixture_key="$(printf '%s' "${FIXTURE}" | hash_file /dev/stdin)"
  marker="${ROOT}/.harness/install-markers/${fixture_key}"
  mkdir -p "$(dirname "${marker}")"
  current="${manager}:$(hash_file "${lockfile}")"
  if [ -d node_modules ] && [ -f "${marker}" ] && [ "$(cat "${marker}")" = "${current}" ]; then
    echo "Dependencies already installed for ${manager} (${lockfile}); skipping install."
    return 0
  fi

  echo "Installing ${manager} dependencies (only when the lockfile changes)..."
  if ! case "${manager}" in
    pnpm) pnpm install --frozen-lockfile ;;
    npm) npm ci ;;
    yarn) yarn install --frozen-lockfile ;;
  esac
  then
    die "dependency installation failed"
  fi
  printf '%s\n' "${current}" > "${marker}"
}

install_dependencies

# Prefer project-local compiler binaries so npx never downloads during a benchmark.
if [ -z "${TSC_BIN}" ]; then
  if [ -x "${FIXTURE}/node_modules/.bin/tsc" ]; then TSC_BIN="${FIXTURE}/node_modules/.bin/tsc"; else TSC_BIN="npx --no-install tsc"; fi
fi
if [ -z "${TSGO_BIN}" ]; then
  if [ -x "${FIXTURE}/node_modules/.bin/tsgo" ]; then TSGO_BIN="${FIXTURE}/node_modules/.bin/tsgo"; else TSGO_BIN="npx --no-install tsgo"; fi
fi

# Use the repository's own tsconfig when available.
if [ ! -f tsconfig.json ]; then
  die "fixture has no tsconfig.json"
fi

cd "${ROOT}" || die "cannot return to repository root"

# Resolve binaries early so missing tools are reported cleanly.
for required in bash git sed awk grep date; do
  have "${required}" || die "required command missing: ${required}"
done

if ! bash -lc "${TSC_BIN} --version" >/dev/null 2>&1; then
  echo "warning: tsc unavailable: ${TSC_BIN}" >&2
  TSC_AVAILABLE=0
else
  TSC_AVAILABLE=1
fi

if ! bash -lc "${TSGO_BIN} --version" >/dev/null 2>&1; then
  echo "warning: tsgo unavailable: ${TSGO_BIN}" >&2
  TSGO_AVAILABLE=0
else
  TSGO_AVAILABLE=1
fi

if ! bash -lc "${TS_RUST_BIN} --help" >/dev/null 2>&1; then
  # Build the default binary once; an explicit override is always respected.
  if [ "${TS_RUST_BIN}" = "${ROOT}/target/release/ts-rust" ]; then
    echo "ts-rust binary not found at ${TS_RUST_BIN}, building it..."
    if bash -lc "cd '${ROOT}' && cargo build --release --bin ts-rust"; then
      echo "Build succeeded."
    else
      echo "warning: cargo build failed; ts-rust will be skipped" >&2
    fi
  fi
fi

if ! bash -lc "${TS_RUST_BIN} --help" >/dev/null 2>&1; then
  echo "warning: ts-rust unavailable: ${TS_RUST_BIN}" >&2
  TS_RUST_AVAILABLE=0
else
  TS_RUST_AVAILABLE=1
fi

[ "${TSC_AVAILABLE}" -eq 1 ] || [ "${TSGO_AVAILABLE}" -eq 1 ] || [ "${TS_RUST_AVAILABLE}" -eq 1 ] ||
  die "no compiler is available"

# All compilers receive the same project path; the ts-rust CLI remains configurable.
PROJECT_ARG="--project ${FIXTURE}/tsconfig.json"
TSC_CMD="${TSC_BIN} --noEmit ${PROJECT_ARG}"
TSGO_CMD="${TSGO_BIN} --noEmit ${PROJECT_ARG}"
TS_RUST_CMD="${TS_RUST_BIN} ${PROJECT_ARG}"

declare -a compilers=("tsc" "tsgo" "ts-rust")

for compiler in "${compilers[@]}"; do
  case "${compiler}" in
    tsc) enabled="${TSC_AVAILABLE}"; cmd="${TSC_CMD}" ;;
    tsgo) enabled="${TSGO_AVAILABLE}"; cmd="${TSGO_CMD}" ;;
    ts-rust) enabled="${TS_RUST_AVAILABLE}"; cmd="${TS_RUST_CMD}" ;;
  esac

  [ "${enabled}" -eq 1 ] || continue

  for run in $(seq 1 "${RUNS}"); do
    run_case "${compiler}.${run}" "${cmd}"
  done

  # Normalize the last run for comparison.
  normalize_diagnostics "${RUN_DIR}/${compiler}.${RUNS}.output.txt" \
    > "${RUN_DIR}/${compiler}.normalized.txt"
done

python3 - "${RUN_DIR}" "${RUNS}" <<'PY'
import json, os, re, statistics, sys
from pathlib import Path

run_dir = Path(sys.argv[1])
runs = int(sys.argv[2])
names = ["tsc", "tsgo", "ts-rust"]

data = {}
for name in names:
    metas = []
    for i in range(1, runs + 1):
        p = run_dir / f"{name}.{i}.meta"
        if not p.exists():
            continue
        vals = {}
        for line in p.read_text().splitlines():
            k, v = line.split("=", 1)
            vals[k] = v
        metas.append(vals)
    if metas:
        times = [int(x["elapsed_ms"]) for x in metas]
        data[name] = {
            "runs": len(metas),
            "exit_codes": [int(x["exit_code"]) for x in metas],
            "time_ms": times,
            "median_ms": statistics.median(times),
            "min_ms": min(times),
            "max_ms": max(times),
        }

def normalized(name):
    p = run_dir / f"{name}.normalized.txt"
    return p.read_text(errors="replace") if p.exists() else None

comparison = {
    "tsc_vs_tsgo": None,
    "tsrust_vs_tsc": None,
    "tsrust_vs_tsgo": None,
}

def compare(a, b):
    x, y = normalized(a), normalized(b)
    if x is None or y is None:
        return None
    return {
        "exact_normalized_output_match": x == y,
        "a_output_bytes": len(x.encode()),
        "b_output_bytes": len(y.encode()),
    }

comparison["tsc_vs_tsgo"] = compare("tsc", "tsgo")
comparison["tsrust_vs_tsc"] = compare("ts-rust", "tsc")
comparison["tsrust_vs_tsgo"] = compare("ts-rust", "tsgo")

if "ts-rust" in data and "tsc" in data:
    base = data["tsc"]["median_ms"]
    actual = data["ts-rust"]["median_ms"]
    data["ts-rust"]["vs_tsc_speedup"] = (base / actual) if actual else None

if "ts-rust" in data and "tsgo" in data:
    base = data["tsgo"]["median_ms"]
    actual = data["ts-rust"]["median_ms"]
    data["ts-rust"]["vs_tsgo_speedup"] = (base / actual) if actual else None

summary = {
    "schema": 1,
    "fixture": "pmndrs/zustand",
    "runs": runs,
    "compilers": data,
    "comparisons": comparison,
    "note": "Diagnostic agreement is an experimental signal, not proof of semantic correctness.",
}
(run_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
PY

echo
echo "Results written to: ${RUN_DIR}"
echo "No production-ready claim is made by this harness."
echo "For reproducibility, record the Zustand ref/commit and compiler versions alongside the results."

if [ "${HARNESS_OWNED}" -eq 1 ] && [ "${CLEANUP}" = "1" ]; then
  rm -rf "${FIXTURE}"
fi
