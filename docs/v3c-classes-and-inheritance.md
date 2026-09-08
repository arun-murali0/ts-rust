# v3c: Classes, Inheritance, and `this`

> **Architecture case study:** adding class semantics by composing the structural model instead of creating a disconnected class type system.

## 1. Why classes came after structural types

Classes require declaration resolution, instance shape, inheritance, methods, constructors, and member access. v2 already provided object types, properties, functions, and a declaration namespace, so v3c could compose those primitives.

Fixtures cover inherited fields, missing inherited fields, method calls, constructor arity, `this`, and conservative unsupported-member behavior.

## 2. Class resolution

`TypeNamespace` resolves a class declaration into an object-shaped semantic representation. Resolution starts with inherited properties and then adds/updates the child's instance members.

```text
class declaration
      -> resolve heritage
      -> collect inherited properties
      -> resolve fields/methods
      -> build instance object shape
```

## 3. Why inheritance belongs in the namespace

Heritage is a declaration relationship. The namespace already owns name lookup and declaration resolution, so resolving a parent class there keeps expression inference focused on using the resulting type.

## 4. Methods reuse `FunctionType`

A method is callable. Instead of creating a separate method type, the checker reuses the existing function representation. Member access obtains the function type; normal call checking validates its arguments.

This gives:

```text
obj.method(...)
 -> member type
 -> callable check
 -> argument compatibility
```

## 5. Constructors

Constructor signatures reuse parameter metadata and are checked when a `new` expression is analyzed. The `new_expression_arity_mismatch.ts` fixture protects this behavior.

## 6. `this` context

`CheckContext` carries `current_class_instance: Option<TypeId>`. Class member checking temporarily enters a class context, resolves `this` against that context, and restores the previous context.

This is an explicit **context-stack style** design rather than a global special case.

## 7. Conservative unsupported behavior

Fixtures such as `this_expression_known_gap.ts`, `one_unresolvable_member_makes_whole_class_unsupported.ts`, and the untyped parameter cases document current boundaries. The checker prefers localized diagnostics or conservative fallback to corrupting the semantic model.

## 8. Why no separate `Type::Class`

A separate class type would force every operation to understand both `Object` and `Class`. Where instance semantics are structural, the existing `ObjectType` can represent the useful shape and therefore lets subtyping, properties, and calls reuse existing machinery.

## 9. What v3c completed

- class declaration resolution
- instance fields
- inherited fields
- methods
- constructors and arity
- method calls
- class-aware `this` context
- conservative unsupported-member handling

The architecture lesson is: **compose existing semantic primitives before introducing a new top-level type category.**
