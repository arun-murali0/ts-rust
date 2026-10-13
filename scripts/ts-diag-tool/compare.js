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
// land, not all at once at the very end. Plain --json still buffers into
// one object at the very end, since a single valid JSON document is the
// point of that flag (something a downstream tool can JSON.parse() whole).
// --json-stream is for watching progress in JSON form instead: one
// complete JSON object per file, printed the moment that file is done
// (newline-delimited JSON / NDJSON -- see https://jsonlines.org), plus a
// final `{"summary": ...}` line once every file has been processed. Each
// line parses on its own; the whole stream does not parse as one JSON
// document, so don't pipe --json-stream output into something expecting a
// single JSON.parse().
//
// --differ shows the same side-by-side table, but only the lines where ts-rust and tsc
// are not an exact match: "tsc extra (gap)" (tsc only), "ts-rust extra (fp)" (ts-rust
// only), "code differs" or "message differs" (both report an error there, but the tsc
// code or the message text is not the same). It is the quick way to see exactly what is
// left to align; the summary still follows. In --json, those lines are the `differences`
// list: kind GAP / FALSE POSITIVE / MISMATCH (what check-baseline.js reads), plus
// `mismatch` ("message" | "code") on a MISMATCH.
//
// The ts-rust column shows ts-rust's own TSR#### code (e.g. `TSR1004 Type 'number' ...`).
// The comparison itself is on the tsc code it maps to, so the two sides can be matched; that
// mapping stays in --json: `tsRust` has the mapped TS#### strings, `tsRustTsr` the TSR codes.
//
// A file's verdict is MATCH only when every line has the same tsc code and message on both
// sides. Same error lines but different wording is MESSAGE DIFFERS; a different tsc code is
// CODE DIFFERS; GAP, FALSE POSITIVE and MIXED are as before.
//
// The summary ends with the wall time and peak memory of ts-rust and of tsc. tsc's time is
// stored in its result cache with each file, so a cached run still reports what tsc cost;
// pass --no-cache to measure it afresh.
//
// --module compares a whole project instead of independent files, which is how ts-rust now
// checks (through its module graph, imports resolved across files). The target must be a
// directory. ts-rust runs as before; tsc checks every file of the directory in ONE Program
// (check-fixtures.js checkProject), run as a child process so its wall time and peak memory
// are measured the same way ts-rust's are, instead of one isolated Program per file. The
// per-file tsc cache is not used, since a project result depends on every file in it. Every
// file is treated as its own module (moduleDetection: force), so independent scripts do not
// collide in one global scope; on tests/fixtures the diagnostics equal the per-file mode's.
//
// --runs N repeats the measured runs of ts-rust (and, with --module, of tsc) N times and
// reports the median time and median peak memory, so one noisy run does not decide the
// figures. The default is 5 with --module and 1 otherwise (per-file tsc results come from a
// cache, so repeating them would measure the cache). --threads N runs ts-rust with N worker
// threads (RAYON_NUM_THREADS); tsc is single-threaded, so --threads 1 is the like-for-like
// figure and the default (every core) is what a user gets.
//
// --flat runs ts-rust with its own --flat option: every file is checked on its own, with no
// module graph, which is how tsc's test suite works (each case is its own Program, and
// hundreds of cases declare the same global names). Without it ts-rust builds the module
// graph over the whole directory, and on a directory of thousands of unrelated scripts that
// run can take far longer than the same files in chunks. Use --flat for the TypeScript
// repo's tests/cases and leave it off for projects whose files import each other.
//
// --timeout S stops a ts-rust or tsc child that has run for S seconds and reports it, so
// one hang names itself instead of freezing the whole comparison. The default is no limit.
//
// Usage: node compare.js <target-dir-or-file> <ts-rust-binary> [--strict-only] [--json | --json-stream] [--only-differ] [--differ] [--no-cache] [--module] [--flat] [--runs N] [--threads N] [--timeout S]

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");
const ts = require("typescript");
const {
  checkFiles,
  collectTsFiles,
  collectProjectFiles,
  defaultCachePath,
} = require("./check-fixtures");
const Table = require("cli-table3");

