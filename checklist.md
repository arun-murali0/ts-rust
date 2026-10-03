# TypeScript Type System — Full Coverage Checklist

Complete semantic surface of the TypeScript type system (language types + checking rules).  
Use for any checker aiming at `tsc`-level type coverage.  
Marks below reflect **ts-rust today** (`[x]` done, `[~]` partial, `[ ]` missing); strip marks for a blank tracker.

---

## 1. Core types

### 1.1 Top / bottom / special
- [x] `any`
- [x] `unknown`
- [x] `never`
- [x] `void`
- [ ] `object` (non-primitive)
- [x] `null`
- [x] `undefined`
- [~] Intrinsic string types (`Uppercase`, `Lowercase`, `Capitalize`, `Uncapitalize`)

### 1.2 Primitives
- [x] `number`
- [x] `string`
- [x] `boolean`
- [ ] `bigint`
- [ ] `symbol`
- [ ] Unique symbols

### 1.3 Literals
- [x] String literals
- [x] Numeric literals
- [x] Boolean literals
- [ ] Bigint literals
- [ ] Template literal *values* vs template literal *types* (see §8)
- [x] Literal widening
- [ ] `as const` / const assertions
- [ ] Enum member literals (full)

### 1.4 Composite
- [x] Object type literals
- [x] Array types (`T[]` / `Array<T>`)
- [ ] Readonly arrays (`readonly T[]` / `ReadonlyArray<T>` as distinct)
- [ ] Tuple types
- [ ] Readonly tuples
- [ ] Optional tuple elements
- [ ] Rest tuple elements
- [ ] Variadic tuples
- [ ] Labeled tuple elements
- [x] Union types
- [ ] Union reduction / subsumption (beyond flatten + dedupe)
- [ ] Intersection types
- [ ] Intersection reduction
- [ ] `keyof T`
- [ ] Indexed access `T[K]`
- [ ] Non-null assertion type effect (`!`) — expression exists; full model `[~]`

---

## 2. Type declarations & names

- [x] Type aliases
- [x] Interfaces
- [x] Interface extension (`extends`)
- [x] Interface declaration merging (same file)
- [x] Duplicate type declarations reported (alias, class and enum collisions; interface and class merges are accepted)
- [ ] Class and interface merging, enum merging (accepted without error, but the later declaration replaces the earlier one)
- [ ] Interface merging across files / packages
- [x] Classes as types (instance type)
- [ ] Class static side vs instance side (full)
- [ ] Abstract classes / members
- [ ] Ambient types (`declare`)
- [ ] Namespace types / value-type dual
- [ ] Enum types (numeric / string / const / heterogeneous) — basic `[~]`
- [ ] Const enums
- [ ] Import/export of types
- [ ] `import type` / `export type`
- [ ] Type-only vs value space separation (full)

---

## 3. Object members & signatures

### 3.1 Properties
- [x] Required properties
- [x] Optional properties (`?`)
- [ ] Readonly properties
- [ ] Property renaming / excess property checking (full freshness rules) `[~]`
- [ ] Private / protected fields (type checking)
- [ ] `#private` fields (type side)
- [x] Parameter properties in constructors (`public` / `private` / `protected` / `readonly` parameters become instance properties)
- [ ] Auto-accessors (type side)

### 3.2 Index signatures
- [ ] String index signatures
- [ ] Numeric index signatures
- [ ] Template-string index signatures
- [ ] Index signature compatibility rules
- [ ] Mixing index signatures with declared properties

### 3.3 Call / construct
- [x] Call signatures (function types)
- [x] Construct signatures (`new`) — basic
- [ ] Multiple call signatures (overloads)
- [ ] Multiple construct signatures
- [ ] Hybrid types (call + properties)

### 3.4 Methods & accessors
- [x] Methods vs function-typed properties (bivariance) `[~]`
- [ ] Getters / setters (types)
- [ ] Method overloads

---

## 4. Functions

- [x] Parameter types
- [x] Return types
- [x] Optional parameters
- [x] Rest parameters
- [ ] Default parameter type interaction
- [x] Function assignability (contravariant params / covariant return) `[~]`
- [ ] `strictFunctionTypes` full matrix
- [ ] Overload declarations
- [ ] Overload implementation checking
- [ ] Overload resolution (candidates, specificity)
- [ ] Generic call signatures
- [ ] `this` parameter types
- [ ] Contextual `this`
- [ ] Void-return special cases (full `tsc` leniency)
- [ ] Promise-returning / `async` function types
- [ ] Generator function types
- [ ] Arrow vs function variance / `this` differences

---

## 5. Classes

