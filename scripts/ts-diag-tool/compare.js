#!/usr/bin/env node
// The comparison/rendering engine behind scripts/compare-local.sh.
//
// Replaces the old awk-based text-parsing approach for the tsc side: tsc
// diagnostics come from check-fixtures.js's `checkFiles`, which reads real
// ts.Diagnostic objects (code, category, message) via the compiler API --
// no regex against pretty-printed CLI text, no risk of a type name
// containing a paren or colon confusing a line parser.
//
// The ts-rust side is still parsed from its CLI text output, since that
// format is small, fully controlled by this project, and already
// unambiguous: `file:line:col: severity: CODE message` (see
// Diagnostic::format_with_position in src/diagnostics.rs).
//
// Output is staged, not buffered: ts-rust runs first (one pass, printed as
// it happens), then tsc checks files one at a time via checkFiles'
// options.onFile callback, and each file's comparison prints immediately as
// it's computed -- so on a large fixture set you see results appear as they
// land, not all at once at the very end. --json still buffers into one
// object, since a partial/streamed JSON document isn't useful.
//
// Usage: node compare.js <target-dir-or-file> <ts-rust-binary> [--strict-only] [--json] [--only-differ]

const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");
const { checkFiles, collectTsFiles } = require("./check-fixtures");
const Table = require("cli-table3");

const args = process.argv.slice(2);
const positional = args.filter((a) => !a.startsWith("--"));
const strictOnly = args.includes("--strict-only");
const asJson = args.includes("--json");
const onlyDiffer = args.includes("--only-differ");

const [target, tsRustBin] = positional;
if (!target || !tsRustBin) {
  console.error(
    "usage: node compare.js <target-dir-or-file> <ts-rust-binary> [--strict-only] [--json]",
  );
  process.exit(2);
}

const NO_COLOR = !!process.env.NO_COLOR || !process.stdout.isTTY;
const c = NO_COLOR
  ? { reset: "", bold: "", dim: "", green: "", red: "", yellow: "", cyan: "" }
  : {
      reset: "\x1b[0m",
      bold: "\x1b[1m",
      dim: "\x1b[2m",
      green: "\x1b[32m",
      red: "\x1b[31m",
      yellow: "\x1b[33m",
      cyan: "\x1b[36m",
    };

function log(msg = "") {
  if (!asJson) console.log(msg);
}
function status(msg) {
  // Progress/status lines go to stderr so `--json` stdout stays clean and
  // `--json > file.json` never picks up a stray non-JSON line.
  console.error(`${c.dim}${msg}${c.reset}`);
}

// === Stage 1: run ts-rust once over the whole target ========================

status(`[1/2] Running ts-rust over ${target} ...`);

const stat = fs.statSync(target);
const scratchDir = fs.mkdtempSync("/tmp/ts-rust-compare-");
let tsRustProjectRoot;

if (stat.isFile()) {
  // Isolate exactly this one file, so ts-rust's own directory walk never
  // silently widens scope to sibling fixtures.
  fs.mkdirSync(scratchDir, { recursive: true });
  fs.copyFileSync(target, path.join(scratchDir, path.basename(target)));
  tsRustProjectRoot = scratchDir;
} else {
  tsRustProjectRoot = target;
}
const tsconfigPath = path.join(tsRustProjectRoot, ".compare-tsconfig.json");
fs.writeFileSync(tsconfigPath, "");

let tsRustRaw = "";
try {
  tsRustRaw = execFileSync(tsRustBin, ["--project", tsconfigPath], {
    encoding: "utf8",
  });
} catch (err) {
  // ts-rust exits 1 when it found real type errors -- expected, and its
  // diagnostics are still on stdout, so only bail if stdout never came back.
  tsRustRaw = err.stdout != null ? err.stdout : "";
  if (!tsRustRaw && err.status === undefined) {
    console.error(
      `error: failed to run ts-rust binary at ${tsRustBin}: ${err.message}`,
    );
    process.exit(2);
  }
}
fs.rmSync(tsconfigPath, { force: true });
fs.rmSync(scratchDir, { recursive: true, force: true });

// ts-rust's own CLI line shape: `file:line:col: severity: CODE message`.
// Only error-severity lines are kept -- ts-rust's warnings are all
// "not yet checked" implementation-status markers with no tsc equivalent
// (see bin/compare-tsc.rs's module doc comment), so comparing them is noise.
const TS_RUST_LINE = /^(.+):(\d+):(\d+): (error|warning): (\S+) (.+)$/;

const tsRustDiags = []; // { file, line, code, message }
for (const rawLine of tsRustRaw.split("\n")) {
  const m = TS_RUST_LINE.exec(rawLine);
  if (!m) continue;
  const [, file, line, , severity, code, message] = m;
  if (severity !== "error") continue;
  tsRustDiags.push({
    file: stat.isFile() ? target : file,
    line: Number(line),
    code,
    message,
  });
}