const args = process.argv.slice(2);
// Flags that take a value: the value follows the flag and is not a target or a binary.
const VALUE_FLAGS = ["--runs", "--threads", "--timeout"];
const positional = args.filter(
  (a, i) => !a.startsWith("--") && !VALUE_FLAGS.includes(args[i - 1]),
);
function intFlag(name) {
  const i = args.indexOf(name);
  if (i === -1) return null;
  const value = Number(args[i + 1]);
  if (!Number.isInteger(value) || value < 1) {
    console.error(`error: ${name} needs a whole number of at least 1`);
    process.exit(2);
  }
  return value;
}
const strictOnly = args.includes("--strict-only");
const asJson = args.includes("--json");
const jsonStream = args.includes("--json-stream");
const onlyDiffer = args.includes("--only-differ");
const showDiffer = args.includes("--differ");
// tsc results are cached per fixture (see check-fixtures.js); --no-cache forces a full run.
const noCache = args.includes("--no-cache");
// Cross-file mode: tsc checks the directory as one project (see the doc comment above).
const moduleMode = args.includes("--module");
const runs = intFlag("--runs") ?? (moduleMode ? 5 : 1);
const threads = intFlag("--threads");
// ts-rust checks each file alone, with no module graph (see the doc comment above).
const flatMode = args.includes("--flat");
// Seconds before a child process is stopped; null means no limit.
const timeoutSeconds = intFlag("--timeout");

const [target, tsRustBin] = positional;
if (!target || !tsRustBin) {
  console.error(
    "usage: node compare.js <target-dir-or-file> <ts-rust-binary> [--strict-only] [--json | --json-stream] [--only-differ] [--differ] [--no-cache] [--module] [--flat] [--runs N] [--threads N] [--timeout S]",
  );
  process.exit(2);
}

