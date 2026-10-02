# The type arena: design and case study

This document explains how the type arena in `src/arena.rs` is built, why it is built
that way, how it changed over time, and what each change measurably did. It is written
as a case study, so it includes the changes that did not help and the estimates that
turned out wrong, because those decided what we did next.

Related files: `src/arena.rs`, `src/types.rs`, `src/subtyping.rs`,
`src/semantic/queries.rs`, `src/semantic/generics.rs`, `src/namespace.rs`,
`src/type_annotation.rs`.

## 1. What the arena is for

The checker needs one place that owns every type it creates. Types refer to each other
constantly: an array holds an element type, an object holds property types, a union holds
members, a function holds parameter and return types. Owning them in one place, and
referring to them by a small id, gives us three things:

- Cheap copies. A `TypeId` is a `u32`, so passing a type around costs nothing.
- Recursive types. A type can name itself by id without an owner cycle.
- One identity per type, which the caches and, later, cross-file resolution depend on.

## 2. Core design

### 2.1 Ids into a vector

```rust
pub struct TypeId(u32);

pub struct TypeArena {
    types: Vec<Type>,          // the source of truth
    display_names: FxHashMap<TypeId, String>,
    record_value_types: FxHashMap<TypeId, TypeId>,
    interned: FxHashMap<u64, TypeId>,
    interned_unions: FxHashMap<u64, TypeId>,
}
```

A `TypeId` is an index into `types`. Everything else on the struct is an index or a side
table that answers a question about ids. `types` is always the authority: if a side
table and `types` ever disagree, `types` wins.

### 2.2 Shallow types

`Type` is an enum (primitives, `Function`, `Object`, `Array`, `Union`, three literal
kinds, `GenericParameter`, `Never`, `Void`, `Any`, `Unknown`, `Error`). A composite
stores the `TypeId`s of its parts, never the parts themselves. `Hash` and `Eq` on `Type`
therefore compare ids, not structure, and never walk the arena.

This is the property the rest of the design leans on. It makes hashing a type cost a
handful of integer operations, and it makes "are these two the same slot" a single
comparison.

The price is that derived equality cannot answer "are these the same type". Two
identical shapes built at two sites hold different ids for their parts, so they compare
unequal. That is why `structurally_equal` exists separately (section 2.7).

### 2.3 Hash-consing anonymous composites

`alloc(ty)` looks for an existing slot with identical content and returns its id. If none
exists it pushes a new slot. This is hash-consing.

Before this, every `alloc` pushed a new slot, so `{ a: number }` written in two places
became two ids. Anything that needed "same type" had to call `structurally_equal`, which
walks both graphs recursively and caches nothing. The subtype cache, keyed on
`(TypeId, TypeId)`, could never hit for two identical shapes built separately.

We did this in the arena, and did it early, for a reason beyond speed. Module resolution
needs a type reached through two import paths to be the same type, and the arena is
meant to be shared per project rather than per file. Doing deduplication in the arena
while it was still small was cheaper than doing it after cross-file code depended on ids
being unique.

What is interned:

| Type | Interned | Note |
|---|---|---|
| Object, Array, Function | yes | Object properties are kept sorted, so key order never splits a shape |
| String, Number, Boolean literal | yes | Number literals compare by bit pattern, so `Eq` and `Hash` agree |
| GenericParameter | yes | Its key includes its declaration identity, so a `T` from one declaration never merges with another |
| Union | yes, own table | Needs flattening and dedup first, see section 2.6 |
| The ten primitives | no | Fixed slots 0 to 9, allocated once in `new()` |
| Object placeholders | no | See section 2.5 |
| Anything from `alloc_fresh` | no | Reserved for types with an identity beyond their shape |

### 2.4 Named types stay unique

Sharing ids creates a hazard. A display name is attached to an id, so naming a shared id
would rename every identical type in the file. `type Alias = Box<number>` must not make
every other `Box<number>` print as `Alias`.

The rule is that anything that receives a name must be unique to its holder:

- `alloc_fresh` always pushes a new slot and never enters an intern table.
- `make_unique(id)` returns the id unchanged if it is already unique, or a fresh copy if
  it is shared.
- `set_display_name`, `set_record_value_type` and `set` assert in debug builds that the
  id is not shared.

Display names live in a side table keyed by id, not on `Type`. That keeps display a
concern of `display_type` alone: no `match` over `Type` elsewhere has to carry a name.
The ten primitive slots never take a name, because they are shared by every use of
`number`, `string` and so on.

