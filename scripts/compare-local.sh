#!/usr/bin/env bash
set -u
set -o pipefail

# Fast, local, no-network diagnostic comparison against tsc -- for the whole
# tests/fixtures/ suite by default, or a narrower path when given one.
#
# Unlike bin/compare-tsc.rs (which is in-process, errors-only, and scoped to
# the small dedicated tests/tsc-conformance/ set), this script:
#   - defaults to every .ts file under tests/fixtures/ -- the real, full
#     feature-tier suite, not a curated subset
#   - is informational, not pass/fail: many fixtures under tests/fixtures/
#     deliberately exercise this checker's OWN recovery behavior (e.g. one
#     unresolvable class member making the whole class unsupported) and are
#     expected to diverge from tsc -- this never exits nonzero for that
#   - runs each side ONCE across the whole file set instead of once per
#     file, so it stays fast even at 100+ fixtures
#
# Parsing below is deliberately plain POSIX awk (split/sub/index/substr),
# not gawk's match(s, r, array) capture extension -- Ubuntu (and this
# project's own ubuntu-latest CI) often ships mawk as /usr/bin/awk, which
# does not support that extension at all.
#
# Usage:
#   ./scripts/compare-local.sh                        # all of tests/fixtures
#   ./scripts/compare-local.sh tests/fixtures/generics-tier1
#   ./scripts/compare-local.sh path/to/one_file.ts
#
# Environment:
#   TSC_BIN        tsc command override (default: npx --no-install tsc)
#   TS_RUST_BIN    ts-rust command override (default: target/release/ts-rust,
#                  falls back to a debug build if release is missing)
#   NO_COLOR       set to disable colored output (also auto-disabled when
#                  stdout is not a terminal)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}" || exit 2

TARGET="${1:-tests/fixtures}"
TSC_BIN="${TSC_BIN:-npx --no-install tsc}"
TS_RUST_BIN="${TS_RUST_BIN:-}"

RESULTS_DIR="${ROOT}/.harness/local"
rm -rf "${RESULTS_DIR}"
mkdir -p "${RESULTS_DIR}"

die() {
  echo "error: $*" >&2
  exit 2
}

# --- color setup -------------------------------------------------------

if [ -z "${NO_COLOR:-}" ] && [ -t 1 ]; then
  C_RESET=$'\033[0m'; C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'
  C_GREEN=$'\033[32m'; C_RED=$'\033[31m'; C_YELLOW=$'\033[33m'; C_CYAN=$'\033[36m'
else
  C_RESET=''; C_BOLD=''; C_DIM=''; C_GREEN=''; C_RED=''; C_YELLOW=''; C_CYAN=''
fi

# --- resolve ts-rust binary ---------------------------------------------

if [ -z "${TS_RUST_BIN}" ]; then
  if [ -x "${ROOT}/target/release/ts-rust" ]; then
    TS_RUST_BIN="${ROOT}/target/release/ts-rust"
  elif [ -x "${ROOT}/target/debug/ts-rust" ]; then
    TS_RUST_BIN="${ROOT}/target/debug/ts-rust"
  else
    echo "ts-rust binary not found, building debug build..."
    cargo build --bin ts-rust || die "cargo build failed"
    TS_RUST_BIN="${ROOT}/target/debug/ts-rust"
  fi
fi

[ -e "${TARGET}" ] || die "no such file or directory: ${TARGET}"

if [ -f "${TARGET}" ]; then
  SINGLE_FILE="${TARGET}"
  mapfile -t FILES <<< "${TARGET}"
else
  SINGLE_FILE=""
  mapfile -t FILES < <(find "${TARGET}" -type f -name '*.ts' ! -name '*.d.ts' | sort)
fi

[ "${#FILES[@]}" -gt 0 ] || die "no .ts files found under ${TARGET}"

if ! bash -lc "${TSC_BIN} --version" >/dev/null 2>&1; then
  die "tsc unavailable (${TSC_BIN}); nothing to compare against"
fi

# --- run ts-rust once over the whole target -----------------------------

