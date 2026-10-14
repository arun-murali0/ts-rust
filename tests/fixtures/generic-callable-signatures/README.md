# Generic callable signature fixtures

These fixtures cover generic metadata that must remain attached to a callable instead of
being reconstructed from parameter and return-type graphs:

- unused generic parameters and default arguments;
- explicit type arguments for parameters absent from the signature graph;
- generic function-type annotations;
- generic arrow expressions;
- generic method signatures;
- nested callable-owned type parameters;
- a negative constraint-inference case;
- a two-file provider/consumer pair for portable export/import of generic function types
  and generic interface methods (`cross_file_provider.ts` and `cross_file_consumer.ts`).

Run locally with:

```sh
cargo test --test generic_callable_signatures
```

The cross-file pair is intended for the repository's module-resolution/project fixture
harness; `check_source` alone does not resolve imports. Run that pair through the same
project entry point used by the existing module-resolution tests as well as the test command
above.