Checking "is this id shared" is done by looking the content up in the intern table and
comparing ids, not by keeping a flag per slot. A flag would have to be kept correct
through every code path that creates or completes a slot.

### 2.5 Placeholders for recursive types

`interface Node { next: Node }` needs a type that refers to itself before it is
finished. The arena hands out a placeholder: an empty object slot from
`alloc_object_placeholder`. The self reference resolves to that id by identity, and
`set(id, ty)` completes the same slot afterwards.

Two rules keep this safe. A placeholder is pushed directly and never interned, otherwise
every empty object and every other placeholder would merge with it and one `set` would
overwrite all of them. And a completed placeholder stays out of the intern table, because
other types already hold its raw id and it could never be merged retroactively with an
identical shape.

### 2.6 Unions

A union cannot go through `alloc`. Its members must be flattened (nested unions
opened), `never` dropped, and duplicates removed first, and only then does the union have
an identity. `alloc_union` does that, then looks the finished member list up in a
separate table, `interned_unions`.

The key keeps member order. Sorting the key would also merge `A | B` with `B | A`, but
the survivor would be whichever was built first, and a later diagnostic could print its
members in a different order than before. That could not be judged without running the
message fixtures, so the behavior-neutral key went in first. Sorting is left open (see
section 8).

### 2.7 Structural equality, separately

`structurally_equal(a, b)` answers "same type by shape". It follows ids through the arena
and is used by `alloc_union` to remove duplicate members. Three rules matter:

- It must terminate on recursive types. A completed placeholder refers to its own id, so
  comparing two identical recursive interfaces used to loop until the stack overflowed.
  It now keeps the object pairs being compared and treats a repeat as equal, the same
  coinductive rule the subtype check uses.
- It compares the method flag, matching the intern key, so `alloc_union` cannot merge
  two members that `alloc` keeps apart.
- A generic parameter is equal only to itself. Identity is the one thing shape must not
  override.

### 2.8 Object properties are sorted

`ObjectType::new` sorts properties by name. Subtyping compares two objects with a
merge-join over their property lists, which is only correct on sorted input, and sorting
at construction means property order can never split two otherwise identical shapes in
the intern table. A debug assertion in the merge-join fails loudly on unsorted input
instead of answering wrongly.

## 3. How a type moves through the checker

```
source text
   |
   v
declare pass: register names, allocate placeholders for named types
   |
   v
resolve annotation --> TypeNamespace::resolve --> arena.alloc / alloc_union
   |                        |                          |
   |                        |                          +--> interned slot, or a new one
   |                        +--> generic reference: substitute, name, memoize
   v
check pass: expressions produce TypeIds, is_assignable asks SemanticQueries
   |
   v
SemanticQueries --> CheckContext subtype cache keyed on (TypeId, TypeId)
   |
   v
subtyping::is_subtype (structural walk, coinductive on cycles)
```

Two things sit above the arena and depend on its ids being stable.

The subtype cache lives in `CheckContext` and is keyed on `(TypeId, TypeId)`.
`SemanticQueries` is the one place allowed to call the subtype check directly, so "did
this go through the cache" has one place to look. Only the outermost question is cached.
Results from inside the recursion are not, because they were computed under an assumption
("a pair already being compared is assumed true") that is only valid within that one
query.

The instantiation memo in `TypeNamespace` reuses finished generic instantiations such as
`Box<number>` (section 5, phase 4).

## 4. Timeline

| When | Change | Why |
|---|---|---|
| 2026-09-16 | Split the checker into feature files | Room to grow |
| 2026-09-21 | Sorted object types, structural equality | An enum declared out of order failed a structural assignment, and unions kept redundant members |
| 2026-09-26 | Display-name side table | Diagnostics should print a name, not a shape |
| 2026-09-26 | Placeholders for recursive types | `interface Node { next: Node }` used to fail with a circular error |
| 2026-09-27 | `Record<K, V>` resolution | Stopped blocking the containing type |
| 2026-09-28 | Hash-consing of anonymous composites | One identity per shape, base for cross-file resolution |
| 2026-09-28 | Fix structural equality on recursive types | Stack overflow on `A \| B` of identical recursive interfaces |
| 2026-09-28 | Performance phases 1 to 5 | Section 5 |

## 5. Case study: five performance phases

