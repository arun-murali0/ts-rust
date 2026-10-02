# Case study: why the checker is built the way it is

This is the document to read when a design choice looks odd and the question is "why not
the simpler thing?". Each section is one decision: the problem, the options, what was
chosen, and how it was checked. The per-topic documents (`arena-architecture.md`,
`control-flow-narrowing.md`, `generics-tier.md`) hold the detail; this one holds the
reasoning that connects them.

## 0. What the project optimises for

Three priorities, in this order, decide most questions below.

1. **A wrong error is worse than a missing one.** A checker that reports a false error
   makes people stop trusting it; one that misses an error is merely incomplete.
   Whenever the checker cannot decide, it keeps the wider type, drops the narrowing, or
   reports nothing. This is why so many handlers say "left alone" for the case they do
   not understand.
2. **Agree with `tsc`, measured, not assumed.** `scripts/ts-diag-tool` runs every fixture
   through real `tsc --strict` and the checker, and CI fails if the number of false
   positives rises above `baseline.json`. Features are judged by that comparison, not by
   whether they look like TypeScript.
3. **Keep the common path cheap.** Arena ids, interning and small inline stacks exist so a
   large file costs little, but a design that is fast and wrong loses to rule 1.

## 1. Types are ids into an arena, not a tree of boxes

**Problem.** Types refer to each other, recursively (`interface Node { next: Node }`), and
the same shape appears thousands of times. A tree of owned boxes cannot express a cycle and
copies shared structure.

**Choice.** One `Vec<Type>`, with `TypeId` an index. Types are shallow: they hold ids of
their parts. Anonymous composites are interned, so two identical `{ a: number }` share an
id and equality is an integer compare. Named types stay unique so each keeps its own name
in messages. Recursive types are allocated as placeholders first and filled in with
`set()`.

**Consequence.** Everything that remembers something about an id (the relation cache, the
parameter-scan memo) must be told when an id changes meaning. That is the cost of
placeholders, and section 4 is how it is paid.

## 2. Structural subtyping with a "currently comparing" stack

**Problem.** Recursive types compared against each other never terminate if each step
recurses.

**Choice.** A stack of the (left, right) pairs in progress. Meeting a pair already on the
stack counts as true: the pair is assumed to hold while its own proof is being built, which
is the standard coinductive treatment.

**Why a `SmallVec`.** The stack is almost always a handful of entries. Sixteen inline slots
mean the heap is never touched in normal use, and scanning sixteen integers is cheaper than
hashing. A deep type moves to the heap and still works, which a test forces.

## 3. Narrowing is an overlay on the declared type

**Problem.** A variable has one declared type and a different type on each path.

**Choice.** `SymbolTypeMap` holds declared types and never changes inside a branch.
`NarrowState` is an overlay of `(SymbolId, TypeId)` pairs. A condition yields a pair of
overlays, one per outcome. Mutating the declared type instead would make a narrowing
outlive its branch, which is a silent wrong-answer bug.

