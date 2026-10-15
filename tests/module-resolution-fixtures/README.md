# Module resolution fixtures

Small projects on disk for `tests/module_resolution.rs`. They are not under
`tests/fixtures/` because the tsc comparison checks each `.ts` file there on its own, where
every import in these projects would be an error.

| Directory | What it holds |
| --- | --- |
| `basic/` | extensionless, directory, `.js`-to-`.ts`, type-only, re-export and unresolved imports |
| `package-exports/` | a package with `exports`, a `types` condition and a subpath |
| `package-imports/` | `#internal` specifiers through the nearest `package.json` |
| `nested-node-modules/` | a nested package that shadows the hoisted one |
| `cycles/` | a ring, a file that imports the ring, and a leaf the ring imports |
| `self-import/` | a file that imports itself |
| `project-check/` | a clean file, one with a type error and one with a syntax error |
| `overloads/` | exported overload signatures without leaking the implementation signature |
| `default-expression/`, `default-function/` | default expression and anonymous inferred function signatures |
| `export-star-as/` | `export * as ns` value and type namespaces |
| `export-assignment/` | `export =` object properties exposed to named imports |
| `ambient-merge/`, `augmentations/` | repeated ambient module declarations and module augmentation merges |
| `umd/`, `umd-callable/` | `export as namespace` globals available to scripts but not external modules, including callable `export =` globals |
| `edge-kinds/` | static, side-effect, dynamic, re-export and import-equals edge kinds |

These test project graph resolution. Looking an imported name up in the file it comes from
is a separate stage and is not claimed here.

- `export-assignment-expression/`: expression-valued `export =` retains inferred object properties.
- `export-star-partial/`: `export * as ns` preserves known value members from a partial source module.
