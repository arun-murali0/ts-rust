Source: microsoft/TypeScript tag v6.0.3, tests/cases/{compiler,conformance}
typescript-6.0.3/part-01 .. part-21   single-file .ts cases, 500 per folder (part-21 has 83); 10,083 total
known-crashers-6.0.3/                 8 cases that abort ts-rust in project mode (out of memory or stack overflow);
                                      keep them out of folder runs, run them one at a time
Skipped: 1,983 multi-file (// @filename), 330 .tsx, 40 .d.ts

Run one folder at a time:
  RUST_MIN_STACK=67108864 ./scripts/compare-local.sh tests/tsc-conformance/typescript-6.0.3/part-01

RUST_MIN_STACK is needed: without it, project mode overflows a worker-thread stack on some batches.
