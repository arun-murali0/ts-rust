// Expect: no diagnostics (same as tsc).
// Gap if reported: `{ ...base, b: 2 }` drops the spread, so `d.a` is
// reported as a missing property.
const base = { a: 1 };
const d = { ...base, b: 2 };
const n: number = d.a;