We worked one phase at a time. Each phase was one small change with a stated reason, a
measurement, and a verdict. We deliberately did not stack changes, so that any movement
in the numbers could be attributed.

### Method

- Fixtures: `benches/support/complex_fixtures.rs`. Workloads range from one tiny file to
  a 1,000 function file, wide unions, deep nesting, class hierarchies, generic calls,
  destructuring, and mixed applications. A generic type reference fixture was added in
  phase 4.
- Instruction counts: iai-callgrind. These are deterministic, so they are the number we
  quote.
- Allocations: the `dhat_heap` example, in release, reporting blocks, bytes and peak.
- Correctness: `cargo test`, and every workload's diagnostic count. The counts must not
  change: 0, 100, 0, 0, 1, 0, 401, 110, 510, 1010, plus 0 for the added fixture.

### Phase 1: keep trivial subtype pairs out of the cache

Problem. Once a file reports one error, `Error` flows through every expression built on
it, and ordinary code asks "is number assignable to number" constantly. Each such
question paid for a hash lookup and a map entry that could never save work, because the
answer needs no structural walk.

Change. `SemanticQueries::is_subtype` answers identical ids and `Any` or `Error`
operands before touching the cache. The rule is one function, `is_trivial_subtype`,
shared with the real subtype check, so the fast path cannot drift from it. A bypass that
differed from the real relation would make cached and uncached runs disagree, and that
only shows up as flaky diagnostics.

Result (iai, debug, instructions): large_1000 -1.32%, realistic_50 -0.85%,
generic_calls_500 -0.62%, connected_application -0.45% to -0.51%, class_hierarchy
-0.39%, wide_union -0.26%, tiny +0.20%.

What we got wrong. We expected the error-heavy workloads to gain most. They gained
least. The gain came from identical-id pairs, which are far more common than we assumed.

Verdict. Small, safe, kept.

### Phase 2: intern unions

Problem. Unions were the one composite the intern table skipped, so the same `A | B`
built twice got two ids and the cache key never repeated.

Change. `alloc_union` looks its finished member list up in `interned_unions`.
`is_interned` had to learn about unions too. Without that, `make_unique` would hand back
a shared union and naming `type Status = "a" | "b"` would rename every identical union,
silently in release builds where the guards are debug-only.

Result: neutral. wide_union_50 +0.51% (worse), connected_application -0.33% to -0.45%,
everything else within 0.02%.

What we got wrong. We expected the union-heavy fixture to improve. It got slightly worse:
the extra lookup and a second copy of each member list cost more than the hits saved,
because the unions in that fixture do not repeat.

Verdict. Kept as groundwork for cross-file identity, not as a speedup. The commit message
says so.

### Phase 3: route generic inference through the cache

Problem. Inference compares each new candidate for a type parameter against its existing
binding, in both directions. A call like `allSame(1, 2, ..., 8)` asks the same pairs
repeatedly, and those went straight to the subtype check.

Change. The cache is threaded through the inference recursion. Inference builds a
`SemanticQueries` per question instead of holding one, because inference allocates in
the arena and a long-lived `SemanticQueries` would keep a shared borrow of it.

One call site is deliberately left out: the type argument constraint check. It runs while
a declaration is still being resolved, when a placeholder can be empty, and the cache is
keyed on `TypeId` alone. A result stored against an empty placeholder would survive
`set()` filling it in, and nothing invalidates it.

Result: no change except connected_application -0.43% to -0.46%.

Verdict. A consistency change more than a speedup: repeated candidates are rare in the
fixtures.

### Phase 4: reuse finished generic instantiations

Problem. Every reference to `Box<number>` redid the substitution and rebuilt the display
name. No fixture wrote a generic type reference, so nothing measured it, and a fixture
was added first.

Change. The pair (generic declaration, resolved arguments) maps to a stored copy, and a
repeat reference takes a duplicate of it. Arity and constraint diagnostics still run for
every reference at that reference's own span. Only the substitution and naming are reused.

Three rules keep it from changing behavior:

- A hit is a duplicate, never the stored slot. Callers rename what they receive, and
  sharing one slot would rename every other `Box<number>`.
- Nothing is stored when substitution left the shape unchanged. That means the generic is
  still an empty placeholder, and remembering it would return the empty shape after it
  is filled in.
- Reuse requires every argument's display to be settled (`has_settled_display`). An
  unfinished type prints as an empty object, and reusing that spelling would turn
  `Box<Node>` into `Box<{}>` in a message.