TS_RUST_RAW="${RESULTS_DIR}/ts-rust.raw.txt"

if [ -n "${SINGLE_FILE}" ]; then
  # Isolate exactly this one file so scope never silently widens to its
  # sibling fixtures: symlink it alone into a scratch project root.
  SCRATCH="${RESULTS_DIR}/scratch"
  mkdir -p "${SCRATCH}"
  ln -sf "${ROOT}/${SINGLE_FILE#"${ROOT}"/}" "${SCRATCH}/$(basename "${SINGLE_FILE}")" 2>/dev/null \
    || ln -sf "${SINGLE_FILE}" "${SCRATCH}/$(basename "${SINGLE_FILE}")"
  : > "${SCRATCH}/tsconfig.json"   # contents are never parsed by ts-rust's CLI
  "${TS_RUST_BIN}" --project "${SCRATCH}/tsconfig.json" > "${TS_RUST_RAW}" 2>&1
else
  # tsconfig contents are never parsed -- this only exists so --project has
  # a file to point at; project_root becomes its parent, TARGET itself.
  TMP_TSCONFIG="${TARGET}/.compare-local-tsconfig.json"
  : > "${TMP_TSCONFIG}"
  trap 'rm -f "${TMP_TSCONFIG}"' EXIT
  "${TS_RUST_BIN}" --project "${TMP_TSCONFIG}" > "${TS_RUST_RAW}" 2>&1
  rm -f "${TMP_TSCONFIG}"
  trap - EXIT
fi

# --- run tsc once PER FILE ------------------------------------------------
#
# Deliberately NOT one batched `tsc file1 file2 file3 ...` call. None of the
# fixtures under tests/fixtures/ have import/export, so each is a global
# script -- batching them into one tsc invocation makes tsc treat every file
# as part of the SAME program, and any name reused across two fixtures
# (Point, Animal, Counter, Shape, Color, add, ...) collides as a spurious
# TS2300/TS2451/TS2393 "duplicate identifier" error that has nothing to do
# with real type checking. ts-rust's own side does not have this problem --
# TypeChecker holds zero fields and check_source is called fresh per file in
# bin/ts-rust.rs's loop -- so only the tsc side needs isolating.

TSC_RAW="${RESULTS_DIR}/tsc.raw.txt"
: > "${TSC_RAW}"
for f in "${FILES[@]}"; do
  bash -lc "${TSC_BIN} --strict --noEmit --pretty false --skipLibCheck --noErrorTruncation ${f@Q}" \
    >> "${TSC_RAW}" 2>&1
done

# --- normalize both sides into a merged, tagged record stream ----------
#
# Record shapes:
#   ALL<TAB>relpath
#   ts-rust<TAB>relpath<TAB>line<TAB>message
#   tsc<TAB>relpath<TAB>line<TAB>code<TAB>message
#
# Only error-severity diagnostics are kept on either side. ts-rust's
# warnings are all "not yet checked" implementation-status markers with no
# tsc equivalent (see bin/compare-tsc.rs's module doc comment) -- comparing
# them produces noise, not signal, as we already found by hand.

RECORDS="${RESULTS_DIR}/records.tsv"
: > "${RECORDS}"

for f in "${FILES[@]}"; do
  printf 'ALL\t%s\n' "${f}" >> "${RECORDS}"
done

if [ -n "${SINGLE_FILE}" ]; then
  # Only one file in scope; label every diagnostic with it directly rather
  # than trusting the symlinked path ts-rust echoes back.
  awk -v label="${SINGLE_FILE}" '
    /: error: / {
      n = split($0, parts, ":")
      line = parts[2] + 0
      msg = $0
      sub(/^[^:]*:[0-9]+:[0-9]+: error: /, "", msg)
      printf "ts-rust\t%s\t%s\t%s\n", label, line, msg
    }
  ' "${TS_RUST_RAW}" >> "${RECORDS}"
