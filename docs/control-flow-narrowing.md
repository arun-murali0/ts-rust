# Control-Flow Narrowing

> **Architecture case study:** introducing flow-sensitive types while keeping declared types stable.

## 1. The problem

A variable can have a broad declared type but a narrower type on a particular control-flow path:

```ts
function f(x: string | null) {
    if (x !== null) {
        // x: string here
    }
}
```

Control-Flow Narrowing introduced a dedicated flow state instead of mutating the permanent symbol type.

Fixtures: `equality_null_narrowing.ts`, `truthy_narrowing.ts`, `typeof_narrowing_string_branch.ts`, `typeof_narrowing_else_branch_mismatch.ts`, `typeof_object_narrows_to_the_object_member.ts`, `typeof_function_narrows_to_the_function_member.ts`, `typeof_object_keeps_null.ts`, `nested_conditions_compose.ts`, `unknown_typeof_tag_narrows_nothing.ts`, `local_annotated_variable_is_registered.ts`, `local_annotated_variable_without_initializer_is_registered.ts`.

## 2. `SymbolTypeMap` versus `NarrowState`

The architecture separates two concepts:

```text
SymbolTypeMap
    = declared/inferred semantic type

NarrowState
    = current flow-sensitive override
```

`NarrowState` stores `(SymbolId, TypeId)` pairs. The use of semantic `SymbolId` means narrowing follows lexical identity rather than source spelling.

## 3. Why Oxc semantic symbols are consumed

Oxc already resolves identifiers to symbols/scopes. ts-rust consumes that result instead of rebuilding lexical binding analysis.

```text
IdentifierReference
       -> Oxc Scoping
       -> SymbolId
       -> NarrowState
```

This matches the project's dependency rule: reuse expensive front-end infrastructure, own TypeScript type semantics.

## 4. Branch states

`narrow_condition()` returns a pair of states:

```text
condition -> (true_state, false_state)
```

The statement checker saves the old state, applies the branch state, checks the branch, and restores the old state.

This is an **environment overlay** pattern. Flow information is temporary and path-specific.

The pair is computed from the overlay's current type for that symbol, falling back to the declared type only when the symbol has not been narrowed yet. That is what makes narrowing compose: an inner condition refines what the enclosing branch already established instead of restarting from the declared type.

```ts
if (typeof value !== "number") {  // value: string | null
    if (value !== null) {         // value: string, not string | number | null
    }
}
```

## 5. `typeof`, nullish, and truthiness

The narrowing layer contains dedicated logic for `typeof` tags, nullish checks, truthiness/falsiness, and union-member filtering. These operations work on semantic `TypeId` values, not AST syntax after the initial condition has been decoded.

The modelled `typeof` tags are `string`, `number`, `boolean`, `undefined`, `object`, and `function`. `null` answers to the `object` tag, as it does at runtime, so `typeof x === "object"` does not remove `null` from `x`.

A tag outside that set, `"symbol"` or `"bigint"`, would match no member of any union and would narrow the true branch to `never`, turning a gap in the type model into errors reported against the source. Such a condition narrows nothing instead.

## 6. Why not mutate the symbol type

Changing `SymbolTypeMap[x]` inside a branch would incorrectly make the narrow survive after the branch. The separate state layer prevents that semantic leak.

## 7. Conservative recovery

When a condition cannot be understood safely, the implementation prefers to retain the existing type rather than invent a false precise type. For a compiler, a conservative result is often safer than a wrong narrow that hides an error.

## 8. Why this design scales

The same state mechanism can later handle:

- `!`
- richer `&&` / `||`
- assignments
- early returns
- loops
- switch discrimination
- discriminated unions
- property and alias narrowing

No new global type environment is required for each operator.

## 9. What Control-Flow Narrowing completed

- symbol-aware flow state
- branch-local narrowing
- `typeof` narrowing
- nullish narrowing
- truthiness narrowing
- state save/restore
- preservation of declared types

The key lesson is: **flow semantics belong in a separate layer over the normal symbol/type environment.**
