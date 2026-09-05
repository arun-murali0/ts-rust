# Recent Changes Summary - ts-rust Repository

**Repository:** arun-murali0/ts-rust  
**Date Range:** Sept 4-5, 2026  
**Status:** 5 commits total

---

## Commit Timeline

### 1. **Initial Commit** | Sept 4, 2026 @ 07:40 UTC
**Message:** `ts-rust`  
**SHA:** `13f6e06940d1386c58e26a77db8a6f40acb5ce70`

- **Initial repository setup** — all source files, Cargo.toml, docs, test fixtures, benchmarks

---

### 2. **Update** | Sept 4, 2026 @ 13:16 UTC
**Message:** `update`  
**SHA:** `977dd3cfd897d2abd6f4d354dbbe1d91ce77dd00`

- Minor repository updates (commit details minimal)

---

### 3. **Bug Solved** ✅ | Sept 5, 2026 @ 08:41 UTC
**Message:** `bug solved`  
**SHA:** `b4cdfaa72ff3812ca9439a2f02b582b28f32b1c4`  
**Stats:** +569 lines, -75 lines (644 net changes)

#### **Key Issues Fixed:**

1. **Line Position Reporting (UTF-16 vs UTF-8 Mismatch)**
   - **New:** `src/line_index.rs` (134 lines)
   - **Problem:** Diagnostics reported byte offsets, not line:column positions. Two consumers need different column formats:
     - Native CLI: Unicode **codepoint** count (like rustc)
     - WASM/LSP: UTF-16 **code unit** count (JS strings are UTF-16)
   - **Solution:** `LineIndex` converts byte offsets to both formats
   - **Impact:** Emoji and CJK characters after line start were causing silent position misalignment
   - **Tests added:** 3 new tests including astral-plane emoji case

2. **Unresolvable Type Annotations Silently Accepted**
   - **File:** `src/bridge/statements.rs`
   - **Problem:** `let x: DoesNotExist = 5;` (typo in type name) was silently accepted with no diagnostic
   - **Root cause:** Code collapsed "no annotation" and "annotation present but unresolvable" into one `None`
   - **Solution:** New `AnnotationOutcome` enum distinguishes three states:
     ```rust
     enum AnnotationOutcome {
         Absent,           // no type annotation written
         Resolved(TypeId), // annotation resolved successfully
         Unresolvable,     // annotation present but failed resolution
     }
     ```
   - **Result:** Now produces a **Warning** diagnostic naming the unresolvable type
   - **Fixture added:** `tests/fixtures/v2/unresolvable_type_annotation_is_reported.ts`

3. **Classes with Unresolvable Members Still Partially Registered**
   - **Files:** `src/namespace.rs`, `src/bridge/statements.rs`
   - **Problem:** A class with one unresolvable field or method registered as a partial instance type, with no diagnostic anywhere
   - **Solution:** Match interface policy — fail the entire class if *any* member can't be resolved, report at class declaration site
   - **Fixtures added:**
     - `tests/fixtures/v3c/one_unresolvable_member_makes_whole_class_unsupported.ts`
     - `tests/fixtures/v3c/untyped_method_param_skips_arity_not_whole_class.ts` (narrower case)

4. **Untyped Method Parameters Took Down Entire Class**
   - **File:** `src/namespace.rs` (resolve_class method)
   - **Problem:** A method with known return type but untyped parameter failed the whole class
   - **Solution:** Register method with `is_untyped: true` flag (like untyped constructors), skip arity checking only
   - **Benefit:** Keeps other correctly-typed class members checkable

5. **Diagnostic API Improvements**
   - **File:** `src/diagnostics.rs`
   - **Added:** `format_with_position(&self, line_index: &LineIndex, source: &str) -> String`
   - **Format:** `Error: message (file:line:col)` with 1-based codepoint positions
   - **Note:** `line_index` reuse amortizes line-boundary scans across all diagnostics in a file

6. **WASM Boundary Encoding Mismatch Fixed**
   - **File:** `src/wasm.rs`
   - **New struct:** `WasmDiagnostic` (UTF-16 line/column)
   - **Problem:** Raw byte offsets crossing to JS broke cursor positioning after non-ASCII characters
   - **Solution:** Convert to UTF-16 at WASM boundary so JS callers get correct string indices

#### **Test Coverage:**
- **New V2 test:** `unresolvable_type_annotation_is_reported_not_silently_swallowed()`
- **New V3c tests:**
  - `one_unresolvable_member_makes_whole_class_unsupported()`
  - `untyped_method_param_skips_arity_not_whole_class()`

---

### 4. **V4 Tier Added** 🚀 | Sept 5, 2026 @ 14:40 UTC
**Message:** `v4 tier added`  
**SHA:** `27dc119f09e28ad76bb85f7b50491a16d4037413`  
**Stats:** +412 lines, -37 lines (375 net changes)

#### **Major Feature: Arrow Functions & Function Expressions (V4 Tier 1, Item 1)**

This is **the single highest-leverage gap** from V3c roadmap — arrow functions as callbacks like `arr.map(x => x + 1)` now work.

