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
const ts = require("typescript");

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

  files.forEach((file, index) => {
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

    results.push(...fileResults);
    if (options.onFile) options.onFile(file, fileResults, index, files.length);
  });

  return { files, results };
}

module.exports = { checkFiles, collectTsFiles, baseOptions, strictExtras };

// CLI entry point -- only runs when this file is executed directly (`node
// check-fixtures.js ...`), not when required as a module by compare.js.
if (require.main === module) {
  const args = process.argv.slice(2);
  const asJson = args.includes("--json");
  const strictOnly = args.includes("--strict-only");
  const target = args.find((a) => !a.startsWith("--"));

  if (!target) {
    console.error(
      "usage: node check-fixtures.js <dir-or-file> [--json] [--strict-only]",
    );
    process.exit(2);
  }

  const { files, results } = checkFiles(target, { strictOnly });
  if (files.length === 0) {
    console.error(`no .ts files found under ${target}`);
    process.exit(2);
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
