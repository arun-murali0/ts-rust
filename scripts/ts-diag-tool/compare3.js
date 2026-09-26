#!/usr/bin/env node
// Three-way comparison: tsc (via compiler API), tsgo (native Go port, CLI
// only -- see below), and ts-rust. Extends compare.js's two-way idea (see
// that file's own doc comment) to all three compilers at once, and adds
// what compare.js does not attempt: wall-clock time and peak memory per
// compiler, plus a self-contained HTML report you can open in a browser.
//
// Why tsgo is handled differently from tsc: as of the `typescript` npm
// package's v7 release, it ships the native/Go-ported compiler ("tsgo")
// instead of the classic JS one, and that package no longer exposes
// ts.createProgram/ts.Diagnostics at all (see check-fixtures.js's own doc
// comment for how we found this out) -- there is currently no in-process
// API for it. So tsgo is driven as a CLI subprocess, one per file (same
// reason check-fixtures.js gives one ts.Program per file for tsc: fixtures
// across a suite commonly reuse top-level names like `Point`/`Animal`, and
// TypeScript files with no imports/exports share a global scope, so
// checking them together would produce spurious duplicate-identifier
// errors that have nothing to do with real type checking).
//
// tsgo's own CLI diagnostic format (confirmed by hand) is the classic
// non---pretty tsc shape: `file(line,col): error TSxxxx: message` -- NOT
// the `file:line:col:` shape ts-rust uses, so it gets its own regex below.
//
// Memory measurement uses `/usr/bin/time -v` (GNU time, reports "Maximum
// resident set size") when available, and silently omits memory figures
// otherwise -- this is a Linux-only tool and not guaranteed to exist on
// every machine this script runs on, so its absence is a soft degrade, not
// an error.
//
// Usage:
//   node compare3.js <target-dir-or-file> <ts-rust-binary> <tsgo-binary> \
//     [--strict-only] [--json] [--html <path>] [--only-differ]
//
// tsgo-binary is typically node_modules/.bin/tsc from the `typescript@7`
// (or later) package installed in a scratch project -- see this repo's
// README for the exact install steps, since `typescript` alone now
// resolves to different things depending on which major version npm picks.

const fs = require("fs");
const path = require("path");
const { execFileSync, spawnSync } = require("child_process");
const { checkFiles, collectTsFiles, baseOptions, strictExtras } = require("./check-fixtures");
const Table = require("cli-table3");

const args = process.argv.slice(2);
const positional = args.filter((a) => !a.startsWith("--"));
const strictOnly = args.includes("--strict-only");
const asJson = args.includes("--json");
const onlyDiffer = args.includes("--only-differ");
const htmlIdx = args.indexOf("--html");
const htmlPath = htmlIdx !== -1 ? args[htmlIdx + 1] : null;