else
  awk '
    /: error: / {
      n = split($0, parts, ":")
      file = parts[1]
      line = parts[2] + 0
      msg = $0
      sub(/^[^:]*:[0-9]+:[0-9]+: error: /, "", msg)
      printf "ts-rust\t%s\t%s\t%s\n", file, line, msg
    }
  ' "${TS_RUST_RAW}" >> "${RECORDS}"
fi

# tsc line shape: file(line,col): error TS####: message
tsc_parse_awk='
  {
    open = index($0, "(")
    if (open == 0) next
    close_marker = "): error "
    close_at = index($0, close_marker)
    if (close_at == 0) next

    file = substr($0, 1, open - 1)
    inside = substr($0, open + 1, close_at - open - 1)
    n = split(inside, lc, ",")
    line = lc[1] + 0

    after = substr($0, close_at + length(close_marker))
    code_end = index(after, ":")
    if (code_end == 0) next
    code = substr(after, 1, code_end - 1)
    msg = substr(after, code_end + 2)

    if (label != "") file = label
    printf "tsc\t%s\t%s\t%s\t%s\n", file, line, code, msg
  }
'
if [ -n "${SINGLE_FILE}" ]; then
  awk -v label="${SINGLE_FILE}" "${tsc_parse_awk}" "${TSC_RAW}" >> "${RECORDS}"
else
  awk -v label="" "${tsc_parse_awk}" "${TSC_RAW}" >> "${RECORDS}"
fi

# --- render the report ---------------------------------------------------

awk -F'\t' \
  -v C_RESET="${C_RESET}" -v C_BOLD="${C_BOLD}" -v C_DIM="${C_DIM}" \
  -v C_GREEN="${C_GREEN}" -v C_RED="${C_RED}" -v C_YELLOW="${C_YELLOW}" -v C_CYAN="${C_CYAN}" \
