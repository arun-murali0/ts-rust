Perfect! Here are the **3 key code changes** in detail:

---

## **3 Critical Code Changes**

### **1️⃣ Arrow Functions & Function Expressions** 
**File:** `src/bridge/expressions.rs` (Lines 56-209)

#### **NEW: `infer_arrow_function_type()` (Lines 125-164)**
```rust
fn infer_arrow_function_type(
    arrow: &oxc_ast::ast::ArrowFunctionExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    // 1. Resolve parameters with any-fallback (untyped params get `any`)
    let param_types = resolve_params_with_any_fallback(&arrow.params, &mut ctx.namespace, &mut ctx.arena);
    bind_params(&arrow.params.items, &param_types, ctx);

    // 2. Check declared return type if present
    let declared_return =
        arrow.return_type.as_ref().and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));

    // 3. Handle two arrow body forms:
    let return_type = if let Some(body_expr) = arrow.body.as_expression() {
        // Expression body: `(n) => n * 2` — infer and check return
        let inferred = infer_expression_type(body_expr, scoping, ctx);
        match declared_return {
            Some(declared) => {
                // Check inferred against declared
                if !crate::subtyping::is_subtype(&ctx.arena, inferred, declared) {
                    ctx.error("Return type does not match...", body_expr.span());
                }
                declared
            }
            None => inferred,  // Use inferred if no declaration
        }
    } else {
        // Block body: `(n) => { return n * 2; }` — uses declared or `any`
        let ArrowFunctionBody::FunctionBody(body) = &arrow.body else {
            unreachable!("as_expression() returned None, so this must be the block-body variant")
        };
        let return_type = declared_return.unwrap_or_else(|| ctx.arena.any());
        let outer_return_type = ctx.current_return_type.replace(return_type);
        for body_stmt in &body.statements {
            check_statement(body_stmt, scoping, ctx);
        }
        ctx.current_return_type = outer_return_type;
        return_type
    };

    ctx.arena.alloc(Type::Function(FunctionType { params: param_types, return_type, is_untyped: false }))
}
```

**Key insight:** Arrow functions **lexically inherit `this`** — no clearing of `current_class_instance`.

#### **NEW: `infer_function_expression_type()` (Lines 166-209)**
```rust
fn infer_function_expression_type(func: &Function, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) -> TypeId {
    // Same parameter resolution as arrow functions
    let param_types = resolve_params_with_any_fallback(&func.params, &mut ctx.namespace, &mut ctx.arena);
    bind_params(&func.params.items, &param_types, ctx);

    let declared_return =
        func.return_type.as_ref().and_then(|rt| resolve_type_annotation(rt, &mut ctx.namespace, &mut ctx.arena));
    let return_type = declared_return.unwrap_or_else(|| ctx.arena.any());

    if let Some(body) = &func.body {
        let outer_return_type = ctx.current_return_type.replace(return_type);
        // 🔴 CRITICAL: Plain `function` expressions get DYNAMIC `this`, not lexical
        // This is why we CLEAR current_class_instance for the body:
        let outer_class_instance = ctx.current_class_instance.take();
        for body_stmt in &body.statements {
            check_statement(body_stmt, scoping, ctx);
        }
        ctx.current_class_instance = outer_class_instance;  // Restore
        ctx.current_return_type = outer_return_type;
    }

    ctx.arena.alloc(Type::Function(FunctionType { params: param_types, return_type, is_untyped: false }))
}
```

**Critical difference:** `.take()` clears `current_class_instance`, so `this` inside resolves to **Error** (unknown), not the enclosing class's type.

#### **Enhanced: `check_callable()` (Lines 366-431)**
```rust
fn check_callable(
    callee_type: TypeId,
    arguments: &[oxc_ast::ast::Argument],
    span: Span,
    not_callable_message: &str,
    untyped_message: &str,  // 🆕 NEW parameter
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Type::Function(function_type) = ctx.arena.get(callee_type).clone() else {
        // 🆕 NEW: Type::Any is now excluded from "not callable" error
        if !matches!(ctx.arena.get(callee_type), Type::Any | Type::Error) {
            ctx.error(not_callable_message, span);
            return ctx.arena.error();
        }
        // Even with Any/Error, still check each argument expression
        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return ctx.arena.error();
    };

    // 🆕 NEW: Handle is_untyped (separate from None)
    if function_type.is_untyped {
        ctx.warning(untyped_message, span);
        for arg in arguments {
            if let Some(arg_expr) = arg.as_expression() {
                infer_expression_type(arg_expr, scoping, ctx);
            }
        }
        return function_type.return_type;
    }

    // Rest of arity/type checking as before...
}
```

**Pattern:** 
- `let f: any = 5; f(1, 2);` → No "not callable" error (because `any` erases all checking)
- But `f(1 - "x")` inside → Still errors (argument itself is checked)

---

### **2️⃣ Unresolvable Type Annotations Now Reported**
**File:** `src/bridge/statements.rs` (Lines 25-177)

#### **NEW: `AnnotationOutcome` Enum (Lines 25-33)**
```rust
enum AnnotationOutcome {
    Absent,                    // No annotation was written
    Resolved(crate::arena::TypeId),  // Annotation resolved successfully
    Unresolvable,              // Annotation written but failed to resolve
}
```

#### **Changes in VariableDeclaration Handling (Lines 90-177)**

**Before (BUGGY):**
```rust
let annotation_type = declarator
    .type_annotation
    .as_ref()
    .and_then(|a| resolve_type_annotation(a, ...));

match (annotation_type, inferred_type) {
    (Some(declared), Some(actual)) => { /* check */ }
    (None, Some(actual)) => { /* infer type */ }  // Treats all failures same
    // ...
}
```

