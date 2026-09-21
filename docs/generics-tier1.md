# Generics Tier 1: Generic Functions

> **Status: current baseline.** This document records the first reusable generic-function and inference implementation. It is intentionally a working semantic reference while the generic model expands.


## 1. Scope as of this document

Generic type parameters are supported on **functions only**:
`function identity<T>(value: T): T`. Interfaces, type aliases, and classes can
declare `<T>` in their AST, but `resolve()` does not push their type parameters into
scope, and `TSTypeReference` resolution does not inspect `type_arguments` at all.
`Box<number>` and bare `Box` resolve identically today.

There are 13 regression fixtures in `tests/fixtures/generics-tier1/`.

## 2. One new type variant, not a parallel type system

```rust
GenericParameter(TypeParameterId, String, Option<TypeId>),
```

A type parameter is represented as an ordinary `Type` variant, the same enum that
already holds `Number`, `Array`, `Object`, and everything else. Nothing downstream
(subtyping, the arena, diagnostics) needs a special case for "is this a generic." A
`GenericParameter` is just a `TypeId` like any other until something specifically
pattern-matches on it during inference or substitution.

The `String` is the parameter's source name (`"T"`), kept for pattern-matching
readability and diagnostics. It is not used as an equality key anywhere.

The `Option<TypeId>` is the parameter's own `extends` bound, resolved once when
the `GenericParameter` is first created and reused from the same
`type_param_cache` as everything else about this parameter — `None` means
fully unconstrained. It's what makes the constraint checking in §8 possible:
without it, a `GenericParameter` would carry no information about what it's
allowed to be, and enforcing `T extends { length: number }` would need some
separate side-table keyed on `TypeParameterId` instead of living on the type
itself.

## 3. Identity: `TypeParameterId`

A type parameter's identity is `TypeParameterId`, derived from where it is written in
the source file, not from an arena slot and not from an AST pointer:

```rust
pub struct TypeParameterId {
    declaration_span_start: u32,
    parameter_index: u32,
}
```

`declaration_span_start` is the byte offset of the individual `TSTypeParameter`
node's own span, and `parameter_index` is its ordinal position among sibling
parameters. Two type parameters in the same or different declarations can only share
a `TypeParameterId` if they are, in fact, the same declared parameter.

This replaced an earlier version that cached identity by the `TSTypeParameter` AST
node's raw pointer address. The pointer version worked for the same reason this
version does (a stable key across the declare pass and the check pass, which both
call `push_type_params` independently against the same AST), but tied identity to one
process's memory layout rather than to a property of the source text. A source-derived
key is stable across separate parses of unchanged source, which a pointer is not
guaranteed to be beyond the one parse that produced it.

```text
enter function<T>(...)
       -> push_type_params
              -> TypeParameterId::new(T's own span start, index 0)
              -> namespace["T"] = GenericParameter(id, "T") at a cached TypeId
       -> resolve params/return type/body against that namespace
       -> pop_type_params
              -> namespace["T"] restored to whatever it pointed to before
```

The cache itself is keyed on `TypeParameterId`, not on the pointer:

```rust
let id = TypeParameterId::new(param.span().start, index as u32);
let type_id = *self
    .type_param_cache
    .entry(id)
    .or_insert_with(|| arena.alloc(Type::GenericParameter(id, name.clone())));
```

## 4. Inference is structural, not declarative

At a call site, nothing is told which argument corresponds to which type parameter.
`infer_type_param_bindings` walks the declared parameter type and the actual argument
type in lockstep, and records a binding the first time it reaches a
`GenericParameter` leaf, keyed on `TypeParameterId`:

```text
declared param type          argument type
Array<T>              <->    Array<number>
   |                            |
   v                            v
 T (id)                        number
       binding: id -> number
```

The walk recurses through arrays, object properties, and function parameter/return
positions. A type parameter that never appears in a reachable position is left
unbound.

Only the first binding for a given identity is kept; a second argument that also
resolves to the same type parameter is checked against the already-bound type rather
than contributing a new candidate.

## 5. Substitution rebuilds the return type, not the declaration

Once bindings are known, the return type is rebuilt with every `GenericParameter`
replaced, matched by `TypeParameterId`:

```text
return type: T (id)
bindings: [(id, number)]
      -> substituted: number
```

`substitute_type_params` walks the same shapes `infer_type_param_bindings` reads and
allocates a fresh, substituted copy. The function's own stored signature is never
touched, so the same generic function can be called with different argument types
without the declaration drifting.

`contains_type_param` short-circuits this rebuild for the ordinary, non-generic case:
if a type contains no `GenericParameter` anywhere, substitution returns it unchanged
without walking or reallocating anything.

## 6. An unbound parameter becomes `unknown`, not `any`

If a type parameter is never reached during argument inference, substitution falls
back to `unknown` rather than `any`, so a failed inference cannot silently suppress
further checking at the call site.

## 7. Multi-candidate inference resolves through subtyping

A second argument resolving to an already-bound type parameter no longer keeps only
the first binding and rejects the rest. Checked against real TypeScript behavior
first: `pair<T>(a: T, b: T)` called as `pair(1, "x")` is a genuine TypeScript error,
not an inferred `T = number | string`, so this does not union candidates. Instead
each new candidate is resolved against the existing binding through ordinary
subtyping: if one is a supertype of the other, the binding widens to the supertype
(`pick(dog, animal)` infers `T = Animal`); if neither is, the binding is left alone
and the real mismatch is still caught by the ordinary per-argument assignability
check, unchanged from before.

## 8. Constraints (`T extends X`)

A type parameter's own `extends` bound is now resolved and enforced for generic
functions. `function f<T extends { length: number }>(value: T)` checks that
whatever `T` is inferred as at each call site actually satisfies `{ length: number
}`, using the same structural assignability check every other type relationship in
this checker goes through.

A constrained type parameter's own members are also usable inside the function's
body: `value.length` type-checks against the constraint's shape, since a
constraint is the one guarantee the checker actually has about what `T` could be.
An *unconstrained* type parameter still has no members at all, matching before.

Not implemented: defaults (`T = X`), and constraints on generic interfaces,
aliases, or classes, since those declaration kinds are not generic at all yet (see
Section 1).

## 9. What is still not covered

- Generic interfaces, type aliases, and classes (see Section 1).
- Explicit call-site type arguments (`identity<string>(x)`).
- Defaults (`T = X`).
- Instantiation memoization. Deliberately not built yet rather than built without a
  case that exercises it: every generic function call already computes its own
  bindings fresh from its own arguments, which is not wasteful repetition, and the
  scenario that would actually justify a cache (the same `Box<number>` reference
  appearing many times in one file) does not exist until generic interfaces and
  classes do.
- A generics-specific recursion guard for self-referential generic types. Checked
  as unnecessary for now: the existing circular-resolution guard in namespace.rs
  already prevents a self-referential generic declaration from resolving at all
  (failing safely, not hanging), and substitution only ever walks an
  already-resolved, therefore acyclic, type tree.
- Construction-site inference (`new Box(5)` inferring `Box<number>`), which does not
  apply yet since generic classes are not supported at all.

These are tracked as follow-up work, not regressions from a prior version.