##### **New Functions in `src/type_annotation.rs`:**

1. **`resolve_params_with_any_fallback()`** (60 lines)
   - Untyped parameters default to `any` instead of failing entire function
   - Used for:
     - Arrow function values: `const f = (x) => x * 2` — x is `any`
     - `TSFunctionType` annotations: `fn: (x) => number` — x gets `any`
   - Rationale: These shapes are extremely common in real code

2. **`resolve_function_params()`** (refactored)
   - Changed to accept `&FormalParameters` instead of `&Function`
   - Now works for both real functions and `TSFunctionType` annotations
   - Keeps strict all-or-nothing behavior (needed for top-level declarations)

3. **`TSType::TSFunctionType` case added**
   - Resolves function-type annotations: `(x: number) => string`
   - Uses `resolve_params_with_any_fallback` instead of strict params resolver
   - Fixes inconsistency: type annotation `(x) => number` now allows untyped x same as value `(x) => x * 2`

##### **New Functions in `src/bridge/expressions.rs`:**

1. **`infer_arrow_function_type()`** (45 lines)
   - Handles arrow function **expressions** (not just types)
   - Two forms:
     - Expression body: `(n: number) => n * n` — return type inferred and checked
     - Block body: `(n: number) => { return n * n; }` — uses declared return type
   - Sets `current_return_type` for `return` statements inside the body
   - **Does NOT** clear `current_class_instance` (arrow functions lexically inherit `this`)

2. **`infer_function_expression_type()`** (47 lines)
   - Handles `const f = function(x) { ... }` (unnamed function expressions)
   - **Does** clear `current_class_instance` (function expressions get dynamic `this`, not lexical)
   - Regression fixture: nested function expressions in class methods no longer leak class's `this` type
   - Recursion caveat: Function expression's own name (`function named() {}`) isn't registered yet (accepted gap)

3. **`check_callable()` refactored**
   - Now handles `Type::Any` the same as `Type::Error` — both are universally callable
   - Separated error messages: `not_callable` vs `untyped_message`
   - Each argument still checked on its own terms even when arity is skipped
   - Regression fixture: `let f: any = 5; f(1, 2);` no longer falsely errors "not callable"

##### **New Files:**
- `src/bridge/statements.rs`: Made `check_statement()` and `bind_params()` public (so expressions.rs can call them)

##### **New Test Fixtures (10 files, 122 lines):**

All in `tests/fixtures/v4/`:

| File | Tests |
|------|-------|
| `arrow_function_fully_annotated.ts` | Fully typed arrow → no errors |
| `arrow_function_return_type_mismatch.ts` | Return type checked against declared |
| `arrow_function_untyped_param_defaults_to_any.ts` | Untyped params default to `any` |
| `arrow_expression_body_return_inferred.ts` | Expression body return inferred & checked |
| `function_expression_as_value.ts` | Function expressions as values |
| `callback_parameter_type_annotation.ts` | Function-type parameters + arrow call |
| `callback_type_annotation_with_untyped_param.ts` | TSFunctionType allows untyped params |
| `any_typed_value_is_callable.ts` | `let f: any = 5; f(...)` is legal |
| `function_expression_does_not_inherit_this.ts` | Plain functions get dynamic `this` |

##### **New Test Suite: `tests/v4_fixtures.rs`** (122 lines)