- [x] Instance properties / methods
- [x] Constructors
- [x] Inheritance (`extends`)
- [x] Structural compatibility of class instances
- [ ] `implements`
- [ ] `abstract`
- [ ] `static` members (full type model)
- [ ] Class static blocks (type effects)
- [ ] Generic classes `[x]` (ts-rust)
- [ ] Generic inheritance constraints
- [ ] Decorators (type side, stage 3 / legacy)
- [ ] Mixins (constructor type patterns)
- [ ] `this` type in class methods
- [ ] Polymorphic `this`

---

## 6. Generics

### 6.1 Parameters & application
- [x] Type parameters on functions
- [x] Type parameters on interfaces / aliases
- [x] Type parameters on classes
- [x] Constraints (`extends`)
- [x] Default type arguments
- [x] Explicit type arguments at calls / references
- [x] Arity checking
- [ ] `const` type parameters
- [ ] Variance modifiers (`in` / `out`) on type parameters

### 6.2 Inference
- [x] Inference from arguments
- [x] Multi-candidate inference (basic widen)
- [x] Literal-preserving inference under a primitive or literal bound
- [~] Inference from union positions
- [ ] Inference from return positions (full)
- [ ] Inference priority / multiple candidates (full algorithm)
- [ ] Contextual inference into generic calls
- [ ] Higher-order function inference
- [ ] Inference involving `infer` (see conditionals)
- [ ] Fixing / speculative inference

### 6.3 Instantiation
- [x] Substitution into structure
- [~] Recursive generic forms (guards; not full)
- [x] Instantiation cache (fingerprint key, exact comparison on a hit)
- [ ] Instantiation canonicalization
- [ ] Generic type parameters as type arguments
- [ ] Constraint checking under substitution (full)

---

## 7. Assignability & relations

- [x] Identity
- [x] Subtyping (structural)
- [x] Assignability ≈ subtyping (current)
- [ ] Full assignability matrix ≠ pure subtype (freshness, enum quirks, etc.)
- [ ] Type comparability / strict equality checks
- [ ] Variance for type constructors (arrays, promises, etc.)
- [ ] Optional property assignability edge cases
- [ ] Excess property checking (all positions)
- [ ] Weak type detection
- [ ] Intersection/union distribute over relations
- [ ] Recursive type relations (coinductive) `[x]` basic

---

## 8. Type operators & advanced forms

### 8.1 Keyof / indexed / mapped
- [ ] `keyof T`
- [ ] `T[K]` indexed access
- [ ] Homomorphic mapped types
- [ ] Non-homomorphic mapped types
- [ ] Key remapping (`as`)
- [ ] `+readonly` / `-readonly` / `+?` / `-?` in mapped types
- [ ] Mapped types over unions / arrays / tuples

### 8.2 Conditionals
- [ ] `T extends U ? X : Y`
- [ ] Distributive conditional types
- [ ] `infer` in true/false branches
- [ ] Nested conditionals
- [ ] Recursive conditional types
- [ ] `infer` constraints (`infer T extends U`)

### 8.3 Template literal types
- [ ] Template literal types
- [ ] Inference into template literal patterns
- [ ] Intrinsic string manipulation types (full)

### 8.4 Other operators
- [ ] `typeof` *type* query (value → type)
- [ ] `instanceof` narrowing (full)
- [ ] `satisfies`
- [ ] Import types (`import("…").T`)
- [ ] `unique symbol` types

---

## 9. Narrowing & control flow

- [x] `typeof` narrowing
- [x] Nullish equality narrowing
- [x] Truthiness narrowing
- [~] Equality narrowing (literals, booleans and `unknown`; not variable against variable)
- [x] `instanceof` (identifier against a declared class)
- [x] `in` operator narrowing (string-literal key on an identifier)
- [x] Discriminated unions (a property of an identifier, in `if` and `switch`)
- [x] `switch` narrowing (grouped labels, default as the complement, `switch (typeof x)`)
- [x] `&&` / `||` condition narrowing
- [ ] Switch exhaustiveness
- [ ] User-defined type predicates
- [ ] Assertion functions (`asserts x is T`)
- [x] Assignment narrowing (identifier targets)
- [x] Branch joins (after `if`/`else`, after loops)
- [x] Truthiness of a property narrows its parent union (`if (result.ok)`)
- [~] Loop test and exit narrowing (the body, and after a loop with no `break`)
- [ ] Property / element narrowing
- [ ] Aliasing / CFA of captured variables
- [ ] Control-flow graph type propagation (full)
- [ ] Loop fixed-point analysis (the body is walked once)
- [x] Unreachable code analysis (every statement list, literal `true` / `false` conditions; matches tsc on the probe set)
- [ ] Definite assignment analysis
- [ ] `this` narrowing