**After (FIXED):**
```rust
let annotation_outcome = match &declarator.type_annotation {
    None => AnnotationOutcome::Absent,
    Some(annotation) => {
        match resolve_type_annotation(annotation, ...) {
            Some(type_id) => AnnotationOutcome::Resolved(type_id),
            None => AnnotationOutcome::Unresolvable,  // ← Separate case!
        }
    }
};

match (annotation_outcome, inferred_type) {
    // 🆕 NEW CASE: Annotation exists but failed to resolve
    (AnnotationOutcome::Unresolvable, _) => {
        ctx.warning(
            format!(
                "Type annotation for '{}' could not be resolved (unknown name, \
                 or its definition isn't fully understood by ts-rust yet).",
                id.name
            ),
            declarator.span(),
        );
        // Variable is deliberately NOT registered — later references will hit warning in expressions.rs
    }

    (AnnotationOutcome::Resolved(declared), Some(actual)) => { /* check */ }
    (AnnotationOutcome::Absent, Some(actual)) => { /* infer */ }
    // ...
}
```

**Impact:**
```typescript
// Before: No diagnostic, x silently gets inferred type
let x: DoesNotExist = 5;  // ❌ Silent pass

// After: Warning diagnostic produced
let x: DoesNotExist = 5;  // ⚠️ Type annotation for 'x' could not be resolved
```

---

### **3️⃣ Smart Parameter Resolution: Strict vs. Any-Fallback**
**File:** `src/type_annotation.rs` (Lines 91-143)

#### **Kept: `resolve_function_params()` (Lines 98-109)** — Strict, all-or-nothing
```rust
pub fn resolve_function_params(
    params: &FormalParameters,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Option<Vec<TypeId>> {
    let mut resolved = Vec::with_capacity(params.items.len());
    for param in &params.items {
        let annotation = param.type_annotation.as_ref()?;  // ← Fails if ANY param untyped
        resolved.push(resolve_type_annotation(annotation, namespace, arena)?);
    }
    Some(resolved)
}
```

**Used for:** Top-level `function` declarations, class methods/constructors  
**Why:** Nothing calls them before Pass 2 hoisting completes, so not registering a partially-typed function is safe.

#### **NEW: `resolve_params_with_any_fallback()` (Lines 127-143)** — Lenient, per-param
```rust
pub fn resolve_params_with_any_fallback(
    params: &FormalParameters,
    namespace: &mut TypeNamespace,
    arena: &mut TypeArena,
) -> Vec<TypeId> {  // ← Returns Vec, never fails
    params
        .items
        .iter()
        .map(|param| {
            param
                .type_annotation
                .as_ref()
                .and_then(|annotation| resolve_type_annotation(annotation, namespace, arena))
                .unwrap_or_else(|| arena.any())  // ← Untyped param becomes `any`
        })
        .collect()
}
```

**Used for:**
- Arrow function values: `const f = (x) => x * 2`
- Function expression values: `const f = function(x) { return x; }`
- `TSFunctionType` annotations: `fn: (x) => number` (callback types)

**Why:** These ARE values at point of writing, already in use by callers, can't skip registration.

#### **NEW: TSFunctionType case (Lines 46-60)**
```rust
TSType::TSFunctionType(func_type) => {
    // Uses the any-fallback resolver, not the strict
    // resolve_function_params below: `(x) => number` written as
    // a type (e.g. a callback parameter's own annotation) is
    // exactly as legal in real TS as an untyped arrow-function
    // *value* parameter, and should default to `any` the same
    // way rather than making the whole annotation unresolvable.
    let params = resolve_params_with_any_fallback(&func_type.params, namespace, arena);
    let return_type = resolve_type_annotation(&func_type.return_type, namespace, arena)?;
    Some(arena.alloc(Type::Function(crate::types::FunctionType { 
        params, 
        return_type, 
        is_untyped: false 
    })))
}
```

**Example:**
```typescript
// Callback type annotation with untyped param
function apply(fn: (x) => number, value: number): number {
    return fn(value);  // ← `apply` now registers! (before it silently failed)
}

apply((x: number) => x * 2, 5);  // ✅ x in callback can be untyped OR typed
```

---

## **Summary Table**

| Change | File | Lines | Purpose | Impact |
|--------|------|-------|---------|--------|
| **Arrow function inference** | expressions.rs | 125-164 | Type-check arrow values as expressions | `arr.map(x => x * 2)` now works |
| **Function expression inference** | expressions.rs | 166-209 | Type-check function expressions, fix `this` binding | `const f = function() {}` works, `this` correct |
| **Enhanced check_callable** | expressions.rs | 366-431 | Handle `Type::Any` as callable, separate `is_untyped` path | `let f: any; f()` legal, args still checked |
| **AnnotationOutcome enum** | statements.rs | 25-33 | Distinguish "absent" from "unresolvable" annotations | Typos in type names now reported |
| **Unresolvable annotation handling** | statements.rs | 117-126 | Produce diagnostic for failed type resolution | `let x: DoesNotExist = 5;` warns |
| **resolve_function_params (refactored)** | type_annotation.rs | 98-109 | Changed to accept `&FormalParameters` instead of `&Function` | Works for both real functions and TSFunctionType |
| **resolve_params_with_any_fallback (new)** | type_annotation.rs | 127-143 | Untyped params default to `any`, don't fail | Callbacks work without full annotation |
| **TSFunctionType resolution** | type_annotation.rs | 46-60 | Use any-fallback for callback type annotations | `fn: (x) => number` allows untyped `x` |

---

These 3 changes unlock **real-world TypeScript patterns**: callbacks, arrow functions, and honest error reporting for typos. 🎯