const [target, tsRustBin, tsgoBin] = positional;
if (!target || !tsRustBin || !tsgoBin) {
  console.error(
    "usage: node compare3.js <target-dir-or-file> <ts-rust-binary> <tsgo-binary> " +
      "[--strict-only] [--json] [--html <path>] [--only-differ]",
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

function status(msg) {
  console.error(`${c.dim}${msg}${c.reset}`);
}

// === Memory-aware process runner ============================================

let timeCmdChecked = false;
let timeCmdAvailable = false;
function hasGnuTime() {
  if (!timeCmdChecked) {
    timeCmdChecked = true;
    const probe = spawnSync("/usr/bin/time", ["-v", "true"]);
    timeCmdAvailable = probe.status === 0 || (probe.stderr || "").includes("Maximum resident");
  }
  return timeCmdAvailable;
}

// Runs `cmd args...`, returns { stdout, exitCode, elapsedMs, peakRssKb }.
// peakRssKb is null when GNU time isn't installed -- callers must handle
// that, not assume it's always a number.
function runMeasured(cmd, cmdArgs) {
  const start = Date.now();
  let result;
  if (hasGnuTime()) {
    result = spawnSync("/usr/bin/time", ["-v", cmd, ...cmdArgs], { encoding: "utf8" });
  } else {
    result = spawnSync(cmd, cmdArgs, { encoding: "utf8" });
  }
  const elapsedMs = Date.now() - start;

  let peakRssKb = null;
  if (hasGnuTime() && result.stderr) {
    const m = /Maximum resident set size \(kbytes\): (\d+)/.exec(result.stderr);
    if (m) peakRssKb = Number(m[1]);
  }

  return {
    stdout: result.stdout || "",
    // GNU time's own stderr (the -v report) is mixed into result.stderr along
    // with the wrapped command's real stderr; the wrapped command's own
    // stdout is unaffected, which is all the diagnostic parsers below read.
    exitCode: result.status,
    elapsedMs,
    peakRssKb,
  };
}

if (!hasGnuTime()) {
  status("Note: GNU time (/usr/bin/time -v) not found -- memory figures will be omitted.");
}

// === Run ts-rust (one process, whole target) =================================

status(`[1/3] Running ts-rust over ${target} ...`);

const stat = fs.statSync(target);
const scratchDir = fs.mkdtempSync("/tmp/ts-rust-compare3-");
let tsRustProjectRoot;
if (stat.isFile()) {
  fs.mkdirSync(scratchDir, { recursive: true });
  fs.copyFileSync(target, path.join(scratchDir, path.basename(target)));
  tsRustProjectRoot = scratchDir;
} else {
  tsRustProjectRoot = target;
}
const tsconfigPath = path.join(tsRustProjectRoot, ".compare-tsconfig.json");
fs.writeFileSync(tsconfigPath, "");

const tsRustRun = runMeasured(tsRustBin, ["--project", tsconfigPath]);
fs.rmSync(tsconfigPath, { force: true });
fs.rmSync(scratchDir, { recursive: true, force: true });

const TS_RUST_LINE = /^(.+):(\d+):(\d+): (error|warning): (\S+) (.+)$/;
const tsRustDiags = []; // { file, line, code, message }
for (const rawLine of tsRustRun.stdout.split("\n")) {
  const m = TS_RUST_LINE.exec(rawLine);
  if (!m) continue;
  const [, file, line, , severity, code, message] = m;
  if (severity !== "error") continue;
  tsRustDiags.push({ file: stat.isFile() ? target : file, line: Number(line), code, message });
}

status(
  `      ts-rust: ${tsRustDiags.length} error(s), ${tsRustRun.elapsedMs}ms` +
    (tsRustRun.peakRssKb != null ? `, peak RSS ${(tsRustRun.peakRssKb / 1024).toFixed(1)} MB` : ""),
);

// === Run tsc (in-process via compiler API, file by file) ====================

const totalFilesGuess = collectTsFiles(target).length;
status(`[2/3] Running tsc over ${totalFilesGuess} file(s) via the compiler API ...`);

const tscStart = Date.now();
const memBefore = process.memoryUsage().rss;
const { files, results: tscAll } = checkFiles(target, { strictOnly });
const tscElapsedMs = Date.now() - tscStart;
const memAfterKb = Math.round(process.memoryUsage().rss / 1024);
// This is this Node process's own peak RSS, which also includes the tool's
// own overhead (V8, cli-table3, etc.) -- not a clean isolated tsc number the
// way ts-rust's and tsgo's subprocess measurements are. Reported anyway,
// clearly labeled, since it's the only number available without shelling
// out to `tsc` as a subprocess per file (which would reintroduce the exact
// text-parsing problem this whole toolset was built to avoid).
const tscDiagsAll = tscAll.filter((d) => d.category === "Error");
status(`      tsc: ${tscDiagsAll.length} error(s), ${tscElapsedMs}ms, in-process RSS ~${(memAfterKb / 1024).toFixed(1)} MB (see caveat in HTML/JSON output)`);

// === Run tsgo (CLI subprocess, one per file -- see module doc comment) ======

status(`[3/3] Running tsgo over ${files.length} file(s) (subprocess per file) ...`);

const compilerOptions = strictOnly ? baseOptions : { ...baseOptions, ...strictExtras };
const tsgoFlags = ["--noEmit", "--pretty", "false", "--skipLibCheck"];
if (compilerOptions.strict) tsgoFlags.push("--strict");
if (compilerOptions.allowUnreachableCode === false) tsgoFlags.push("--allowUnreachableCode", "false");
if (compilerOptions.noUncheckedIndexedAccess) tsgoFlags.push("--noUncheckedIndexedAccess");
if (compilerOptions.noImplicitOverride) tsgoFlags.push("--noImplicitOverride");
if (compilerOptions.exactOptionalPropertyTypes) tsgoFlags.push("--exactOptionalPropertyTypes");
if (compilerOptions.noPropertyAccessFromIndexSignature) tsgoFlags.push("--noPropertyAccessFromIndexSignature");
if (compilerOptions.noFallthroughCasesInSwitch) tsgoFlags.push("--noFallthroughCasesInSwitch");
if (compilerOptions.noImplicitReturns) tsgoFlags.push("--noImplicitReturns");

// tsgo's own CLI line shape (confirmed by hand): `file(line,col): error TSxxxx: message`
const TSGO_LINE = /^(.+)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$/;

const tsgoDiagsByFile = new Map(); // file -> [{ line, code, message }]
const tsgoTimings = []; // per-file elapsedMs
const tsgoRssSamples = []; // per-file peakRssKb, only non-null entries

files.forEach((fsPath, index) => {
  const file = stat.isFile() ? target : path.relative(process.cwd(), fsPath);
  const run = runMeasured(tsgoBin, [...tsgoFlags, fsPath]);
  tsgoTimings.push(run.elapsedMs);
  if (run.peakRssKb != null) tsgoRssSamples.push(run.peakRssKb);

  const diags = [];
  for (const rawLine of run.stdout.split("\n")) {
    const m = TSGO_LINE.exec(rawLine);
    if (!m) continue;
    const [, , line, , severity, code, message] = m;
    if (severity !== "error") continue;
    diags.push({ line: Number(line), code, message });
  }
  tsgoDiagsByFile.set(file, diags);

  if ((index + 1) % 25 === 0 || index === files.length - 1) {
    status(`      ... ${index + 1}/${files.length} files checked by tsgo`);
  }
});

const tsgoTotalMs = tsgoTimings.reduce((a, b) => a + b, 0);
const tsgoPeakRssKb = tsgoRssSamples.length ? Math.max(...tsgoRssSamples) : null;
const tsgoDiagCount = [...tsgoDiagsByFile.values()].reduce((a, d) => a + d.length, 0);
status(
  `      tsgo: ${tsgoDiagCount} error(s), ${tsgoTotalMs}ms total (subprocess sum)` +
    (tsgoPeakRssKb != null ? `, peak RSS ${(tsgoPeakRssKb / 1024).toFixed(1)} MB (max across files)` : ""),
);

// === Merge three-way, per file, per line =====================================

function byLine(diags) {
  const m = new Map();
  for (const d of diags) {
    if (!m.has(d.line)) m.set(d.line, []);
    m.get(d.line).push(d);
  }
  return m;
}

const tsRustByFile = new Map();
for (const d of tsRustDiags) {
  if (!tsRustByFile.has(d.file)) tsRustByFile.set(d.file, []);
  tsRustByFile.get(d.file).push(d);
}
for (const [f, diags] of tsRustByFile) tsRustByFile.set(f, byLine(diags));

const tscByFile = new Map();
for (const d of tscDiagsAll) {
  const f = d.file;
  if (!tscByFile.has(f)) tscByFile.set(f, []);
  tscByFile.get(f).push({ line: d.line, code: `TS${d.code}`, message: d.message });
}
for (const [f, diags] of tscByFile) tscByFile.set(f, byLine(diags));

const tsgoByFileLines = new Map();
for (const [f, diags] of tsgoDiagsByFile) tsgoByFileLines.set(f, byLine(diags));

// Per-file, per-line three-way presence, then rolled up into pairwise and
// all-three agreement counts.
let filesAllAgree = 0,
  filesAnyDiverge = 0;
const pair = {
  tsRustVsTsc: { agree: 0, tsRustOnly: 0, tscOnly: 0 },
  tsRustVsTsgo: { agree: 0, tsRustOnly: 0, tsgoOnly: 0 },
  tscVsTsgo: { agree: 0, tscOnly: 0, tsgoOnly: 0 },
};
let linesAllThreeAgree = 0,
  linesPartial = 0;

const fileReports = [];

for (const fsPath of files) {
  const file = stat.isFile() ? target : path.relative(process.cwd(), fsPath);
  const a = tsRustByFile.get(file) || new Map();
  const b = tscByFile.get(file) || new Map();
  const g = tsgoByFileLines.get(file) || new Map();

  const allLines = new Set([...a.keys(), ...b.keys(), ...g.keys()]);
  if (allLines.size === 0) {
    filesAllAgree++;
    continue;
  }

  let fileDiverges = false;
  const lineReports = [];
  for (const ln of [...allLines].sort((x, y) => x - y)) {
    const hasA = a.has(ln),
      hasB = b.has(ln),
      hasG = g.has(ln);

    if (hasA && hasB) pair.tsRustVsTsc.agree++;
    else if (hasA) pair.tsRustVsTsc.tsRustOnly++;
    else if (hasB) pair.tsRustVsTsc.tscOnly++;

    if (hasA && hasG) pair.tsRustVsTsgo.agree++;
    else if (hasA) pair.tsRustVsTsgo.tsRustOnly++;
    else if (hasG) pair.tsRustVsTsgo.tsgoOnly++;

    if (hasB && hasG) pair.tscVsTsgo.agree++;
    else if (hasB) pair.tscVsTsgo.tscOnly++;
    else if (hasG) pair.tscVsTsgo.tsgoOnly++;

    if (hasA && hasB && hasG) linesAllThreeAgree++;
    else {
      linesPartial++;
      fileDiverges = true;
    }

    lineReports.push({
      line: ln,
      tsRust: (a.get(ln) || []).map((d) => `${d.code} ${d.message}`),
      tsc: (b.get(ln) || []).map((d) => `${d.code} ${d.message}`),
      tsgo: (g.get(ln) || []).map((d) => `${d.code} ${d.message}`),
    });
  }

  if (fileDiverges) filesAnyDiverge++;
  else filesAllAgree++;

  if (!onlyDiffer || fileDiverges) {
    fileReports.push({ file, allAgree: !fileDiverges, lines: lineReports });
  }
}

// === Output ===================================================================

const summary = {
  totalFiles: files.length,
  filesAllAgree,
  filesAnyDiverge,
  linesAllThreeAgree,
  linesPartial,
  pairwise: pair,
  performance: {
    tsRust: { totalMs: tsRustRun.elapsedMs, peakRssKb: tsRustRun.peakRssKb, model: "single process, whole target" },
    tsc: {
      totalMs: tscElapsedMs,
      inProcessRssKb: memAfterKb,
      model: "in-process compiler API, one Program per file -- RSS includes this tool's own overhead, not isolated",
    },
    tsgo: {
      totalMs: tsgoTotalMs,
      peakRssKb: tsgoPeakRssKb,
      model: "one subprocess per file -- totalMs is the sum, peakRssKb is the max across files",
    },
  },
  errorCounts: { tsRust: tsRustDiags.length, tsc: tscDiagsAll.length, tsgo: tsgoDiagCount },
  strictOnly,
  onlyDiffer,
};

if (asJson) {
  console.log(JSON.stringify({ summary, files: fileReports }, null, 2));
} else {
  console.log("");
  console.log(`${c.bold}== Three-way summary: ts-rust vs tsc vs tsgo ==${c.reset}`);
  console.log("");

  const statsTable = new Table({
    head: ["compiler", "errors", "total time", "peak memory", "notes"],
    style: { head: [], border: NO_COLOR ? [] : ["grey"] },
    wordWrap: true,
    colWidths: [12, 10, 14, 16, 46],
  });
  statsTable.push(
    [
      "ts-rust",
      String(summary.errorCounts.tsRust),
      `${tsRustRun.elapsedMs}ms`,
      tsRustRun.peakRssKb != null ? `${(tsRustRun.peakRssKb / 1024).toFixed(1)} MB` : c.dim + "n/a" + c.reset,
      "single process, whole target",
    ],
    [
      "tsc",
      String(summary.errorCounts.tsc),
      `${tscElapsedMs}ms`,
      `~${(memAfterKb / 1024).toFixed(1)} MB`,
      "in-process API -- RSS includes this tool's own overhead, not isolated",
    ],
    [
      "tsgo",
      String(summary.errorCounts.tsgo),
      `${tsgoTotalMs}ms (sum)`,
      tsgoPeakRssKb != null ? `${(tsgoPeakRssKb / 1024).toFixed(1)} MB (max)` : c.dim + "n/a" + c.reset,
      "one subprocess per file -- no in-process API available",
    ],
  );
  console.log(statsTable.toString());

  console.log("");
  const agreeTable = new Table({
    head: ["pair", "agree", "left-only", "right-only"],
    style: { head: [], border: NO_COLOR ? [] : ["grey"] },
    colWidths: [22, 10, 16, 16],
  });
  agreeTable.push(
    ["ts-rust vs tsc", String(pair.tsRustVsTsc.agree), String(pair.tsRustVsTsc.tsRustOnly), String(pair.tsRustVsTsc.tscOnly)],
    ["ts-rust vs tsgo", String(pair.tsRustVsTsgo.agree), String(pair.tsRustVsTsgo.tsRustOnly), String(pair.tsRustVsTsgo.tsgoOnly)],
    ["tsc vs tsgo", String(pair.tscVsTsgo.agree), String(pair.tscVsTsgo.tscOnly), String(pair.tscVsTsgo.tsgoOnly)],
  );
  console.log(agreeTable.toString());

  console.log("");
  console.log(
    `${c.bold}${filesAllAgree}${c.reset} of ${files.length} file(s) have all three compilers fully agreeing ` +
      `(${filesAnyDiverge} diverge on at least one line)`,
  );
  console.log(`Line-level: ${linesAllThreeAgree} line(s) all three agree on, ${linesPartial} line(s) with a split.`);
}

// === HTML report ==============================================================

if (htmlPath) {
  const esc = (s) => String(s).replace(/[&<>"']/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[ch]));

  const fileRowsHtml = fileReports
    .map((f) => {
      const lineRows = f.lines
        .map(
          (l) => `
        <tr class="${l.tsRust.length && l.tsc.length && l.tsgo.length ? "agree" : "split"}">
          <td class="line">${l.line}</td>
          <td>${l.tsRust.map(esc).join("<br>") || "<span class=dim>—</span>"}</td>
          <td>${l.tsc.map(esc).join("<br>") || "<span class=dim>—</span>"}</td>
          <td>${l.tsgo.map(esc).join("<br>") || "<span class=dim>—</span>"}</td>
        </tr>`,
        )
        .join("");
      return `
      <details ${f.allAgree ? "" : "open"} class="${f.allAgree ? "file-agree" : "file-split"}">
        <summary>${esc(f.file)} <span class="badge">${f.allAgree ? "ALL AGREE" : "DIVERGES"}</span></summary>
        <table class="lines">
          <thead><tr><th>line</th><th>ts-rust</th><th>tsc</th><th>tsgo</th></tr></thead>
          <tbody>${lineRows}</tbody>
        </table>
      </details>`;
    })
    .join("");

  const html = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>ts-rust vs tsc vs tsgo -- comparison report</title>
<style>
  :root { color-scheme: dark; }
  body { background:#0d1117; color:#c9d1d9; font-family: ui-monospace, "SF Mono", Consolas, monospace; margin: 2rem; }
  h1 { font-size: 1.3rem; }
  .subtitle { color:#8b949e; margin-bottom: 2rem; }
  .stats-grid { display:grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; margin-bottom: 2rem; }
  .stat-card { background:#161b22; border:1px solid #30363d; border-radius:8px; padding:1rem; }
  .stat-card h3 { margin:0 0 .5rem 0; font-size:.95rem; color:#58a6ff; }
  .stat-card .big { font-size:1.6rem; font-weight:bold; }
  .stat-card .row { display:flex; justify-content:space-between; font-size:.85rem; color:#8b949e; margin-top:.25rem; }
  .bar-track { background:#21262d; border-radius:4px; height:8px; margin-top:.5rem; overflow:hidden; }
  .bar-fill { height:100%; background:#58a6ff; }
  table { border-collapse: collapse; width:100%; margin-bottom:1rem; }
  th, td { border:1px solid #30363d; padding:.4rem .6rem; text-align:left; font-size:.85rem; vertical-align:top; }
  th { background:#161b22; color:#58a6ff; }
  tr.agree td { color:#8b949e; }
  tr.split td { color:#f0d264; }
  .dim { color:#484f58; }
  details { background:#0d1117; border:1px solid #30363d; border-radius:8px; margin-bottom:.6rem; padding:.5rem .8rem; }
  details.file-split { border-color:#f0883e; }
  summary { cursor:pointer; font-weight:bold; }
  .badge { font-size:.7rem; padding:.1rem .5rem; border-radius:4px; margin-left:.5rem; }
  .file-agree .badge { background:#238636; color:#fff; }
  .file-split .badge { background:#f0883e; color:#000; }
  table.lines { margin-top:.6rem; }
  footer { color:#484f58; font-size:.75rem; margin-top:2rem; }
</style>
</head>
<body>
  <h1>ts-rust vs tsc vs tsgo</h1>
  <div class="subtitle">Generated ${esc(new Date().toISOString())} -- target: ${esc(target)}${strictOnly ? " -- --strict only" : " -- --strict + extras"}</div>

  <div class="stats-grid">
    <div class="stat-card">
      <h3>ts-rust</h3>
      <div class="big">${summary.errorCounts.tsRust} error(s)</div>
      <div class="row"><span>time</span><span>${tsRustRun.elapsedMs}ms</span></div>
      <div class="row"><span>peak memory</span><span>${tsRustRun.peakRssKb != null ? (tsRustRun.peakRssKb / 1024).toFixed(1) + " MB" : "n/a"}</span></div>
    </div>
    <div class="stat-card">
      <h3>tsc (compiler API)</h3>
      <div class="big">${summary.errorCounts.tsc} error(s)</div>
      <div class="row"><span>time</span><span>${tscElapsedMs}ms</span></div>
      <div class="row"><span>in-process RSS</span><span>~${(memAfterKb / 1024).toFixed(1)} MB*</span></div>
    </div>
    <div class="stat-card">
      <h3>tsgo (subprocess)</h3>
      <div class="big">${summary.errorCounts.tsgo} error(s)</div>
      <div class="row"><span>time (sum)</span><span>${tsgoTotalMs}ms</span></div>
      <div class="row"><span>peak memory (max)</span><span>${tsgoPeakRssKb != null ? (tsgoPeakRssKb / 1024).toFixed(1) + " MB" : "n/a"}</span></div>
    </div>
  </div>
  <p style="color:#8b949e;font-size:.8rem;">* tsc's memory figure includes this comparison tool's own process overhead (V8, etc.) since there's no isolated subprocess for it -- see compare3.js's module doc comment. ts-rust's and tsgo's figures are real per-process peak RSS.</p>

  <h2>Pairwise line agreement</h2>
  <table>
    <thead><tr><th>pair</th><th>agree</th><th>left-only</th><th>right-only</th></tr></thead>
    <tbody>
      <tr><td>ts-rust vs tsc</td><td>${pair.tsRustVsTsc.agree}</td><td>${pair.tsRustVsTsc.tsRustOnly}</td><td>${pair.tsRustVsTsc.tscOnly}</td></tr>
      <tr><td>ts-rust vs tsgo</td><td>${pair.tsRustVsTsgo.agree}</td><td>${pair.tsRustVsTsgo.tsRustOnly}</td><td>${pair.tsRustVsTsgo.tsgoOnly}</td></tr>
      <tr><td>tsc vs tsgo</td><td>${pair.tscVsTsgo.agree}</td><td>${pair.tscVsTsgo.tscOnly}</td><td>${pair.tscVsTsgo.tsgoOnly}</td></tr>
    </tbody>
  </table>

  <h2>Files (${fileReports.length} shown${onlyDiffer ? ", --only-differ" : ""} of ${files.length} total, ${filesAllAgree} fully agree)</h2>
  ${fileRowsHtml || "<p class=dim>No files to show (try without --only-differ).</p>"}

  <footer>Diagnostic agreement is a regression signal, not a proof of semantic equivalence. See bin/compare-tsc.rs's module doc comment for this project's flag-set policy.</footer>
</body>
</html>`;

  fs.writeFileSync(htmlPath, html);
  status(`\nHTML report written to ${htmlPath}`);
}
