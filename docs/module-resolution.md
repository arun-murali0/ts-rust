# Module Resolution

> **Architecture case study:** giving the checker a project to work on without teaching it about files.

## 1. Why Module Resolution is a separate milestone

Every earlier milestone checks one source string. A real project is a set of files that import each other, and before any name can be looked up across a file boundary, three smaller questions need an answer: which file does an import mean, in what order can the files be checked, and which files changed since they were last read.

Module Resolution answers those three and nothing else. It does not bind an imported name to its declaration in another file; that is the next consumer of what this milestone builds, and the document says so in section 10 so the gap is not read as a claim.

There are 20 integration tests in `tests/module_resolution.rs`, 10 unit tests next to the code, and 7 small projects under `tests/module-resolution-fixtures/`.

## 2. An opt-in feature, not a default dependency

```text
cargo build                               -> single-file checker, as before
cargo build --features module-resolution  -> adds src/module_resolution/
```

The feature pulls in `oxc_resolver`, `rayon` and `xxhash-rust`. It is off by default because everything in it reads the filesystem, and the in-memory checker, the benchmarks and the WASM build neither have a filesystem nor want the dependencies. `ProjectFiles` and `FileId` stay in the always-built core, so the seam the project layer attaches to is available either way.

## 3. The resolver stays `oxc_resolver`

```text
file + specifier -> ModuleResolver -> oxc_resolver -> path
```

Resolving `"./a"` to a path is a large, edge-case-heavy job (extension probing, directory entries, `exports` and `imports` maps, `node_modules` lookup), and `oxc_resolver` already does it. `ModuleResolver` is a wrapper that picks options and converts the error into `ModuleError`, so the dependency does not show up in the public API.

The options are the TypeScript ones:

- `types` is the first condition name and the first main field, so a package's declaration entry wins over its JavaScript one.
- The extension aliases follow NodeNext: `./legacy.js` in source means `./legacy.ts` on disk, because the emitted file is the one that gets named.

A value import is resolved as a runtime would be, with a fallback to declaration files, because a package can ship types and no code. A type-only import skips straight to declarations: its target is always a `.d.ts`, so whichever JavaScript entry the plain resolver would pick is the wrong file. `package_exports_prefer_the_types_condition` and `a_type_only_import_is_marked_and_resolved` protect those two rules.

## 4. Discovery reads the parser's record

Oxc already records every import and re-export while parsing. `scan_module_requests` parses a file and reads that record, so no second pass walks the AST looking for import statements.

Two details matter:

- The record is a hash map keyed by specifier, and its iteration order changes between runs. Requests are sorted by source position so the graph is the same every time.
- `is_import` is false for `export ... from`. That statement depends on the other module as much as an import does but binds nothing in this one, so it is an edge with a flag and not a different kind of thing.

A file with a syntax error is the dangerous case. If its requests came back empty it would look like a file with no imports. The scan carries `parse_failed` separately, and the graph exposes it as `has_syntax_errors`, so a file that lost edges is never mistaken for one that has none.

## 5. The graph is indexed by `FileId`

`ModuleGraph` owns a `ProjectFiles` and several plain vectors indexed by `FileId`: edges, dependencies, dependents, file state and syntax-error flags. `ProjectFiles` hands out ids in discovery order, so a lookup is an array index and never hashes a path.

```text
entries (absolute paths) -> intern -> FileId 0, 1, ...
walk ids in order:
    read file -> scan requests -> resolve each -> intern the target (a new id at the end)
```

Because new files are interned at the end of the list while it is being walked, visiting ids in order is a breadth-first walk with no queue.

Two decisions are worth stating:

- **An unresolved import stays as an edge with `target: None`.** Dropping it would make a file with a broken import look like a file with no imports, and the phase that reports a missing module needs the specifier. `an_unresolved_import_stays_as_an_edge_without_a_target` protects this.
- **`dependencies` and `dependents` are deduplicated, in both directions.** Two statements importing the same file are one dependency. "What must be checked first" and "what a change affects" are each one lookup.

Entries must be absolute. Paths are not canonicalized, so one file reached under two spellings would get two ids. `a_relative_entry_is_rejected` makes the rule visible.

## 6. Cycles and layers

Cycles are structure, not errors. TypeScript allows them, so the graph reports them and does not refuse to build.

`components()` is Tarjan's algorithm with an explicit stack. An import chain is as long as the project's deepest dependency path, and a recursive version puts that depth on the thread's call stack. `a_long_import_chain_does_not_overflow_the_stack` builds a chain of 50,000 files to keep it honest.

Tarjan emits a component only after every component it imports from, which is exactly the order layering needs:

```text
depth(component) = 1 + max depth of the components it imports
layer n          = every file whose component has depth n
```

Members of one cycle share a layer, since none of them can go first. A file that merely imports a cycle comes after it and is not part of it. Putting every file left over after the acyclic layers into one last layer would be simpler, but it would sweep the importers of a cycle in with the cycle; `a_ring_is_one_cycle_and_its_importer_is_not_part_of_it` and `layers_run_dependencies_first_and_keep_a_cycle_together` protect the distinction.

## 7. Checking a project

```text
for each layer, in order:
    files of the layer in parallel (rayon)
        each file: its own CheckSession -> diagnostics
sort the report by FileId
```

- **One session per file.** A session reuses its arena across repeated checks of one file, but here each file is checked once. A session per file means the workers share nothing mutable, so no lock is needed anywhere in the checker.
- **Sorted by `FileId`.** `FileId` derives `Ord` for this. Without the sort, the report order would depend on which thread finished first.
- **Only checkable files.** TypeScript source outside `node_modules`. Declaration files of dependencies are in the graph because their imports matter, but checking someone else's package is not the project's job. `dependency_declarations_are_in_the_graph_but_are_not_checked` protects that.
- **No file is left out.** A file that cannot be read is `ReadFailed` and one the checker rejects is `CheckFailed`. A report that silently omits files reads as all clear for exactly the files nobody looked at, which is the same rule that keeps an unresolved import in the graph. `ProjectReport::failure_count` separates files that were not checked from files that were checked and found wrong.

Each file is still checked on its own, and that is deliberate. An import statement currently gets the same unimplemented-statement warning it gets in single-file mode. The layering is in place so that once an import is looked up in the file it names, that file has already been checked.

## 8. Change detection in two steps

```text
FileFingerprint (length + mtime)  -> one stat call, decides the common case
content_hash    (XXH3 of bytes)   -> read and hash only if the fingerprint moved
```

A touched file whose bytes did not change has a new fingerprint and the old hash, and is unchanged. `a_rewrite_with_the_same_bytes_is_not_a_change_but_new_bytes_are` checks both halves.

The fingerprint is taken before the read, so a file that changes in between is recorded under the older fingerprint and the change is still seen later. A file that cannot be read counts as changed: calling it unchanged would keep stale results for a file that may be gone, while calling it changed costs one re-check. `a_deleted_file_counts_as_changed` protects that.

The fingerprint is only as trustworthy as the filesystem clock. A rewrite that keeps the size and lands in the same timestamp tick is invisible to it, the same blind spot `make` has. The tests move the mtime forward explicitly so they do not depend on how fine the clock is.

## 9. Fixtures live outside `tests/fixtures/`

The tsc comparison harness checks every `.ts` file under `tests/fixtures/` on its own, where each of these imports would be an error. The module fixtures are directory trees whose imports are meant to resolve, so they live in `tests/module-resolution-fixtures/` and `.gitignore` is told not to drop their `node_modules` folders.

| Fixture | Protects |
| --- | --- |
| `basic` | extensionless, directory, `.js`-to-`.ts`, type-only, re-export and unresolved edges |
| `package-exports` | `exports` with the `types` condition, and subpath exports |
| `package-imports` | `#internal` specifiers through the nearest `package.json` |
| `nested-node-modules` | a nested package shadowing the hoisted one |
| `cycles` | a ring, its importer and a leaf |
| `self-import` | a file importing itself |
| `project-check` | a clean file, a file with a type error and a file with a syntax error |

## 10. What is still not done

- **Cross-file binding.** An imported name is not yet looked up in the file it comes from. That needs an export table per checked file, which is the next stage in `BUILD.md`.
- **tsconfig-aware resolution.** The resolver is built without a tsconfig, so `paths` and `baseUrl` are not applied.
- **Ambient modules and augmentation.** `declare module` is not modeled.
- **Using the change report.** `changed_files` says what differs; nothing re-checks only those files and their dependents yet.
- **`import type` as a checker distinction.** The graph records the flag; the checker does not read it.

## 11. What Module Resolution completed

- an opt-in `module-resolution` feature
- a resolver wrapper with TypeScript options, `types`-first conditions and NodeNext extension aliases
- discovery from the parser's module record, in source order, with parse failures kept visible
- a graph indexed by `FileId` that keeps unresolved edges and both edge directions
- cycle detection and dependency layers without recursion
- a parallel project check that reports every file in a stable order
- two-step change detection with a hash only when the fingerprint moves

The architecture lesson is: **a layer above the checker should hand it files in a safe order, and keep every fact it could not resolve as data instead of dropping it.**