'
function repeat(s, n,   out, i) { out = ""; for (i = 0; i < n; i++) out = out s; return out }
function trunc(s, w,    out) {
  gsub(/[ \t]+/, " ", s)
  if (length(s) <= w) return s
  return substr(s, 1, w - 1) "\xe2\x80\xa6"
}
function tier(path,    parts, n) {
  n = split(path, parts, "/")
  if (n >= 3) return parts[1] "/" parts[2] "/" parts[3]
  return "(root)"
}
$1 == "ALL" {
  path = $2
  if (!(path in seen_all)) { seen_all[path] = 1; all_files[++n_all] = path }
  next
}
$1 == "ts-rust" {
  path = $2; line = $3; msg = $4
  key = path SUBSEP line
  if (!(key in tsrust_lines)) tsrust_lines[key] = ""
  tsrust_lines[key] = tsrust_lines[key] (tsrust_lines[key] == "" ? "" : "\x1f") msg
  file_has_diag[path] = 1
  next
}
$1 == "tsc" {
  path = $2; line = $3; code = $4; msg = $5
  key = path SUBSEP line
  entry = code " " msg
  if (!(key in tsc_lines)) tsc_lines[key] = ""
  tsc_lines[key] = tsc_lines[key] (tsc_lines[key] == "" ? "" : "\x1f") entry
  file_has_diag[path] = 1
  next
}
END {
  LW = 6; MW = 40
  total_files = n_all
  total_agree = 0
  total_differ = 0
  total_gap = 0
  total_fp = 0
  current_tier = ""

  for (i = 1; i <= n_all; i++) {
    path = all_files[i]
    t = tier(path)
    if (t != current_tier) {
      if (current_tier != "") print ""
      current_tier = t
      print C_BOLD "== " t " ==" C_RESET
    }

    if (!(path in file_has_diag)) {
      total_agree++
      continue   # clean on both sides -- omitted from the detailed table, tallied only
    }

    delete linenos
    delete ordered
    nlines = 0
    for (key in tsrust_lines) {
      split(key, kp, SUBSEP)
      if (kp[1] == path && !(kp[2] in linenos)) { linenos[kp[2]] = 1; ordered[++nlines] = kp[2] + 0 }
    }
    for (key in tsc_lines) {
      split(key, kp, SUBSEP)
      if (kp[1] == path && !(kp[2] in linenos)) { linenos[kp[2]] = 1; ordered[++nlines] = kp[2] + 0 }
    }
    for (a = 1; a <= nlines; a++)
      for (b = a + 1; b <= nlines; b++)
        if (ordered[b] < ordered[a]) { tmp = ordered[a]; ordered[a] = ordered[b]; ordered[b] = tmp }

    # A line only ts-rust reports is a false positive; a line only tsc reports
    # is a gap (a check not built yet). Lines are compared by presence, not by
    # how many errors each side reports on them.
    has_fp = 0; has_gap = 0
    for (a = 1; a <= nlines; a++) {
      key = path SUBSEP ordered[a]
      if ((key in tsrust_lines) && !(key in tsc_lines)) has_fp = 1
      if ((key in tsc_lines) && !(key in tsrust_lines)) has_gap = 1
    }
    file_ok = (!has_fp && !has_gap)
    if (file_ok) total_agree++
    else {
      total_differ++
      if (has_fp) total_fp++
      if (has_gap) total_gap++
    }

    if (file_ok) verdict = C_GREEN "MATCH" C_RESET
    else if (has_fp && has_gap) verdict = C_RED "MIXED" C_RESET
    else if (has_fp) verdict = C_RED "FALSE POSITIVE" C_RESET
    else verdict = C_YELLOW "GAP" C_RESET
    printf "\n  %s%s%s  %s\n", C_BOLD, path, C_RESET, verdict
    printf "  %-" LW "s  %-" MW "s  %-" MW "s\n", "line", "ts-rust", "tsc"
    printf "  %s  %s  %s\n", repeat("-", LW), repeat("-", MW), repeat("-", MW)

    for (a = 1; a <= nlines; a++) {
      ln = ordered[a]
      key = path SUBSEP ln
      both = (key in tsrust_lines) && (key in tsc_lines)

      n_left = (key in tsrust_lines) ? split(tsrust_lines[key], left_arr, "\x1f") : 0
      n_right = (key in tsc_lines) ? split(tsc_lines[key], right_arr, "\x1f") : 0
      rows = (n_left > n_right ? n_left : n_right); if (rows < 1) rows = 1

      for (r = 1; r <= rows; r++) {
        lcell = (r <= n_left) ? trunc(left_arr[r], MW) : C_DIM "\xe2\x80\x94" C_RESET
        rcell = (r <= n_right) ? trunc(right_arr[r], MW) : C_DIM "\xe2\x80\x94" C_RESET
        label = (r == 1) ? ln : ""
        lcolor = both ? C_GREEN : (label != "" ? C_YELLOW : "")
        printf "  %s%-" LW "s%s  %-" MW "s  %-" MW "s\n", lcolor, label, (lcolor != "" ? C_RESET : ""), lcell, rcell
      }

      if (!both) {
        if (!(key in tsc_lines))
          diag = "ts-rust flagged an error tsc did not"
        else
          diag = "tsc flagged an error ts-rust did not -- likely a detection gap"
        printf "  %-" LW "s  %s\xe2\x86\xb3 %s%s\n", "", C_CYAN, diag, C_RESET
      }
    }
  }

  print ""
  clean_or_skipped = total_files - total_agree - total_differ
  printf "%s%d%s of %d file(s) agree with tsc (errors-only, %d differ)\n", \
    (total_differ == 0 ? C_GREEN : (total_fp == 0 ? C_YELLOW : C_RED)), total_agree, C_RESET, total_files, total_differ
  printf "  %d with a gap (tsc reports an error ts-rust does not -- a check not built yet)\n", total_gap
  printf "  %d with a false positive (ts-rust reports an error tsc does not)\n", total_fp
  print C_DIM "Note: divergence here is often intentional -- see the module doc comment" C_RESET
  print C_DIM "in bin/compare-tsc.rs for which fixtures are meant to diverge from tsc." C_RESET
}
' "${RECORDS}"

echo
echo "Raw output kept in ${RESULTS_DIR} (overwritten on next run)."
