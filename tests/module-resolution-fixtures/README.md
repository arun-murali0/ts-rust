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

These test project graph resolution. Looking an imported name up in the file it comes from
is a separate stage and is not claimed here.
