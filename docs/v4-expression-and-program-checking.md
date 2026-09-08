# v4: Expression and Program Checking

> **Architecture case study:** integrating the earlier semantic layers into a broader program checker.

## 1. Why v4 is the major integration stage

v4 contains 44 fixtures covering functions, arrows, callbacks, calls, member access, computed properties, array indexing, logical operators, conditional expressions, optional chaining, non-null assertions, `as` assertions, loops, switch statements, early returns, enums, and static class members.

The important milestone is not the number of syntax features. It is that the architecture now composes across expressions, statements, declarations, symbols, narrowing, and subtyping.

## 2. `CheckContext` as the explicit semantic environment

The checker context groups shared state:

```text
TypeArena
TypeNamespace
SymbolTypeMap
Diagnostic list
NarrowState
current return type
current class instance
```

This is an explicit compiler-context pattern. State is shared without becoming global, and temporary context can be saved/restored.

## 3. Expression inference dispatcher

`infer_expression_type` is the central expression entry point. Specialized handlers cover member access, computed access, arrows, function expressions, logical and conditional expressions, assertions, literals, arrays/objects, binary expressions, calls, and `new`.

Each handler returns a semantic `TypeId`.

```text
Oxc Expression
      -> infer_expression_type
      -> TypeId
      -> subtyping / narrowing / call checking
```

## 4. Functions and callbacks

Arrow functions and function expressions reuse `FunctionType`. Return inference, explicit return checking, untyped parameters, and callback annotations all feed the same callable representation.

The architecture avoids separate semantic types for arrow functions, function expressions, and methods when their callable behavior is shared.

## 5. Centralized call checking

`check_callable` performs callable validation: identify callable type, check arity, validate arguments, and return the resulting type. The fixture `call_arity_mismatch_still_checks_arguments.ts` protects the recovery rule that an arity error should not prevent useful argument checking.

## 6. Member and computed access

Member access first computes the property type. A subsequent call can then use normal callable checking. Computed access distinguishes statically known string keys from dynamic keys; unknown dynamic property names are handled conservatively.

## 7. Arrays and indexing

The existing `Array(TypeId)` representation becomes operational. Numeric indexing returns the element type, while dynamic indexing can include `undefined` in the current model. No separate array-index type system was required.

## 8. Logical and conditional expressions

`&&`, `||`, and `??` are handled by a dedicated logical-expression path because they both produce values and carry flow semantics. Ternaries reuse branch narrowing and combine branch result types.

```text
condition
  -> true/false NarrowState
  -> check each expression
  -> compute result TypeId
```

This is direct reuse of v3b.

## 9. Early return, loops, and switch

The statement layer tracks exit behavior and uses the existing narrowing state. v4 adds body checking for `while`, `for`, and `switch`, while early-return fixtures establish that narrowing on a surviving path must remain available.

The architecture therefore grows the existing statement/flow layer instead of adding independent checkers for every control-flow construct.

## 10. Assertions and optional chaining

`as` changes the tracked semantic type while still allowing downstream checking. Non-null assertions remove nullish components. Optional chaining introduces the possible `undefined` result. These behaviors are expressed through existing type/union operations.

## 11. Enums and static members

Enum declaration processing lives in the declaration layer. Numeric auto-increment, string members, enum type-position behavior, and unsupported computed initializers are covered by fixtures. Static class fields/methods reuse existing member and statement checking with appropriate class context.

## 12. Error recovery as a design requirement

v4 tests deliberately preserve useful diagnostics after earlier errors. The general policy is:

```text
find error
  -> record diagnostic
  -> continue if enough semantic information remains
```

This is important for developer tooling because one recoverable error should not erase every later diagnostic.

## 13. Why the architecture scales

By v4 the flow is:

```text
Oxc AST + symbols
        |
        v
CheckContext
  /    |     \
namespace symbols NarrowState
  \    |     /
   expression/statement checking
          |
        TypeId
          |
      subtyping
          |
      diagnostics
```

The stage proves that the chosen boundaries cooperate rather than merely existing as separate modules.

## 14. What v4 completed

- function and arrow checking
- callback typing
- call checking and arity recovery
- property/computed access
- array indexing
- logical/conditional expression typing
- optional chaining
- non-null assertions
- type assertions
- loop/switch body checking
- early-return flow behavior
- enums
- static class members

The central lesson is: **program checking is built by composing semantic layers, not by growing one giant AST matcher.**