Result: release dhat shows about 2.4% fewer allocation blocks on connected_application,
measured against the run before phase 1, so it includes phases 1 to 3. The instruction
comparison was lost when the project moved to a new compiler between runs (section 6).

Verdict. Done and tested, benefit not isolated. The A/B needed to settle it is listed in
section 8.

### Phase 5: key the intern tables on a digest

Problem. The intern tables owned a second copy of every interned type as their key. Each
composite lived in memory twice, and every miss paid to clone it, including the clone of
an object's property list.

Change. The tables key on a 64-bit digest of the content. A hit is confirmed by
comparing the slot in `types` with the incoming type before it is reused. If two different
types ever share a digest, the second is left out of the table and gets a slot of its own.
That costs one missed reuse and can never merge two types that differ.

Result (rustc 1.98.1, against the previous run):

| Workload | Instructions |
|---|---|
| class_hierarchy_50 | -15.2% |
| complex_mixed_200 | -14.3% |
| complex_mixed_50 | -7.1% |
| nested_objects_50 | -3.6% |
| wide_union_50 | -1.9% |
| generic_type_refs_500 | -1.0% |
| connected_application | -0.2% to -0.4% |
| realistic_50 | +0.15% |
| large_1000 | +0.08% |
| destructuring_500 | unchanged |

| dhat | Before | After |
|---|---|---|
| Peak heap | 8,689,739 bytes | 7,506,138 bytes (-13.6%) |
| Blocks at peak | 6,703 | 5,895 |
| Total allocated | 33.16 MB | 31.51 MB (-5.0%) |
| class_hierarchy bytes | 642,274 | 572,986 (-10.8%) |
| complex_mixed_200 bytes | 13,095,551 | 11,846,505 (-9.5%) |
| generic_type_refs bytes | 1,644,005 | 1,510,976 (-8.1%) |

What we got wrong. Before measuring we estimated the saving at well under 1% of peak,
because the profile suggested peak heap was mostly parser and semantic data. The real
figure was 13.6%. The duplicated data included each object's property list, so any
workload with many distinct object types paid on nearly every type.

Verdict. The only phase with a large effect, and the one that removed per-type work
everywhere rather than trying to avoid an occasional recomputation.

### Cumulative picture

Through phase 4, on the older compiler, instruction counts against the first baseline
moved by between -1.3% and +0.3% depending on the workload: connected_application about
-1.3%, large_1000 -1.3%, realistic_50 -0.8%, generic_calls -0.6%, class_hierarchy and
complex_mixed -0.3% to -0.4%, wide_union +0.25%, tiny +0.33%. Phase 5 then removed 14%
to 15% of instructions on the type-heavy workloads on top of that. Excluding the added
fixture, total allocation fell about 4.9% in bytes and 2.1% in blocks, and peak heap fell
13.6%.

## 6. Measurement pitfalls we hit

- The compiler changed mid-project (rustc 1.95 to 1.98.1). Every absolute iai number
  shifted, for example `tiny` from 86,101 to 93,581 instructions, with no source change.
  Comparisons only mean something inside one toolchain, and this is why the phase 4
  instruction result was lost.
- Criterion deltas against an earlier run are unreliable on this machine. The
  `parse_and_bind_only` case, which no phase touched, showed +40% in one run. Ratios
  within a single run are usable.
- Debug builds overstate the checker's own cost by roughly an order of magnitude, and
  they run the debug assertions. Quote release numbers where possible.
- iai reports LL hit swings of a few percent with identical instruction counts. That is
  allocation layout, not a change in work.
- A profile taken over all workloads at once hides which one matters. Recursion also
  inflates inclusive percentages in callgrind, so the inclusive column cannot be added up.

## 7. What we would do differently

- Profile a type-heavy workload before choosing phases. The first four phases were
  guesses aimed at caches, and the checker was about 1% of the time on the fixture we
  looked at first (parse and bind was 3.64 ms of a 3.67 ms check on large_1000). The
  phase that paid off removed duplicated work in the arena itself.
- Add the fixture before the change. Phase 4 had nothing to measure against until a
  generic type reference fixture was written.
- Record the toolchain next to every baseline, and take before and after on the same one.
- Write the expected effect down before measuring. Two of our estimates were wrong in
  opposite directions (phase 2 too high, phase 5 too low), and stating them first is what
  made that visible.

## 8. Open items and limits

- Sorting the union key so `A | B` and `B | A` share an id. Needs the message fixtures
  run to confirm no diagnostic changes member order, and a benchmark showing the hit rate
  is worth it.
