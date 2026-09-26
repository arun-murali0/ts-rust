#!/usr/bin/env node
// Reads compare.js's --json output on stdin and baseline.json in this same
// directory, and fails (nonzero exit) if the false-positive count regressed
// past what baseline.json currently allows.
//
// Only false positives are gated, deliberately -- see baseline.json's
// _comment. Gaps (tsc catches something ts-rust doesn't check yet) are
// expected on a checker that's still being built out feature by feature,
// and gating on them would make CI red for every legitimate "not built yet"
// fixture, which isn't a regression.
//
// Usage (see .github/workflows/ci.yml's tsc-drift job for the full pipeline):
//   node compare.js tests/fixtures target/release/ts-rust --json | node check-baseline.js

const fs = require("fs");
const path = require("path");

let raw = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (raw += chunk));
process.stdin.on("end", () => {
  let result;
  try {
    result = JSON.parse(raw);
  } catch (err) {
    console.error("error: could not parse compare.js --json output as JSON");
    console.error(err.message);
    process.exit(2);
  }

  const baselinePath = path.join(__dirname, "baseline.json");
  const baseline = JSON.parse(fs.readFileSync(baselinePath, "utf8"));

  const actual = result.summary.falsePositive;
  const allowed = baseline.maxFalsePositive;

  console.log(`false positives: ${actual} (baseline allows up to ${allowed})`);

  if (actual > allowed) {
    console.error(
      `\nFAIL: false-positive count regressed from ${allowed} to ${actual}.\n` +
        `A false positive means ts-rust rejects code real tsc accepts -- that's the ` +
        `more damaging failure mode (it breaks valid code, not just under-checks it).\n` +
        `Run ./scripts/compare-local.sh --only-differ to see which fixture(s) newly ` +
        `regressed, then either fix the checker or, if the divergence is deliberate ` +
        `(matches this project's own documented recovery behavior), update ` +
        `${path.relative(process.cwd(), baselinePath)} explaining why.`,
    );
    process.exit(1);
  }

  if (actual < allowed) {
    console.log(
      `\nNote: false-positive count improved (${allowed} -> ${actual}). Consider ` +
        `lowering maxFalsePositive in ${path.relative(process.cwd(), baselinePath)} ` +
        `so this doesn't silently regress back up later.`,
    );
  }

  process.exit(0);
});