---

## 10. Contextual typing

- [ ] Contextual typing of parameters
- [ ] Contextual typing of return
- [ ] Contextual typing of object literals
- [ ] Contextual typing of array / tuple literals
- [ ] Contextual typing of JSX (if applicable)
- [ ] Best common type / candidate unions
- [ ] Soften / widen under context
- [ ] Generic contextual inference

---

## 11. Modules & namespaces (type side)

- [ ] ES module exports (types + values)
- [ ] ES module imports
- [ ] `export type` / `import type`
- [ ] Re-exports
- [ ] Default export types
- [ ] Ambient modules (`declare module`)
- [ ] Wildcard / pattern ambient modules
- [ ] Namespace merging
- [ ] Module augmentation
- [ ] Global augmentation
- [ ] UMD / global scripts vs modules
- [~] Path mapping / package types (`.d.ts`): package `types` and `exports` resolve; tsconfig `paths` and `baseUrl` do not

### 11.1 Project graph (resolution layer, `module-resolution` feature)

These are regression-tested in `tests/module_resolution.rs`. They are about finding and ordering files, so they are not part of the tsc diagnostic comparison, and none of them binds a name across files.

- [x] Extensionless, directory (`index`) and `.js`-to-`.ts` specifiers
- [x] Package `exports` with the `types` condition, subpath exports and `#imports`
- [x] Nested `node_modules` shadowing a hoisted package
- [x] Type-only imports and re-exports recorded as distinct edges
- [x] Unresolved imports kept as edges, syntax errors flagged
- [x] Import cycles (including a file importing itself) and dependency layers
- [x] Parallel project check with a report for every file, in `FileId` order
- [x] Changed-file detection (fingerprint, then content hash)
- [ ] tsconfig `paths` / `baseUrl`
- [ ] Re-check only changed files and their dependents

---

## 12. JSX / TSX (if in scope)

- [ ] Intrinsic elements
- [ ] Value-based components
- [ ] Class components
- [ ] JSX element types / `JSX` namespace
- [ ] Children typing
- [ ] Attribute checking

---

## 13. JavaScript checking & interop

- [ ] `allowJs` type checking
- [ ] JSDoc type syntax
- [ ] CommonJS `exports` / `require` types
- [ ] ES interop (`esModuleInterop`) type effects

---

## 14. Standard library & builtins (typing)

- [x] Empty array literal typed `never[]` (assignable to any array type); an unannotated `const xs = []` is `any[]`
- [ ] Array methods (`push`, `map`, `filter`, ...). Only `length` is modelled, so a call to any array method is reported as a missing property. This is the largest remaining source of false errors on everyday code, and it needs generic method typing with contextual callback inference

- [~] `Array` / `ReadonlyArray`
- [~] `Promise` (opaque / partial)
- [ ] `Readonly`, `Partial`, `Required`, `Pick`, `Omit`, `Record`, …
- [ ] `ReturnType`, `Parameters`, `ConstructorParameters`, `InstanceType`, …
- [ ] `Awaited`
- [ ] `ThisType` / `ThisParameterType` / `OmitThisParameter`
- [ ] `NonNullable`, `Extract`, `Exclude`
- [ ] Iterator / Iterable types
- [ ] DOM / lib stubs as adopted

---

## 15. Diagnostics & error model

- [x] Core type errors (assignability, arity, constraints)
- [~] Error recovery without cascades (`Error` type)
- [ ] Related information / error chains
- [~] Exact `tsc` error codes / messages (wording matches for the strict fixture set)
- [ ] Suppression (`@ts-ignore` / `@ts-expect-error`)
- [ ] Suggestion diagnostics

---

## Summary (ts-rust snapshot)

| Area | Coverage |
|------|----------|
| Core primitives / unions / objects / functions | High |
| Generics (functions, interfaces, aliases, classes) | High |
| Classes / structural subtyping | Medium–high |
| Narrowing | Medium–high (property discriminants, `in`, `instanceof`, joins; no property paths or predicates) |
| Tuples / intersections / index signatures | Low / none |
| `keyof` / indexed / mapped / conditional / template | None |
| Overloads / full contextual typing | None / low |
| Modules / project / `.d.ts` | Resolution and project graph only (opt-in feature); no cross-file binding |
| Lib utility types | Minimal |

**To “cover all TS types”:** every unchecked item above is in scope.  
**Practical order:** modules (§11) → mid gaps (tuples, intersections, index, overloads, contextual) → operators (§8) → full flow (§9) → lib (§14).

---

*This list is the TypeScript type-system surface, not an implementation roadmap. Check items off only when behavior is regression-tested against `tsc` on representative fixtures.*