function median(values) {
  const v = values.filter((x) => x !== null && x !== undefined);
  v.sort((a, b) => a - b);
  if (v.length === 0) return null;
  const mid = v.length >> 1;
  return v.length % 2 ? v[mid] : (v[mid - 1] + v[mid]) / 2;
}
if (asJson && jsonStream) {
  console.error(
    "error: --json and --json-stream are mutually exclusive -- pick one",
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
  if (!asJson && !jsonStream) console.log(msg);
}
function jsonLine(obj) {
  if (jsonStream) console.log(JSON.stringify(obj));
}
function status(msg) {
  // Progress/status lines go to stderr so `--json` stdout stays clean and
  // `--json > file.json` never picks up a stray non-JSON line.
  console.error(`${c.dim}${msg}${c.reset}`);
}

function fmtMs(ms) {
  if (ms < 10) return `${ms.toFixed(1)} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  const m = Math.floor(ms / 60000);
  return `${m}m ${((ms - m * 60000) / 1000).toFixed(1)}s`;
}
function fmtKb(kb) {
  return kb >= 1024 ? `${(kb / 1024).toFixed(1)} MB` : `${kb} KB`;
}

// === Stage 1: run ts-rust once over the whole target ========================

status(`[1/2] Running ts-rust over ${target} ...`);

const stat = fs.statSync(target);
if (moduleMode && stat.isFile()) {
  console.error(
    "error: --module compares a project, so the target must be a directory, not a single file",
  );
  process.exit(2);
}
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

// Runs ts-rust once and reports how long it took and, when the OS lets us ask, its peak
// memory. Node cannot read a child's peak RSS by itself, so `measure` wraps the command in
// /usr/bin/time (-v on Linux, -l on macOS) or, failing that, a few lines of python3. If
// neither exists the run is still made and memory is simply reported as unavailable.
const PY_WRAPPER = [
  "import resource,subprocess,sys,time",
  "t=time.perf_counter()",
  "r=subprocess.run(sys.argv[1:])",
  "w=(time.perf_counter()-t)*1000",
  "k=resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss",
  "k=k//1024 if sys.platform=='darwin' else k",
  "sys.stderr.write('\\n__MAXRSS_KB__ %d\\n__WALL_MS__ %f\\n'%(k,w))",
  "sys.exit(r.returncode)",
].join("\n");
let python3Ok = null;
function hasPython3() {
  if (python3Ok === null)
    python3Ok = spawnSync("python3", ["--version"]).status === 0;
  return python3Ok;
}

// Runs `cmd` (an argv array), optionally inside one of the measuring wrappers above.
function spawnMeasured(cmd, wrapper, env) {
  let file = cmd[0];
  let fileArgs = cmd.slice(1);
  if (wrapper === "time") {
    file = "/usr/bin/time";
    fileArgs = [process.platform === "darwin" ? "-l" : "-v", ...cmd];
  } else if (wrapper === "python") {
    file = "python3";
    fileArgs = ["-c", PY_WRAPPER, ...cmd];
  }
  const startedAt = process.hrtime.bigint();
  const res = spawnSync(file, fileArgs, {
    encoding: "utf8",
    maxBuffer: 1 << 30,
    env: { ...process.env, ...env },
    // spawnSync kills the child with SIGTERM when this passes; undefined means no limit.
    timeout: timeoutSeconds ? timeoutSeconds * 1000 : undefined,
  });
  res.outerMs = Number(process.hrtime.bigint() - startedAt) / 1e6;
  return res;
}

// `label` names the program in error messages. Both ts-rust and the tsc child exit 0, or 1
// when they found errors; anything else with no output is a real failure.
function runMeasured(cmd, { measure, label, env }) {
  let res;
  let maxRssKb = null;
  let wallMs = null;
  let how = null;
  const wrappers = [];
  if (measure && fs.existsSync("/usr/bin/time")) wrappers.push("time");
  if (measure && hasPython3()) wrappers.push("python");
  for (const wrapper of wrappers) {
    res = spawnMeasured(cmd, wrapper, env);
    const err = res.stderr || "";
    let m;
    if (wrapper === "time") {
      if ((m = /Maximum resident set size \(kbytes\): (\d+)/.exec(err)))
        maxRssKb = Number(m[1]);
      else if ((m = /(\d+)\s+maximum resident set size/.exec(err)))
        maxRssKb = Math.round(Number(m[1]) / 1024);
      wallMs = res.outerMs;
    } else {
      if ((m = /__MAXRSS_KB__ (\d+)/.exec(err))) maxRssKb = Number(m[1]);
      if ((m = /__WALL_MS__ ([\d.]+)/.exec(err))) wallMs = Number(m[1]);
    }
    if (maxRssKb !== null) {
      how = wrapper === "time" ? "/usr/bin/time" : "python3";
      break;
    }
  }
  if (maxRssKb === null) {
    res = spawnMeasured(cmd, null, env);
    wallMs = res.outerMs;
  }
  // A hang or a crash is told apart from "no output" so the report says what happened
  // instead of "exited with status null". The measuring wrapper, when there is one, is the
  // process that was stopped, which takes the program down with it.
  if (res.error && res.error.code === "ETIMEDOUT") {
    console.error(
      `error: ${label} did not finish within ${timeoutSeconds} s and was stopped. ` +
        `A hang like this usually comes from one file or from the files being checked together; ` +
        `try a smaller directory, or --flat to check each file on its own.`,
    );
    process.exit(2);
  }
  if (res.error) {
    console.error(
      `error: failed to run ${label} (${cmd[0]}): ${res.error.message}`,
    );
    process.exit(2);
  }
  if (res.signal) {
    console.error(
      `error: ${label} was killed by ${res.signal} (a stack overflow or an abort looks like this)`,
    );
    process.exit(2);
  }
  // ts-rust exits 1 when it found real type errors -- expected, and its
  // diagnostics are still on stdout. Anything else with no output is a real failure.
  if (!res.stdout && res.status !== 0 && res.status !== 1) {
    console.error(
      `error: ${label} exited with status ${res.status}: ${(res.stderr || "").trim().split("\n").slice(0, 3).join(" ")}`,
    );
    process.exit(2);
  }
  return { stdout: res.stdout || "", wallMs, maxRssKb, how };
}

function runTsRust(extraArgs, { measure }) {
  const flatArgs = flatMode ? ["--flat"] : [];
  return runMeasured(
    [tsRustBin, "--project", tsconfigPath, ...flatArgs, ...extraArgs],
    {
      measure,
      label: "the ts-rust binary",
      // rayon reads this; without it ts-rust uses every core.
      env: threads ? { RAYON_NUM_THREADS: String(threads) } : undefined,
    },
  );
}

// Same run twice, because ts-rust prints one code per diagnostic: its own TSR#### by default,
// or the tsc code it maps to with --tsc-codes. The comparison needs the tsc code; the report
// shows both, so a TSR code is never lost behind its mapping. Both runs list the same
// diagnostics in the same order (checked below).
const rawRun = runTsRust([], { measure: false });
// The measured runs. The first one's output is what is compared (every run prints the same
// diagnostics); time and peak memory are the median over all of them, written onto
// `mappedRun` so everything below reads one pair of figures.
const mappedRuns = [];
for (let i = 0; i < runs; i++)
  mappedRuns.push(runTsRust(["--tsc-codes"], { measure: true }));
const mappedRun = mappedRuns[0];
mappedRun.wallMs = median(mappedRuns.map((r) => r.wallMs));
mappedRun.maxRssKb = median(mappedRuns.map((r) => r.maxRssKb));
fs.rmSync(tsconfigPath, { force: true });
fs.rmSync(scratchDir, { recursive: true, force: true });

// Keys must match the tsc side, which uses paths relative to the cwd.
const normalizePath = (f) => path.relative(process.cwd(), path.resolve(f));

// ts-rust's own CLI line shape: `file:line:col: severity: CODE message`.
// Only error-severity lines are kept -- ts-rust's warnings are all
// "not yet checked" implementation-status markers with no tsc equivalent
// (see bin/compare-tsc.rs's module doc comment), so comparing them is noise.
const TS_RUST_LINE = /^(.+):(\d+):(\d+): (error|warning): (\S+) (.+)$/;

function parseTsRust(raw) {
  const diags = []; // { file, line, code, message }
  let lastDiag = null;
  for (const rawLine of raw.split("\n")) {
    const m = TS_RUST_LINE.exec(rawLine);
    if (!m) {
      // A message that is a chain of reasons continues on indented lines, the way
      // tsc prints it; they belong to the diagnostic above.
      if (lastDiag && /^ {2,}\S/.test(rawLine))
        lastDiag.message += "\n" + rawLine;
      else lastDiag = null;
      continue;
    }
    const [, file, line, , severity, code, message] = m;
    if (severity !== "error") {
      lastDiag = null;
      continue;
    }
    lastDiag = {
      file: stat.isFile() ? target : normalizePath(file),
      line: Number(line),
      code,
      message,
    };
    diags.push(lastDiag);
  }
  return diags;
}

const tsRustDiags = parseTsRust(mappedRun.stdout);
const tsRustTsrDiags = parseTsRust(rawRun.stdout);
let tsrCodesAvailable =
  tsRustTsrDiags.length === tsRustDiags.length &&
  tsRustDiags.every(
    (d, i) =>
      d.file === tsRustTsrDiags[i].file && d.line === tsRustTsrDiags[i].line,
  );
if (tsrCodesAvailable) {
  tsRustDiags.forEach((d, i) => {
    d.tsr = tsRustTsrDiags[i].code;
  });
} else {
  status(
    "      warning: ts-rust's own codes could not be paired with the tsc codes it maps to; showing tsc codes only.",
  );
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

// The unmeasured run above exists only for the TSR codes; its time is not used.
const tsRustWallMs = mappedRun.wallMs ?? rawRun.wallMs;
status(
  `      ts-rust reported ${tsRustDiags.length} error(s) in ${fmtMs(tsRustWallMs)}` +
    (mappedRun.maxRssKb !== null
      ? `, peak memory ${fmtKb(mappedRun.maxRssKb)}.`
      : " (peak memory unavailable: no /usr/bin/time or python3)."),
);

// === Stage 2: run tsc file-by-file, printing/merging as each one lands ======

const totalFilesGuess = (
  moduleMode ? collectProjectFiles(target) : collectTsFiles(target)
).length;
status(
  moduleMode
    ? `[2/2] Running tsc over ${totalFilesGuess} file(s) as one project ...`
    : `[2/2] Running tsc over ${totalFilesGuess} file(s) (this is the slow part) ...`,
);

// The group a file is listed under: its first three path parts, i.e. the folder. Looking at
// the folder, not the whole path, matters for a file that sits directly in a short folder
// (tests/fixtures/x.ts): the old version made the file itself its own group, and its name
// then came out empty in the table.
function tier(p) {
  const dir = path.dirname(p);
  if (dir === ".") return "(root)";
  const parts = dir.split(path.sep);
  return parts.slice(0, 3).join(path.sep);
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
  return rel === "" || rel.startsWith("..") ? file : rel;
}

const report = []; // per-file structured verdicts, for --json
const differences = []; // --differ: one entry per line where the two sides disagree
// Beyond "is there an error on this line": a line is `exact` when ts-rust and tsc
// report the same codes and the same message text on it, in the same order.
let exactFiles = 0,
  exactLines = 0,
  comparedLines = 0;
const lineKey = (d) => `${d.code} ${d.message}`;
const lineIsExact = (rust, tsc) =>
  rust.length === tsc.length &&
  rust.every((d, i) => lineKey(d) === lineKey(tsc[i]));

// exact | message (same codes, wording differs) | code (codes differ) | gap | fp
function lineResultOf(rust, tsc) {
  if (rust.length === 0) return "gap";
  if (tsc.length === 0) return "fp";
  if (lineIsExact(rust, tsc)) return "exact";
  const sameCodes =
    rust.length === tsc.length && rust.every((d, i) => d.code === tsc[i].code);
  return sameCodes ? "message" : "code";
}
const verdictCounts = {
  MATCH: 0,
  "MESSAGE DIFFERS": 0,
  "CODE DIFFERS": 0,
  GAP: 0,
  "FALSE POSITIVE": 0,
  MIXED: 0,
};
const VERDICT_COLOR = {
  MATCH: c.green,
  "MESSAGE DIFFERS": c.yellow,
  "CODE DIFFERS": c.red,
  GAP: c.yellow,
  "FALSE POSITIVE": c.red,
  MIXED: c.red,
};

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
const VERDICT_W = 18;
const remaining = usable - FILE_W - LINE_W - VERDICT_W;
const TSRUST_W = Math.max(20, Math.floor(remaining / 2));
const TSC_W = Math.max(20, remaining - TSRUST_W);

// The file name is shown whole -- real name, ".ts" and underscores included -- in a cell that
// wraps at the column edge instead of at a space. (cli-table3 otherwise only breaks on
// whitespace, so a long underscore_separated name was cut off with an ellipsis, and the old
// workaround of rewriting the name hid its real spelling.)
function fileCell(name) {
  return { content: name, wrapOnWordBoundary: false };
}

// ts-rust's cells show the TSR code it really emits, and only that. The tsc code it maps to
// is still what the comparison uses and is still in the JSON (`tsRust` holds the mapped
// "TS####" strings, `tsRustTsr` the TSR codes), so a viewer such as an IDE can later turn a
// TSR code into a link to its tsc code. tsc's own diagnostics have no TSR code.
function codeLabel(d) {
  return d.tsr || d.code;
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
    if (!asJson && !jsonStream) {
      flushTier();
      pendingTierHeader = `${c.bold}== ${t} ==${c.reset}`;
      tierTable = newTierTable();
    }
    currentTier = t;
  }

  if (tsRustLines.size === 0 && tscLines.size === 0) {
    totalAgree++;
    exactFiles++;
    verdictCounts.MATCH++;
    if (!onlyDiffer) jsonLine({ file, verdict: "MATCH", lines: [] });
    if (!asJson && !jsonStream && !onlyDiffer && !showDiffer) {
      ensureTierHeader();
      tierFileCount++;
      const clean = `${c.dim}did not produce any error or warning${c.reset}`;
      tierTable.push([
        fileCell(displayName(file, t)),
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

  // Per line: exact | message (same codes, wording differs) | code (different codes)
  // | gap (tsc only) | fp (ts-rust only).
  const lineResult = new Map();
  let hasFp = false,
    hasGap = false,
    hasCode = false,
    hasMessage = false;
  for (const ln of allLineNumbers) {
    const r = lineResultOf(tsRustLines.get(ln) || [], tscLines.get(ln) || []);
    lineResult.set(ln, r);
    comparedLines++;
    if (r === "exact") exactLines++;
    if (r === "fp") hasFp = true;
    if (r === "gap") hasGap = true;
    if (r === "code") hasCode = true;
    if (r === "message") hasMessage = true;
  }
  const fileExact = !hasFp && !hasGap && !hasCode && !hasMessage;
  if (fileExact) exactFiles++;

  if (showDiffer) {
    for (const ln of allLineNumbers) {
      const r = lineResult.get(ln);
      if (r === "exact") continue;
      const left = tsRustLines.get(ln) || [];
      const right = tscLines.get(ln) || [];
      // `kind` and the leading TS code of each string are what check-baseline.js keys on,
      // so they stay as they were; the extra fields are for people reading the JSON.
      differences.push({
        file,
        line: ln,
        kind: r === "gap" ? "GAP" : r === "fp" ? "FALSE POSITIVE" : "MISMATCH",
        ...(r === "message" || r === "code" ? { mismatch: r } : {}),
        tsc: right.map((d) => `${d.code} ${d.message}`),
        tsRust: left.map((d) => `${d.code} ${d.message}`),
        tsRustTsr: left.map((d) => d.tsr || null),
      });
    }
  }

  // "agree" keeps its old meaning (no line where only one side reports an error), so
  // anything reading the JSON summary sees the same numbers as before.
  const fileOk = !hasFp && !hasGap;
  if (fileOk) totalAgree++;
  else {
    totalDiffer++;
    if (hasFp) totalFp++;
    if (hasGap) totalGap++;
  }

  const verdict =
    hasFp && hasGap
      ? "MIXED"
      : hasFp
        ? "FALSE POSITIVE"
        : hasGap
          ? "GAP"
          : hasCode
            ? "CODE DIFFERS"
            : hasMessage
              ? "MESSAGE DIFFERS"
              : "MATCH";
  verdictCounts[verdict]++;
  const verdictColor = VERDICT_COLOR[verdict];

  // --only-differ applies to --json/--json-stream too: a file that matches exactly is left
  // out entirely, not just hidden from the table. A file whose lines all have errors on both
  // sides but with different wording is NOT exact, so it stays.
  if (!(onlyDiffer && fileExact)) {
    const fileReport = {
      file,
      verdict,
      lines: allLineNumbers.map((ln) => ({
        line: ln,
        exact: lineResult.get(ln) === "exact",
        result: lineResult.get(ln),
        tsRust: (tsRustLines.get(ln) || []).map(
          (d) => `${d.code} ${d.message}`,
        ),
        tsRustTsr: (tsRustLines.get(ln) || []).map((d) => d.tsr || null),
        tsc: (tscLines.get(ln) || []).map((d) => `${d.code} ${d.message}`),
      })),
    };
    report.push(fileReport);
    jsonLine(fileReport);
  }

  if (asJson || jsonStream) return;
  if ((onlyDiffer || showDiffer) && fileExact) return;

  ensureTierHeader();
  tierFileCount++;
  const cellText = (d) =>
    d
      ? `${codeLabel(d)} ${d.message.replace(/\s+/g, " ")}`
      : `${c.dim}—${c.reset}`;

  // Per-row status: every row gets one -- "match" when both sides agree exactly, "message
  // differs" / "code differs" when both report an error on the line but the text or the
  // code is not the same, and "tsc extra (gap)" / "ts-rust extra (fp)" for a line only one
  // side reported. The file's overall verdict still gets its own line after the last row,
  // since that summarizes the whole file, not one line of it.
  const reasonFor = (r) =>
    ({
      exact: `${c.green}match${c.reset}`,
      message: `${c.yellow}message differs${c.reset}`,
      code: `${c.red}code differs${c.reset}`,
      gap: `${c.yellow}tsc extra (gap)${c.reset}`,
      fp: `${c.red}ts-rust extra (fp)${c.reset}`,
    })[r];

  // --differ: same side-by-side table, but only the lines that are not an exact match.
  const shownLines = showDiffer
    ? allLineNumbers.filter((ln) => lineResult.get(ln) !== "exact")
    : allLineNumbers;

  let firstRow = true;
  for (const ln of shownLines) {
    const left = tsRustLines.get(ln) || [];
    const right = tscLines.get(ln) || [];
    const rows = Math.max(left.length, right.length, 1);

    for (let r = 0; r < rows; r++) {
      const lcell = r < left.length ? cellText(left[r]) : cellText(null);
      const rcell = r < right.length ? cellText(right[r]) : cellText(null);
      tierTable.push([
        firstRow ? fileCell(displayName(file, t)) : "",
        String(ln),
        lcell,
        rcell,
        reasonFor(lineResult.get(ln)),
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

// Merges one file's tsc diagnostics with ts-rust's and renders the result.
function onTscFile(fsPath, fileResults) {
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
}

// --module: tsc checks the directory as one Program in a child process, so its wall time
// and peak memory come from the same wrappers as ts-rust's. The time is the whole process
// (Node and TypeScript start-up included), the way ts-rust's includes its own start-up;
// `programMs` is the part spent building and checking the Program, shown next to it.
function runTscProject() {
  const cmd = [
    process.execPath,
    path.join(__dirname, "check-fixtures.js"),
    target,
    "--project",
    ...(strictOnly ? ["--strict-only"] : []),
  ];
  // Repeated like ts-rust's runs, and reported as the median, so the two sides are summarized
  // the same way. The first run's diagnostics are the ones compared.
  const results = [];
  for (let i = 0; i < runs; i++) {
    const run = runMeasured(cmd, {
      measure: true,
      label: "tsc (project mode)",
    });
    try {
      results.push({ run, out: JSON.parse(run.stdout) });
    } catch (error) {
      console.error(
        `error: could not read the tsc project result: ${error.message}`,
      );
      process.exit(2);
    }
  }
  const { out } = results[0];
  const byFile = new Map(out.files.map((f) => [f, []]));
  for (const d of out.results) byFile.get(d.file).push(d);
  out.files.forEach((f) => onTscFile(f, byFile.get(f)));
  return {
    files: out.files,
    // Fields the performance section reads; nothing came from a cache.
    stats: {
      cached: 0,
      ran: out.files.length,
      ranMs: median(results.map((r) => r.run.wallMs)),
      cachedMs: 0,
      cachedUntimed: 0,
    },
    perf: {
      maxRssKb: median(results.map((r) => r.run.maxRssKb)),
      measuredThisRun: true,
    },
    project: {
      programMs: median(results.map((r) => r.out.programMs)),
      outsideRoots: out.outsideRoots,
    },
  };
}

const { files, stats, perf, project } = moduleMode
  ? runTscProject()
  : {
      ...checkFiles(target, {
        strictOnly,
        cachePath: noCache ? undefined : defaultCachePath(),
        onFile: (fsPath, fileResults) => onTscFile(fsPath, fileResults),
      }),
      project: null,
    };
flushTier();
if (!noCache && !moduleMode)
  status(`      tsc: ${stats.cached} from cache, ${stats.ran} checked.`);

// === Time and memory =========================================================

const tscWallMs = stats.ranMs + stats.cachedMs;
const performance = {
  runs,
  tsRust: {
    files: files.length,
    threads: threads ?? os.cpus().length,
    wallMs: Math.round(tsRustWallMs * 10) / 10,
    maxRssKb: mappedRun.maxRssKb,
    memoryMeasuredWith: mappedRun.how,
  },
  tsc: {
    typescript: ts.version,
    mode: moduleMode ? "project" : "per-file",
    files: files.length,
    wallMs: Math.round(tscWallMs),
    ...(project ? { programMs: Math.round(project.programMs) } : {}),
    checkedThisRun: stats.ran,
    fromCache: stats.cached,
    fromCacheWithoutStoredTime: stats.cachedUntimed,
    maxRssKb: perf.maxRssKb,
    memoryMeasuredThisRun: perf.measuredThisRun,
  },
  timesFaster:
    tsRustWallMs > 0 && tscWallMs > 0
      ? Math.round(tscWallMs / tsRustWallMs)
      : null,
  timesLessMemory:
    mappedRun.maxRssKb && perf.maxRssKb
      ? Math.round((perf.maxRssKb / mappedRun.maxRssKb) * 10) / 10
      : null,
};

function printPerformance() {
  const p = performance;
  const rustMem = p.tsRust.maxRssKb !== null ? fmtKb(p.tsRust.maxRssKb) : "n/a";
  const tscMem =
    p.tsc.maxRssKb !== null
      ? fmtKb(p.tsc.maxRssKb) + (p.tsc.memoryMeasuredThisRun ? "" : " *")
      : "n/a";
  // A time of 0 would be a lie when the cached entries carry no timing.
  const tscTime =
    p.tsc.checkedThisRun === 0 && p.tsc.fromCacheWithoutStoredTime > 0
      ? "n/a"
      : fmtMs(p.tsc.wallMs);
  const rows = [
    ["", "time", "peak memory"],
    ["ts-rust", fmtMs(p.tsRust.wallMs), rustMem],
    ["tsc", tscTime, tscMem],
  ];
  const w = [0, 1, 2].map((i) => Math.max(...rows.map((r) => r[i].length)));
  log("");
  log(`${c.bold}Performance${c.reset} (${files.length} file(s))`);
  rows.forEach((r, i) => {
    const line = `  ${r[0].padEnd(w[0])}  ${r[1].padStart(w[1])}  ${r[2].padStart(w[2])}`;
    log(i === 0 ? `${c.dim}${line}${c.reset}` : line);
  });
  if (p.timesFaster && tscTime !== "n/a")
    log(
      `  ts-rust is ~${p.timesFaster.toLocaleString("en-US")}x faster` +
        (p.timesLessMemory
          ? ` and uses ~${p.timesLessMemory}x less memory`
          : "") +
        " than tsc",
    );
  log(
    `${c.dim}  ts-rust: one process over every file, ${p.tsRust.threads} thread(s)${threads ? "" : " (all cores)"}, time includes process start-up.${c.reset}`,
  );
  if (runs > 1)
    log(
      `${c.dim}  Times and peak memory are the median of ${runs} runs${moduleMode ? " for both" : " for ts-rust"}.${c.reset}`,
    );
  if (project) {
    log(
      `${c.dim}  tsc ${p.tsc.typescript}: one Program over the whole project, run as its own process (Node and TypeScript start-up included);${c.reset}`,
    );
    log(
      `${c.dim}  building and checking the Program took ${fmtMs(project.programMs)} of that.${c.reset}`,
    );
  } else {
    log(
      `${c.dim}  tsc ${p.tsc.typescript}: one isolated Program per file, ${p.tsc.checkedThisRun} checked now, ${p.tsc.fromCache} from cache.${c.reset}`,
    );
  }
  if (p.tsc.fromCache > 0)
    log(
      `${c.dim}  Cached files count the time they cost when last checked` +
        (p.tsc.fromCacheWithoutStoredTime
          ? ` (${p.tsc.fromCacheWithoutStoredTime} have no stored time and count as 0)`
          : "") +
        `; run with --no-cache for a fresh measurement.${c.reset}`,
    );
  if (p.tsc.maxRssKb !== null && !p.tsc.memoryMeasuredThisRun)
    log(
      `${c.dim}  * tsc memory is from the last run that checked files, not this one.${c.reset}`,
    );
  if (p.tsRust.maxRssKb === null)
    log(
      `${c.dim}  ts-rust memory needs /usr/bin/time or python3 on PATH.${c.reset}`,
    );
}

const summaryObject = () => ({
  totalFiles: files.length,
  agree: totalAgree,
  differ: totalDiffer,
  gap: totalGap,
  falsePositive: totalFp,
  exact: exactFiles,
  exactLines,
  comparedLines,
  verdicts: verdictCounts,
  strictOnly,
  onlyDiffer,
  showDiffer,
  performance,
  // `files` in the --json output is filtered to just the differing ones when
  // onlyDiffer is set -- totalFiles/agree/etc. still reflect the whole run.
});

// === Final summary ===========================================================

if (asJson) {
  console.log(
    JSON.stringify(
      {
        summary: summaryObject(),
        files: report,
        // Only present under --differ: every line where the two sides disagree.
        ...(showDiffer ? { differences } : {}),
      },
      null,
      2,
    ),
  );
  process.exit(0);
}

if (jsonStream) {
  // Everything else has already been printed line-by-line, per file, as it
  // was computed (see jsonLine() calls in renderFile above). This final
  // line is the only thing printed after all files are done -- a summary
  // object, on its own line, the same NDJSON shape as every file line
  // before it. A consumer distinguishes it from a file line by the
  // presence of `summary` rather than `file`.
  console.log(JSON.stringify({ summary: summaryObject() }));
  process.exit(0);
}

const v = verdictCounts;
log("");
const summaryColor =
  v.MATCH === files.length ? c.green : totalFp === 0 ? c.yellow : c.red;
log(
  `${summaryColor}${v.MATCH}${c.reset} of ${files.length} file(s) MATCH exactly (same tsc code and message on every line)` +
    (strictOnly ? " [--strict only]" : " [--strict + extras]"),
);
log(
  `  ${v["MESSAGE DIFFERS"]} MESSAGE DIFFERS  (same error lines and codes, wording differs)`,
);
log(
  `  ${v["CODE DIFFERS"]} CODE DIFFERS     (error on the same lines, but a different tsc code)`,
);
log(
  `  ${v.GAP} GAP             (tsc reports an error ts-rust does not -- a check not built yet)`,
);
log(
  `  ${v["FALSE POSITIVE"]} FALSE POSITIVE  (ts-rust reports an error tsc does not)`,
);
if (v.MIXED > 0)
  log(`  ${v.MIXED} MIXED           (both of the above in one file)`);
log(
  `${totalAgree} of ${files.length} file(s) agree with tsc on which lines have errors (errors-only, ${totalDiffer} differ); ` +
    `${exactLines} of ${comparedLines} line(s) are exact`,
);
if (showDiffer) {
  const n = (k) => differences.filter((d) => d.kind === k).length;
  const m = (k) => differences.filter((d) => d.mismatch === k).length;
  log(
    `  ${differences.length} line(s) differ: ${n("GAP")} gap, ${n("FALSE POSITIVE")} false positive, ${m("message")} message, ${m("code")} code`,
  );
}
printPerformance();
log("");
log(
  `${c.dim}Note: divergence here is often intentional -- see the module doc comment${c.reset}`,
);
log(
  `${c.dim}in bin/compare-tsc.rs for which fixtures are meant to diverge from tsc.${c.reset}`,
);