- The phase 4 A/B on a single toolchain: check out the earlier `arena.rs`, `namespace.rs`
  and `type_annotation.rs`, run iai, restore, run again.
- Two untried speed levers outside the arena: the bench build profile (`opt-level = "s"`
  versus 3) and the cost of Oxc's control-flow graph, which only unreachable code
  detection uses.
- Known edge in the instantiation memo: the names of arguments are baked in at first use,
  so an argument renamed later keeps its old text in reused instantiations.
- `structurally_equal_cached` is not connected to `alloc_union`. Connecting it changed
  diagnostic counts once and the cause was not established, so the uncached form stays
  until it is. See section 11 for what was ported alongside it.

## 9. Invariants to keep

These are the rules the design depends on. Break one and the failure is usually silent.

1. `types` is the source of truth. Every side table is an index over it.
2. Anything that receives a display name, a record value type, or a `set` must be unique
   to its holder. Get it from `alloc_fresh`, `alloc_object_placeholder` or `make_unique`.
3. A digest match is a hint. Confirm against the slot before reuse.
4. Placeholders and completed placeholders never enter an intern table.
5. The relation cache is scoped to an arena generation (section 11), so an answer from
   before a placeholder was filled in, or before the arena was cleared, cannot be read
   back. Resolution still bypasses the cache; the generation check would make caching
   there safe, it just has not been needed. A growth policy for the intern tables is still
   open.
6. Fast paths in front of a cache must use the same rule as the real check.

## 10. Reproducing the measurements

```bash
# instruction counts, compares against the previous run
cargo bench --bench checker_iai

# allocation counts, release build
cargo run --release --example dhat_heap

# wall clock, noisy, read ratios within one run only
cargo bench --bench checker_benchmark
```

Keep one toolchain for a comparison, do not run `cargo clean` between the two iai runs,
and check that every workload's diagnostic count is unchanged.

## 11. Reuse across files: generation, `clear`, sessions

Everything above treats one arena as living for one file. Checking the same file again, as
an editor does, can reuse the allocation, but only if nothing from the previous check can
be seen by the next one.

**`clear()` returns to the primitive baseline.** It truncates the type vector to the ten
fixed primitive slots, so `number()` and the rest return the same ids as in a fresh arena,
and keeps the vector's allocation. Every side table is cleared with it: display names,
record value types, both intern tables, the parameter-scan memo and the equality cache.
A table that survived would let the next file see a stale name or a stale intern entry,
which fails silently.

**A generation counter instead of an invalidation rule.** `generation` advances whenever
an id that already exists changes meaning: `set()` filling in a placeholder, and
`clear()`. New ids do not advance it, since an id nobody has asked about has no stale
answer. `RelationCache` remembers the generation it was filled under, and
`SemanticQueries::new` drops every entry from an older one. The alternative, telling each
caller "invalidate after `set`", puts a rule in every call site that has to be remembered
forever; comparing one integer makes the cache correct by construction. It also lifts the
old restriction that no query may run while a placeholder is empty.

**`CheckSession` is the explicit mutable boundary.** The reusable state lives in a session,
not inside `TypeChecker`, so the checker keeps no interior mutability and independent
sessions can run side by side, one per file. The arena is moved into each check and moved
back at the end, with its capacity.

**Recursion guards are `SmallVec`s.** The pair stacks used by structural equality,
subtyping and inference, and the visited lists in the generic helpers, hold sixteen or
eight entries inline. Real comparisons are shallow, so they almost never reach the heap,
and a lookup over so few entries is a handful of integer compares, cheaper than hashing.
A pathologically deep type still works: the vector moves to the heap. A test builds a chain
forty levels deep to cover that path.

**The instantiation memo is keyed on a fingerprint.** `(generic shape, 64-bit fingerprint
of the bindings)` means a lookup hashes two integers and never allocates a `Vec` to ask.
The fingerprint only picks a bucket; a hit is confirmed by comparing the stored bindings, so
two different binding lists that collide can never be mistaken for each other.

**Counters, not guesses.** `TypeArena::stats`, `RelationCache::stats` and
`TypeNamespace::stats` report sizes and hit counts, and a session keeps the last check's
copy. They are plain counts of structures the code already maintains, so asking costs
nothing. No performance figures are claimed for this section: the counters make a
regression observable, and the numbers belong to the next benchmark run.
