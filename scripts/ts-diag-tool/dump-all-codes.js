#!/usr/bin/env node
// Dumps TypeScript's own internal diagnostic table: every (code, category,
// message template) triple it knows about, straight from the compiler's
// source of truth (ts.Diagnostics), not from parsing CLI output.
//
// This is the full universe of codes -- including ones no small fixture set
// will ever trigger -- so it's the right source for a complete code->message
// reference table. It gives you the *template* (with {0}, {1} placeholders
// unfilled), not a message from one specific real error.
//
// Usage: node dump-all-codes.js [--json]

const ts = require("typescript");

const asJson = process.argv.includes("--json");

// ts.Diagnostics is an object keyed by the message's own snake_ish key
// (e.g. "Type_0_is_not_assignable_to_type_1_2322"), each value a
// DiagnosticMessage: { key, category, code, message, reportsUnnecessary?,
// reportsDeprecated?, elidedInCompatabilityPyramid? }
const entries = Object.values(ts.Diagnostics)
  .filter((d) => typeof d === "object" && d !== null && "code" in d)
  .sort((a, b) => a.code - b.code);

if (asJson) {
  const out = entries.map((d) => ({
    code: d.code,
    category: ts.DiagnosticCategory[d.category],
    message: d.message,
  }));
  console.log(JSON.stringify(out, null, 2));
  process.exit(0);
}

console.log(`# ${entries.length} known TypeScript diagnostic codes\n`);
for (const d of entries) {
  const cat = ts.DiagnosticCategory[d.category].padEnd(11);
  console.log(`TS${d.code}\t${cat}\t${d.message}`);
}
