# v2: Structural Types, Declarations, and Resolution

> **Architecture case study:** how ts-rust moved from primitive checking to a reusable structural type system.

## 1. What changed from v1

v2 introduced object types, properties, structural subtyping, optional properties, arrays, unions, aliases, interfaces, forward references, circular references, and function arity.

Fixtures cover: aliases, arrays, excess properties, arity, interfaces, nested objects, optional properties, unions, unresolvable annotations, and width subtyping.

## 2. Structural typing as the core model

TypeScript's ordinary object compatibility is structural. `src/types.rs` therefore models object shape directly:

```rust
pub struct ObjectType { pub properties: Vec<PropertyEntry> }
pub struct PropertyEntry { pub name: String, pub type_id: TypeId, pub optional: bool }
```

A property carries its name, type, and optionality because all three affect assignability.

## 3. Width subtyping

`src/subtyping.rs` checks that every required target property exists in the source and that matching property types are compatible. Extra source properties are acceptable for ordinary structural assignment.

This is protected by `width_subtyping_extra_prop_ok.ts`, `width_subtyping_missing_prop_error.ts`, `width_subtyping_multiple_props.ts`, and `nested_object_subtyping.ts`.

## 4. Optional property semantics

The `optional` bit is semantic, not decorative. A source optional property cannot satisfy a required target property. This rule is explicitly covered by `optional_property_cannot_satisfy_required.ts`.

This is a good example of why a real semantic object model was needed instead of a plain name/type map.

## 5. Arrays and unions

Arrays use `Type::Array(TypeId)`, so element types are ordinary arena types. Unions use `Type::Union(Vec<TypeId>)` and are normalized by `TypeArena::alloc_union`, which flattens nested unions, removes `Never`, collapses one-member unions, and removes duplicates.

The v2 covariance fixtures intentionally record the current compatibility/soundness boundary.

## 6. Declaration namespace

`TypeNamespace` maps declaration names to aliases, interfaces, classes, or resolved types. Declaration lookup is separated from expression checking.

```text
name -> TypeEntry -> alias/interface/class/resolved TypeId
```

This keeps name resolution out of every type operation.

## 7. Lazy resolution and recursion protection

A declaration entry has `resolved` and `resolving` state. Resolution therefore behaves like a small state machine:

```text
unresolved -> resolving -> resolved
                 |
                 +-> circular reference detected
```

Lazy resolution permits forward references and recursive declarations without requiring declaration order to match dependency order.

## 8. Annotation resolution boundary

`src/type_annotation.rs` translates Oxc `TSType` nodes into ts-rust `TypeId` values. AST details stop at this boundary; the semantic core consumes `TypeId`.

## 9. Function compatibility

`FunctionType` and `Param` were introduced as semantic callable representations. Parameter metadata already records type, optionality, and rest-ness, giving later stages a stable place to implement richer call behavior.

## 10. Why these patterns were chosen

The stage deliberately uses composition:

- object subtyping composes property entries
- arrays compose `TypeId`
- unions compose existing types
- declarations resolve to semantic types
- functions reuse parameter/type representations

The goal is to add language features by extending reusable semantic primitives, not by adding syntax-specific special cases.

## 11. What v2 established

- structural object types
- width subtyping
- optional-property semantics
- arrays
- unions and normalization
- aliases/interfaces/classes as declarations
- lazy resolution
- forward/circular reference handling
- function arity
- reusable annotation resolution

The main architecture lesson is: **Oxc owns front-end infrastructure; ts-rust owns stable semantic types and relationships.**