function byLine(diags) {
  const perLine = new Map();
  for (const d of diags) {
    if (!perLine.has(d.line)) perLine.set(d.line, []);
    perLine.get(d.line).push(d);
  }
  return perLine;
}

const tsRustByFile = new Map(); // file -> Map(line -> [entries])
for (const d of tsRustDiags) {
  if (!tsRustByFile.has(d.file)) tsRustByFile.set(d.file, []);
  tsRustByFile.get(d.file).push(d);
}
for (const [file, diags] of tsRustByFile) tsRustByFile.set(file, byLine(diags));

status(`      ts-rust reported ${tsRustDiags.length} error(s).`);

// === Stage 2: run tsc file-by-file, printing/merging as each one lands ======

const totalFilesGuess = collectTsFiles(target).length;
status(
  `[2/2] Running tsc over ${totalFilesGuess} file(s) (this is the slow part) ...`,
);

function tier(p) {
  const parts = p.split(path.sep);
  return parts.length >= 3 ? parts.slice(0, 3).join(path.sep) : "(root)";
}

// Display name inside a tier's table: the path relative to that tier's own
// header (which already names the shared folder), so e.g.
// "tests/fixtures/foundation/binary_op_mismatch.ts" becomes just
// "binary_op_mismatch.ts" in the table -- short enough to never need
// mid-word truncation, since cli-table3's wordWrap only breaks on spaces,
// not "/".
function displayName(file, t) {
  if (t === "(root)") return file;
  const rel = path.relative(t, file);
  return rel.startsWith("..") ? file : rel;
}

const report = []; // per-file structured verdicts, for --json
let totalAgree = 0,
  totalDiffer = 0,
  totalGap = 0,
  totalFp = 0;
let currentTier = null;
let tierTable = null;
let tierFileCount = 0;

// Column widths sized to the actual terminal, not a fixed guess -- so wide
// terminals get more room per column and narrow ones still wrap instead of
// spilling off-screen. wordWrap: true means cli-table3 reflows long text
// inside the cell rather than either truncating it or letting it overflow.
const termWidth = process.stdout.columns || 120;
// account for: 6 border/separator chars (| ... | ... | ... | ... | ... |)
const usable = Math.max(termWidth - 6, 60);
const FILE_W = Math.min(40, Math.floor(usable * 0.28));
const LINE_W = 6;
const VERDICT_W = 16;
const remaining = usable - FILE_W - LINE_W - VERDICT_W;
const TSRUST_W = Math.max(20, Math.floor(remaining / 2));
const TSC_W = Math.max(20, remaining - TSRUST_W);

// cli-table3's wordWrap only breaks on real whitespace (\s+), and fixture
// filenames are one long underscore_separated word with none -- so without
// help, a name longer than FILE_W gets truncated with an ellipsis instead of
// wrapping. Swapping underscores for spaces gives wordWrap real break
// points; the ".ts" is dropped since the tier header + row position already
// make clear these are fixture files.
function wrappable(name) {
  return name.replace(/\.ts$/, "").replace(/_/g, " ");
}

function newTierTable() {
  return new Table({
    head: ["file", "line", "ts-rust", "tsc", "verdict"],
    colWidths: [FILE_W, LINE_W, TSRUST_W, TSC_W, VERDICT_W],
    wordWrap: true,
    style: { head: [], border: NO_COLOR ? [] : ["grey"] },
  });
}

function flushTier() {
  if (tierTable && tierFileCount > 0) {
    log("");
    log(tierTable.toString());
  }
  tierTable = null;
  tierFileCount = 0;
}

// Tier header is printed lazily, right before the first row that actually
// gets added -- so under --only-differ, a tier with nothing but MATCHes
// never prints an empty "== foo ==" heading with no table under it.
let pendingTierHeader = null;
function ensureTierHeader() {
  if (pendingTierHeader !== null) {
    log("");
    log(pendingTierHeader);
    pendingTierHeader = null;
  }
}

