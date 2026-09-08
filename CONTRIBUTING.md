# Contributing

## Workflow

1. Make a focused change.
2. Add or update regression coverage.
3. Run the repository quality script.
4. Review the diff for accidental files, secrets, and unrelated formatting churn.
5. Keep documentation aligned with the implemented architecture stage.
6. Open a pull request and let CI verify the same quality gates.

## Local checks

```bash
./scripts/ci.sh
```

The script formats the workspace, then runs compilation, Clippy with warnings denied, and the full test suite. The local pre-push hook uses the same sequence.

## Code organization

Keep parser and Oxc-specific logic inside `src/bridge/`. Keep the core type system independent from AST ownership details. Centralize semantic compatibility rules in the subtype layer.

## Tests

A semantic bug fix should normally include a regression fixture demonstrating the expected behavior. Prefer small fixtures that isolate one rule.

## Error handling

Do not use `unwrap()` as normal error handling. Prefer explicit propagation or an intentional fallback. Do not weaken strict Clippy rules simply to make CI pass.

## Comments and documentation

Keep source comments rare and limited to non-obvious invariants or decisions. Put architectural reasoning, development history, and stage boundaries in `docs/`.
