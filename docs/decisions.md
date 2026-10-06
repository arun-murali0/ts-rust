# Decisions

> **Architecture case study:** what was decided at stage 0, why, and what would reopen it.

The design is in `HLD.md` and `LLD.md`. This page is the short list of what was left open there and is now closed, so a later change can check itself against it. Each decision stays as it is until the condition in the last column happens.

| # | Decision | Reason | Reopen when |
| --- | --- | --- | --- |
| 1 | `DeclKey` is identity, `SigHash` is version | A signature inside the id changes the id of every referrer on each edit, which defeats early cutoff | Never expected; this is the base of ADR-5 |
| 2 | Aliases are transparent wrapper nodes (`Named`) | A side table keyed by id is what forces `set()` and `make_unique` today | Stage 1 shows the wrapper costs more than the table it replaces |
| 3 | Union display order is first writer wins, in a side table | Identity stays canonical and messages keep the written order, closest to tsc | The tsc comparison shows order mismatches that matter |
| 4 | The v1 unit of work is one SCC | Cycle members share one arena, so nothing crosses arenas inside a cycle | S4 shows one SCC dominating wall time |
| 5 | Hash function is XXH3-128 | Already a dependency, and fast | Input must be treated as adversarial |
| 6 | The hash is computed lazily, at an export boundary | Most types never leave their file | S2 shows lazy and eager disagree |
| 7 | Salsa is pinned at `=0.28.5`, provisionally | It is what the foundation layer builds on | S1 fails |
| 8 | A module key follows the rule in LLD 3.1.1 | An id has to survive moving the project and a different install layout | A platform test shows two spellings of one file get two keys |
| 9 | `FileId` is a run-local index, sorted by canonical path | Output order must not depend on discovery order, and ids must not be stored | Never expected; it only works if no key uses it |
| 10 | Intersection identity is ordered, deduplicated by id, never sorted | Call-signature order depends on member order, and callability needs a body that may not exist yet. tsc 5.9.3 does the same | A fixture shows overload order through intersections is not needed |
| 11 | Intersection reduction has two levels: ids and wrappers at construction, bodies lazily | A decision that read a body while a declaration was unresolved would bake a wrong answer into the intern table | Never expected; it is the placeholder bug turned around |
| 12 | A product of union sizes of 100,000 or more is TS2590 | Same cap as tsc | The ADR-6 budgets need it lower |

## The module key, in one place

```text
Project   : path relative to the project root, "/" separators
Package   : package name, version, path inside the package
External  : absolute canonical path
```

A file under `node_modules` is a Package file when its nearest `package.json` has both a name and a version, and an External file when none does. A workspace package or a linked package resolves outside `node_modules`, so it is a Project file. Nothing in a key comes from a `FileId`, the absolute root or a timestamp.

The cost is accepted and written down: two copies of one name and version with different contents, one patched in place, share a key.

## What this changes in the code

Decision 9 is the first one that does. `ModuleGraph::build` still walks the project with discovery ids, but only until the walk ends. It then sorts the discovered paths and renumbers every file and every edge target, so the ids it publishes follow path order and no longer depend on the order the entries were given in. `ProjectFiles` itself is unchanged and still hands out ids in the order it is asked.

Nothing implements decision 8 yet: there is no key code, by design, until stage 2.