function renderFile(file, tsRustLines, tscLines) {
  const t = tier(file);

  if (t !== currentTier) {
    if (!asJson) {
      flushTier();
      pendingTierHeader = `${c.bold}== ${t} ==${c.reset}`;
      tierTable = newTierTable();
    }
    currentTier = t;
  }

  if (tsRustLines.size === 0 && tscLines.size === 0) {
    totalAgree++;
    if (!asJson && !onlyDiffer) {
      ensureTierHeader();
      tierFileCount++;
      const clean = `${c.dim}did not produce any error or warning${c.reset}`;
      tierTable.push([
        wrappable(displayName(file, t)),
        "—",
        clean,
        clean,
        `${c.green}MATCH${c.reset}`,
      ]);
    }
    return;
  }

  const allLineNumbers = [
    ...new Set([...tsRustLines.keys(), ...tscLines.keys()]),
  ].sort((a, b) => a - b);

  let hasFp = false,
    hasGap = false;
  for (const ln of allLineNumbers) {
    const hasTsRust = tsRustLines.has(ln);
    const hasTsc = tscLines.has(ln);
    if (hasTsRust && !hasTsc) hasFp = true;
    if (hasTsc && !hasTsRust) hasGap = true;
  }

  const fileOk = !hasFp && !hasGap;
  if (fileOk) totalAgree++;
  else {
    totalDiffer++;
    if (hasFp) totalFp++;
    if (hasGap) totalGap++;
  }

  const verdict = fileOk
    ? "MATCH"
    : hasFp && hasGap
      ? "MIXED"
      : hasFp
        ? "FALSE POSITIVE"
        : "GAP";
  const verdictColor =
    verdict === "MATCH" ? c.green : verdict === "GAP" ? c.yellow : c.red;

  report.push({
    file,
    verdict,
    lines: allLineNumbers.map((ln) => ({
      line: ln,
      tsRust: (tsRustLines.get(ln) || []).map((d) => `${d.code} ${d.message}`),
      tsc: (tscLines.get(ln) || []).map((d) => `${d.code} ${d.message}`),
    })),
  });

  if (asJson) return;
  if (onlyDiffer && fileOk) return; // --only-differ: hide files that fully MATCH

  ensureTierHeader();
  tierFileCount++;
  const cellText = (d) =>
    d ? `${d.code} ${d.message.replace(/\s+/g, " ")}` : `${c.dim}—${c.reset}`;

  // Per-row status: every row gets one, not just the file's last row --
  // "match" for a line both sides agree on, "tsc extra (gap)" / "ts-rust
  // extra (fp)" for a line only one side reported. The file's overall
  // verdict (MATCH/GAP/FALSE POSITIVE/MIXED) still gets its own line
  // appended after the last row, since that summarizes the whole file, not
  // one line of it.
  const reasonFor = (hasTsRust, hasTsc) => {
    if (hasTsRust && hasTsc) return `${c.green}match${c.reset}`;
    if (!hasTsc) return `${c.yellow}ts-rust extra (fp)${c.reset}`;
    return `${c.yellow}tsc extra (gap)${c.reset}`;
  };

  let firstRow = true;
  for (const ln of allLineNumbers) {
    const left = tsRustLines.get(ln) || [];
    const right = tscLines.get(ln) || [];
    const rows = Math.max(left.length, right.length, 1);

    for (let r = 0; r < rows; r++) {
      const lcell = r < left.length ? cellText(left[r]) : cellText(null);
      const rcell = r < right.length ? cellText(right[r]) : cellText(null);
      tierTable.push([
        firstRow ? wrappable(displayName(file, t)) : "",
        String(ln),
        lcell,
        rcell,
        reasonFor(left.length > 0, right.length > 0),
      ]);
      firstRow = false;
    }
  }
  tierTable.push([
    "",
    "",
    "",
    { colSpan: 1, content: `${c.bold}file verdict:${c.reset}` },
    `${verdictColor}${verdict}${c.reset}`,
  ]);
}

const { files } = checkFiles(target, {
  strictOnly,
  onFile: (fsPath, fileResults, index, total) => {
    const file = stat.isFile() ? target : path.relative(process.cwd(), fsPath);
    const tscLines = byLine(
      fileResults
        .filter((d) => d.category === "Error")
        .map((d) => ({
          file,
          line: d.line,
          code: `TS${d.code}`,
          message: d.message,
        })),
    );
    const tsRustLines = tsRustByFile.get(file) || new Map();
    renderFile(file, tsRustLines, tscLines);
  },
});
flushTier();

// === Final summary ===========================================================

if (asJson) {
  console.log(
    JSON.stringify(
      {
        summary: {
          totalFiles: files.length,
          agree: totalAgree,
          differ: totalDiffer,
          gap: totalGap,
          falsePositive: totalFp,
          strictOnly,
        },
        files: report,
      },
      null,
      2,
    ),
  );
  process.exit(0);
}

log("");
const summaryColor =
  totalDiffer === 0 ? c.green : totalFp === 0 ? c.yellow : c.red;
log(
  `${summaryColor}${totalAgree}${c.reset} of ${files.length} file(s) agree with tsc ` +
    `(errors-only, ${totalDiffer} differ)` +
    (strictOnly ? " [--strict only]" : " [--strict + extras]"),
);
log(
  `  ${totalGap} with a gap (tsc reports an error ts-rust does not -- a check not built yet)`,
);
log(
  `  ${totalFp} with a false positive (ts-rust reports an error tsc does not)`,
);
log(
  `${c.dim}Note: divergence here is often intentional -- see the module doc comment${c.reset}`,
);
log(
  `${c.dim}in bin/compare-tsc.rs for which fixtures are meant to diverge from tsc.${c.reset}`,
);