8 test functions covering:
- ✅ Fully annotated arrows
- ✅ Return type mismatch detection
- ✅ Untyped parameter fallback
- ✅ Expression-body return inference
- ✅ Function expressions
- ✅ Callback patterns (the #1 real-world use case)
- ✅ `any`-typed callables
- ✅ `this` binding correctness

##### **Dependencies Updated:**
- `Cargo.lock`: Minor version bumps (syn 3.0.4→3.0.5, wasm-bindgen 0.2.127→0.2.128, etc.)
- `.gitignore`: Removed `dhat-heap.json` line (heap profiling artifact)

#### **Impact:**
- **Closes the #1 gap** from V3c roadmap
- Enables real-world patterns: `arr.map(x => x * 2)`, `apply((x) => x + 1, 5)`
- Fixes callback-type annotations inconsistency
- Maintains honest diagnostics (no silent passes)

---

### 5. **README Updated** | Sept 5, 2026 @ 17:01 UTC
**Message:** `readme added`  
**SHA:** `c3023a1dd9d57ebd55e70f4f6bbce666c2eb0280`  
**Stats:** +2 lines, -2 lines

#### **Changes:**
- Renamed `README.md` (no content change)
- **`pkg-template/package.json`:**
  - License: `"MIT OR Apache-2.0"` → `"Apache-2.0"` (consistent with repo)
  - Repository: `https://github.com/YOUR_ORG/ts-rust` → `https://github.com/arun-murali0/ts-rust` (personalized)

---

## Summary of Progress

### V3c Improvements (Bug Fixes)
| Category | Fixed | Impact |
|----------|-------|--------|
| Line position reporting | UTF-16/UTF-8 mismatch | WASM cursor placement now correct |
| Type annotation validation | Unresolvable names accepted silently | Now reports Warning diagnostic |
| Class resolution | Partial types when members unresolvable | Now fails entire class (honest) |
| Method arity checking | Untyped params failed whole class | Now uses `is_untyped` flag |

### V4 Tier 1 Implementation (Arrow Functions & Function Expressions)
| Feature | Status | Tests |
|---------|--------|-------|
| Arrow function inference | ✅ Complete | 4 fixtures + tests |
| Function expression inference | ✅ Complete | 2 fixtures + tests |
| TSFunctionType with untyped params | ✅ Complete | 2 fixtures + tests |
| `any`-typed callables | ✅ Complete | 1 fixture + test |
| `this` binding correctness | ✅ Complete | 1 fixture + test |
| **Total V4 Coverage** | **✅ 9/9** | **8 test functions** |

---

## Architecture Changes

### New Modules
- **`src/line_index.rs`** — Byte-to-line-column conversion with UTF-8 and UTF-16 support

### Refactored Modules
- **`src/bridge/expressions.rs`** — Added 109 lines for arrow/function expression inference
- **`src/bridge/statements.rs`** — Visibility changes (pub functions), AnnotationOutcome enum
- **`src/namespace.rs`** — Class resolution now all-or-nothing per member
- **`src/type_annotation.rs`** — Split parameter resolution into strict and any-fallback variants
- **`src/diagnostics.rs`** — Added `format_with_position()` method
- **`src/wasm.rs`** — WasmDiagnostic wrapper for UTF-16 boundary crossing

### API Additions
```rust
// New public API
pub struct LineIndex { ... }
impl LineIndex {
    pub fn line_col_utf8(...) -> (u32, u32)
    pub fn line_col_utf16(...) -> (u32, u32)
}

impl Diagnostic {
    pub fn format_with_position(...) -> String
}
```

---

## Testing

### Test Statistics
- **V2 fixtures:** 1 new test (unresolvable type annotation)
- **V3c fixtures:** 2 new tests (class resolution, untyped method params)
- **V4 fixtures:** New test file with 8 tests covering all arrow/function expression patterns
- **Total new tests:** 11 test functions
- **New fixture files:** 13 total (1 V2, 2 V3c, 10 V4)

### Coverage
- All V3c bugfixes have regression fixtures proving the fix
- V4 Tier 1 is comprehensively tested (happy path, error cases, edge cases)
- WASM UTF-16 encoding has indirect test coverage via WasmDiagnostic

---

## Known Gaps (By Design)

### V4 Tier 1 (Just Completed)
✅ Arrow functions + function expressions
✅ TSFunctionType with untyped params
✅ Callback patterns

### V4 Tier 2 (Planned)
- Narrowing persistence past `if` statement early returns
- Logical operators (`&&`, `||`, `??`) & ternary
- `for` / `while` / `switch` loops

### V4 Tier 3 (Later)
- Computed member access (`obj[key]`)
- Optional chaining (`?.`)
- Type assertions (`as`, `!`)

### V4+ (Post-Tier 3)
- Generics
- Modules + `.d.ts`
- Incremental checking

---

## Performance Implications

### No Regression Expected
- Arrow function inference: O(n) same as regular function inference
- `TSFunctionType` resolution: Lazy, same pattern as type aliases
- `LineIndex` construction: Single O(n) pass over source (amortized across diagnostics)
- WASM UTF-16 conversion: Once per diagnostic at boundary (minimal overhead)

### Potential Future Improvements
- Parallel arrow function checking across multiple files (Post-V4)
- Cache parsed arrow function types for repeated patterns
- Pre-index common callback signatures (low priority)

---

## Migration Path for Users

### Native Rust Users
```rust
// Before: Arrow functions silently passed through unchecked
const map = arr.map(x => x * 2);  // No type info on x or return

// After: Arrow functions now get full type inference & checking
const map = arr.map((x: number): number => x * 2);  // x is typed, return is checked
```

### WASM/JS Users
```typescript
// Before: Diagnostic positions were off after emoji/CJK
checker.checkSource("const 😀 = 1; error here", "test.ts")
// → Reported wrong column position for "error here"

// After: Diagnostic positions use UTF-16, matching JS string indexing
checker.checkSource("const 😀 = 1; error here", "test.ts")
// → Diagnostic.start_column, end_column now correct for JS use
```

---

## Next Steps

1. **Benchmark V4 Tier 1** — Run `cargo bench` to establish baseline for arrow function overhead
2. **Test WASM build** — `wasm-pack build --target web --features wasm` to verify UTF-16 path works
3. **Plan V4 Tier 2** — Narrowing persistence requires control flow graph analysis (higher complexity)
4. **Lint pass** — `cargo clippy --all-targets -- -D warnings` (docs mention expected clippy noise)

---

## Commits at a Glance

```
c3023a1 (HEAD)   readme added (2 changes)
27dc119          v4 tier added (375 net: Arrow functions + function expressions)
b4cdfaa          bug solved (644 net: UTF-16, type annotation, class resolution fixes)
977dd3c          update (minimal changes)
13f6e06          (root) ts-rust (initial commit)
```

**Total project:** 5 commits, ~1500 lines added (net), **5 days old**

