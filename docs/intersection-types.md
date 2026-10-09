# Intersection Types

> **Architecture case study:** adding `A & B` to a checker whose types are shared, hash-consed and sometimes unresolved, without letting a reduction read a body that does not exist yet.

## 1. Why this is its own milestone

An intersection touches four things at once: interning (a node whose identity is a list), unresolved declarations (a `Ref` reads as an empty object until its members are resolved), generics (substituting a type parameter can make an intersection reducible), and the stable hash planned for stage 2. The rules were fixed first, in LLD 1.13 and decisions 10 to 12, and checked against tsc 5.9.3. This milestone is the code that follows them.

There are 25 unit tests in `src/arena.rs` (one per construction rule and for the lazy reduction), 10 in `src/subtyping.rs` (relations), and 17 fixtures in `tests/fixtures/intersection-types/` with `tests/intersection_types.rs`. Each fixture was run through tsc first, and each reports what tsc reports, with the same code and the same message.

## 2. Two levels, and only the first is identity

```text
alloc_intersection   ids and Named/App wrappers only; never a Ref's body; part of identity
intersection_reduces_to_never   reads bodies; asked on demand; cached; never part of identity
```

**Construction** runs while declarations are still being resolved. Whatever it decides is interned under an id that nothing invalidates, so it may only use what an id and its wrappers show. In order: `never` ends it; a nested intersection (also one seen through an alias) is spread into its members; `unknown` is dropped; the error type, then `any`, absorb; primitives and literals cancel or collapse; a union member is distributed in written order, each choice built by this same function, with a cap of 100,000 combinations; an anonymous `{}` is dropped next to an object-like member; duplicates are removed by id. Fewer than two members left means the member itself, or `unknown` for none.

**Reduction** is the part that needs a body. `{ kind: "a" } & { kind: "b" }` has no values, but deciding that means reading the members' properties, which a `Ref` may not have yet. So it stays an intersection node and is reduced when a relation, a member access or the printer asks, the way tsc does it (it prints as `never` while its flags still say intersection). The answer is cached per id and the cache is emptied by `resolve_ref`, so an answer from before a declaration was resolved is asked again.

## 3. Identity is the written order

`A & B` and `B & A` are two ids that are assignable both ways. The members are never sorted, because call signatures are tried in member order and callability needs a body. The cost is stated in LLD 1.13 rule 4: reordering the members of a written intersection changes its stable hash, which is an extra recheck and never a stale result.

Distribution has one subtlety that came from the existing union. `alloc_union` stores its members in the reverse of the order it is given, so distributing a union and rebuilding it would flip its stored order and give a different id for the same type. The results are handed back reversed, so `("a" | "b" | "c") & string` is the id of `"a" | "b" | "c"`. `an_alias_of_an_intersection_is_flattened_like_the_intersection_itself` and `distribution_reduces_each_choice` protect it.

## 4. Where an intersection is used

- **Relations.** A target intersection needs every member. A source intersection needs some member, or, for an object target, members whose properties together satisfy it: a property is required if any member requires it. Against a union both directions are tried. An intersection that reduces to `never` is a subtype of everything.
- **Member access.** The members that have the property all contribute, and the type is the intersection of what they give, so `{ x: number } & { x: string }` has `x: never`. One member is enough for the property to exist.
- **Calls.** Through `F1 & F2` the first function member that accepts the arguments is used. Argument types are inferred once and what that reported is dropped; the ordinary check of the member picked then reports them again, so nothing is reported twice. With no accepting member the first is used, and its errors are the ones reported.
- **Generics.** Substitution rebuilds through `alloc_intersection`, so `T & number` with `T := string` is `never`. Inference goes into each member from the argument, which is what tsc does.
- **Annotations.** `A & B` resolves through `alloc_intersection`, and a name that does not exist inside one is reported.
- **Display.** Members print in written order, a function member is parenthesized, an array of an intersection is parenthesized, and a reduced one prints as `never`.

## 5. The `Intersection` variant is last in the enum

Adding the variant after `Union` changed `benchmark_fixtures_report_the_expected_number_of_diagnostics` from 5 to 1, with either of two independent implementations, and moving it to the end restored 5. The four diagnostics that disappear are `Property 'payloadNN' does not exist on type 'never'`, which tsc does not report on that source. Something depends on the position of the variants, and it is not found. So the variant sits at the end with a comment saying why, and this is an open finding, not a fix: the false positives are still there.

## 6. What is still not done

- **TS2590.** A distribution that is too large becomes the error type, silently. There is no way to report from the annotation resolver.
- **Parenthesized types.** `(() => void) & A` does not resolve, because no parenthesized type resolves at all, intersection or not. Fixtures use aliases for function types.
- **Names in alias bodies.** An unknown name inside `type X = A & Missing` is not reported, for unions either. A variable annotation is reported.
- **Overload errors.** A call that no member accepts reports the first member's error, where tsc reports TS2769.
- **Narrowing.** `typeof`, `in` and discriminant narrowing do not look into intersections.
- **`T & {}`** for `NonNullable<T>`, enum literal members in an intersection, and `void & undefined` have unit tests but no fixture.

## 7. What Intersection Types completed

- the `Intersection` node and `alloc_intersection` with every construction rule
- lazy, cached reduction of discriminant conflicts
- relations, member access, calls, inference and substitution through intersections
- annotation support and display that matches tsc
- 52 tests, and 17 fixtures checked against tsc

The architecture lesson is: **a reduction that reads a body belongs where the body is guaranteed to exist, and a canonical form belongs where it cannot be wrong.**