**Joins.** Saving and restoring is safe but throws away what a branch learned. The state
after a branch is decided by which paths can fall through (a guard clause leaves the
alternate's state, an `if`/`else` that both continue leaves the join). The join keeps a
symbol only if both paths narrowed it, unioning the two types. Dropping a symbol only ever
loses precision, so the join cannot be unsound, which is why it can be one small function
used for `if`, loops and `a && b` alike.

**The `never` trap.** Narrowing a plain `string` by `x === "a"` once produced `never`.
`never` is assignable to everything, so the bug raised no error at all, and the first
fixtures passed. The fix came with fixtures that assign the narrowed value to an
incompatible type and expect exactly one error: the only observable difference between
`"a"` and `never`. When a bug is silent, the test has to be built to make it loud.

**Where the handlers stop.** Property paths, user-defined predicates, optional chaining and
a join after `switch` are not done, and the narrowing document says why each needs more
than another handler.

## 4. A generation counter, not an invalidation rule

**Problem.** The relation cache keys on a pair of ids. When a placeholder is filled in, an
id's meaning changes and the cached answer is wrong. The first design was a rule: "resolve
declarations before checking bodies, and never query while a placeholder is empty". It held
because nothing broke it yet.

**Choice.** The arena counts changes of meaning (`set`, `clear`). The cache records the
generation it was filled under and drops its entries if the arena's has moved. No caller
has to remember anything.

**Principle.** If correctness depends on every caller following a rule, make the data
structure follow it instead. The same reasoning put the `FileId` inside `TypeParameterId`
instead of asking each caller to keep files apart.

## 5. Generic inference

**First candidate wins, widened only by subtyping.** `pair<T>(a: T, b: T)` called as
`pair(1, "x")` is a genuine TypeScript error, not `T = number | string`. So a second
candidate replaces the binding only when it is a supertype, and otherwise the ordinary
argument check reports the mismatch.

**Literals widen, unless the bound asks for them.** `identity(5)` is `number`. But
`T extends "a" | "b"` asks for the narrow type; widening would bind `T` to `string` and
then reject the very argument it came from. A bound that mentions a primitive or literal
keeps the literal, and same-primitive literals combine (`pair(1, 2)` is `1 | 2`).

**An unresolvable explicit type argument stays in position.** Arguments are matched to
parameters by position. Dropping one shifts the rest left onto the wrong parameters and
produces a bogus error on an argument that was fine. It becomes the error type, which is
compatible with everything, so exactly one parameter goes quiet.

**The memo is keyed on a fingerprint and confirmed exactly.** Keying on
`(shape, Vec<bindings>)` would allocate a `Vec` on every lookup, which is a good part of
what the memo exists to save. A 64-bit fingerprint picks the bucket and the stored bindings
are compared before a hit is returned, so a collision costs a comparison and never a wrong
type.

## 6. Declaration collisions: report only what is certainly an error

TypeScript merges some duplicate declarations and rejects others. This checker models only
interface-with-interface merging, so it replaces the earlier entry for the rest. Flagging a
merge it does not model would report an error `tsc` does not. So a collision is reported
only when it is illegal in TypeScript: an alias on either side, two classes, an enum
against anything but an enum. Interface-with-class and enum-with-enum are accepted
silently and recorded in the checklist as a known limit. Both declarations are reported,
as `tsc` does, so the output compares line for line.

## 7. Sessions and file identity

An editor re-checks one file constantly. Reusing the arena's allocation is the obvious
saving, but only if nothing leaks between checks, so `clear()` resets every side table and
a test checks that a name from the previous file is unknown in the next. The reusable state
lives in a `CheckSession`, not in `TypeChecker`: the checker stays free of interior
mutability, and independent sessions can run in parallel. `FileId` is a plain index, not a
path, so type identity never depends on a filesystem.

## 8. Considered and not done

- **Ignoring parameter names in the intern key.** Two functions that differ only in
  parameter names would share an id, which speeds equality. But diagnostics print
  parameter names (`Expected 2 arguments, missing 'b'`), and a shared id keeps the first
  function's names. Wrong text in a message costs more than the saved comparison, and
  `structurally_equal` already ignores names where equality is asked directly.
- **Caching union de-duplication.** Connecting `structurally_equal_cached` to `alloc_union`
  changed diagnostic counts once and the cause was not found. It stays disconnected.
- **Module resolution and `oxc_resolver`.** Left to the module-resolution work in
  progress, to avoid two competing implementations. `ProjectFiles` and `FileId` are the
  seam it can attach to.
- **Baseline-first benchmarking.** No speed figures are claimed for the reuse work. The
  counters make a regression observable; the numbers belong to a measured run.

## 9. How the decisions were checked

- **Fixtures first, in pairs.** A fixture that must stay clean, and one that must produce
  exactly one error, so both a false positive and a silent over-narrowing show up.
- **The `tsc` comparison.** Every new fixture is run through real `tsc`. A fixture where
  the two disagree is either a recorded gap or a bug, and the false-positive count may not
  rise.
- **Tests for what must not survive.** Reuse code is tested by asserting what is gone (a
  name from the previous file, a cached answer from before a placeholder was filled), not
  by asserting the happy path twice.
- **Counters over opinions.** Hit and miss counts and arena sizes are exposed so a claim
  such as "the memo answers repeats" is a test assertion.

## 10. Rules of thumb that fell out of this

1. If a wrong answer is silent, write the test that makes it loud.
2. Prefer the data structure that cannot be misused to the rule that must be remembered.
3. Dropping information is safe; inventing it is not. Joins, narrowing and inference all
   resolve doubt toward the wider type.
4. Report an error only when sure the other checker would too.
5. Keep a deliberate non-decision written down, with the reason, so it is not reopened by
   accident.
