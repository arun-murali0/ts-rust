#!/usr/bin/env node
// Runs the real TypeScript compiler (via its JS API, not the CLI) over a set
// of .ts files and dumps every diagnostic as structured data: code, category,
// exact message (with real type names substituted), file, line, column.
//
// Unlike shelling out to `tsc` and regexing its pretty-printed text back
// apart, this reads ts.Diagnostic objects directly -- no risk of a type name
// containing a paren or colon confusing a parser, and you get the numeric
// `code` and `category` as real fields, not something recovered from text.
//
// Each file is compiled in ITS OWN isolated Program (matching compare-local.sh's
// own reasoning: fixtures share names like `Point`/`Animal`/`add` and batching
// them into one Program would produce spurious duplicate-identifier errors
// that have nothing to do with real type checking).
//
// Usage:
//   node check-fixtures.js <dir-or-file> [--json] [--strict-only]
//
// By default this compiles with --strict PLUS the extra flags that are real,
// commonly-used checks but are NOT part of --strict: allowUnreachableCode
// (inverted to false), noUncheckedIndexedAccess, noImplicitOverride,
// exactOptionalPropertyTypes, noPropertyAccessFromIndexSignature,
// noFallthroughCasesInSwitch, noImplicitReturns. Pass --strict-only to
// compile with plain --strict instead, matching compare-local.sh's current
// flag set exactly.

const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const ts = require("typescript");

const sha = (text) => crypto.createHash("sha1").update(text).digest("hex");

// Result cache, so only fixtures that changed pay for a tsc run.
//
// Problem: tsc over the full suite takes several minutes, and almost none of
// it is needed on any given merge -- a fixture's diagnostics only depend on its
// own text (and on the files it imports), not on the ts-rust change being tested.
// Picked: each file's diagnostics are stored with the sha1 of the file and of every
// non-lib file its isolated Program pulled in. A later run reuses the entry when all
// of those hashes still match. The whole cache is dropped when the TypeScript
// version or the compiler options change, since either one changes what tsc says.
// Cost: an import that did not resolve last time and exists now is not noticed, so
// delete the cache file (or pass --no-cache to compare.js) after adding such a file.
const CACHE_SCHEMA = 1;

function loadCache(cachePath, meta) {
  try {
    const cache = JSON.parse(fs.readFileSync(cachePath, "utf8"));
    const m = cache.meta || {};
    if (
      m.schema === meta.schema &&
      m.tsVersion === meta.tsVersion &&
      m.optionsHash === meta.optionsHash
    )
      return cache;
  } catch {
    // missing or unreadable: start empty
  }
  return { meta, entries: {} };
}

function saveCache(cachePath, cache) {
  // Entries for fixtures that no longer exist would only make the file grow.
  for (const key of Object.keys(cache.entries))
    if (!fs.existsSync(key)) delete cache.entries[key];
  const tmp = `${cachePath}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(cache));
  fs.renameSync(tmp, cachePath);
}

function readHash(file) {
  try {
    return sha(fs.readFileSync(file));
  } catch {
    return null;
  }
}

function entryIsFresh(entry, fileHash) {
  if (!entry || entry.hash !== fileHash) return false;
  return Object.entries(entry.deps).every(([dep, h]) => readHash(dep) === h);
}

function collectTsFiles(target) {
  const stat = fs.statSync(target);
  if (stat.isFile()) return [target];
  const out = [];
  for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) out.push(...collectTsFiles(full));
    else if (entry.name.endsWith(".ts") && !entry.name.endsWith(".d.ts"))
      out.push(full);
  }
  return out;
}

const baseOptions = {
  strict: true,
  noEmit: true,
  skipLibCheck: true,
  target: ts.ScriptTarget.ES2020,
  module: ts.ModuleKind.CommonJS,
};

// The extras that are real, commonly-enabled checks but sit OUTSIDE --strict,
// so a fixture exercising one of these silently never triggers tsc at all
// unless they're turned on explicitly.
const strictExtras = {
  allowUnreachableCode: false, // default is `undefined` (unreachable code is NOT an error); false makes it TS7027
  noUncheckedIndexedAccess: true, // adds `| undefined` to indexed access results
  noImplicitOverride: true, // TS4114/TS4113
  exactOptionalPropertyTypes: true, // TS2412 and stricter `?:` semantics
  noPropertyAccessFromIndexSignature: true, // TS4111
  noFallthroughCasesInSwitch: true, // TS7029
  noImplicitReturns: true, // TS7030
};

// Runs real tsc (via the compiler API) over every .ts file under `target`
// (a single file or a directory, searched recursively) and returns a flat
// array of { file, line, column, code, category, message }. `file` is
// relative to cwd. `line`/`column` are 1-based, or undefined for a
// diagnostic with no source position (rare, e.g. a config-level error).
//
// Each file gets its own isolated ts.Program -- see the module doc comment
// at the top of this file for why (fixtures across a suite commonly reuse
// names like `Point`/`Animal`/`add`, and one shared Program would report
// spurious cross-file duplicate-identifier errors).
//
// options.strictOnly: true compiles with plain --strict only, matching
// compare-local.sh's historical flag set. false (the default) adds the
// extra checks real tsc supports but does not enable under --strict alone
// (see strictExtras above) -- turn this on to reproduce old results.
//
// options.cachePath: if given, results are read from and written to that file (see
// the cache comment above). The return value then also carries `stats`, how many
// files came from the cache and how many tsc actually ran on.
//
// options.onFile(file, fileResults, index, total): if given, called
// synchronously right after each individual file finishes checking -- lets
// a caller (compare.js) print progress/results per-file as they land,
// instead of waiting for the whole set to finish before anything is shown.
function checkFiles(target, options = {}) {
  const compilerOptions = options.strictOnly
    ? baseOptions
    : { ...baseOptions, ...strictExtras };

  const files = collectTsFiles(target).sort();
  const results = [];

  const cache = options.cachePath
    ? loadCache(options.cachePath, {
        schema: CACHE_SCHEMA,
        tsVersion: ts.version,
        optionsHash: sha(JSON.stringify(compilerOptions)),
      })
    : null;
  const stats = { cached: 0, ran: 0 };

  files.forEach((file, index) => {
    const cacheKey = path.relative(process.cwd(), file);
    const fileHash = cache ? readHash(file) : null;
    const cached = cache && cache.entries[cacheKey];
    if (cache && fileHash && entryIsFresh(cached, fileHash)) {
      stats.cached++;
      results.push(...cached.diagnostics);
      if (options.onFile)
        options.onFile(file, cached.diagnostics, index, files.length);
      return;
    }
    stats.ran++;

    const program = ts.createProgram([file], compilerOptions);
    const sourceFile = program.getSourceFile(file);
    const diagnostics = ts.getPreEmitDiagnostics(program, sourceFile);

    const fileResults = [];
    for (const d of diagnostics) {
      let line, character;
      if (d.file && d.start !== undefined) {
        ({ line, character } = d.file.getLineAndCharacterOfPosition(d.start));
        line += 1;
        character += 1;
      }
      fileResults.push({
        file: path.relative(process.cwd(), file),
        line,
        column: character,
        code: d.code,
        category: ts.DiagnosticCategory[d.category],
        message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
      });
    }

    if (cache && fileHash) {
      // Everything the isolated Program read besides the TypeScript lib files.
      const deps = {};
      for (const sf of program.getSourceFiles()) {
        if (program.isSourceFileDefaultLibrary(sf) || sf === sourceFile)
          continue;
        const h = readHash(sf.fileName);
        if (h) deps[sf.fileName] = h;
      }
      cache.entries[cacheKey] = {
        hash: fileHash,
        deps,
        diagnostics: fileResults,
      };
    }

    results.push(...fileResults);
    if (options.onFile) options.onFile(file, fileResults, index, files.length);
  });

  if (cache) saveCache(options.cachePath, cache);
  return { files, results, stats };
}

// One shared location so compare.js, this file's --warm-cache and the CI cache step all
// agree. TSC_CACHE overrides it.
function defaultCachePath() {
  return process.env.TSC_CACHE || path.join(__dirname, ".tsc-cache.json");
}

module.exports = {
  checkFiles,
  collectTsFiles,
  defaultCachePath,
  baseOptions,
  strictExtras,
};

// CLI entry point -- only runs when this file is executed directly (`node
// check-fixtures.js ...`), not when required as a module by compare.js.
if (require.main === module) {
  const args = process.argv.slice(2);
  const asJson = args.includes("--json");
  const strictOnly = args.includes("--strict-only");
  // Fills the cache without printing diagnostics; what the tsc-cache workflow runs on main.
  const warmCache = args.includes("--warm-cache");
  const target = args.find((a) => !a.startsWith("--"));

  if (!target) {
    console.error(
      "usage: node check-fixtures.js <dir-or-file> [--json] [--strict-only] [--warm-cache]",
    );
    process.exit(2);
  }

  const { files, results, stats } = checkFiles(target, {
    strictOnly,
    cachePath: warmCache ? defaultCachePath() : undefined,
  });
  if (files.length === 0) {
    console.error(`no .ts files found under ${target}`);
    process.exit(2);
  }

  if (warmCache) {
    console.error(
      `tsc cache: ${stats.cached} reused, ${stats.ran} checked, ${files.length} total`,
    );
    process.exit(0);
  }

  if (asJson) {
    console.log(JSON.stringify(results, null, 2));
    process.exit(0);
  }

  for (const r of results) {
    const loc =
      r.line !== undefined ? `${r.file}:${r.line}:${r.column}` : r.file;
    console.log(`${loc}\t${r.category}\tTS${r.code}\t${r.message}`);
  }

  console.error(
    `\n${results.length} diagnostics across ${files.length} file(s)` +
      (strictOnly ? " (--strict only)" : " (--strict + extras)"),
  );
}
