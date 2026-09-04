//! Heap-allocation profiler. Run with:
//!   cargo run --release --example dhat_heap --features dhat-heap
//! then open the resulting `dhat-heap.json` at
//! https://nnethercote.github.io/dh_view/dh_view.html
//!
//! This measures allocation *count and size*, not instruction count or
//! wall-clock time — it answers "how many mallocs/frees happened, and
//! how big were they", which neither `checker_benchmark.rs` (Criterion,
//! wall-clock) nor `benches/checker_iai.rs` (Callgrind, instructions)
//! can tell you directly.
//!
//! One thing this will NOT show, so don't go looking for it: a "string
//! interner" or allocations dropping to zero. Neither exists in this
//! codebase. `TypeArena` (which does exist, and predates the
//! HashMap-to-Vec/FxHashMap change) still allocates when it grows;
//! `SymbolTypeMap`'s `Vec<Option<TypeId>>` still allocates on resize;
//! `TypeNamespace`'s `FxHashMap` still allocates its bucket table
//! exactly like `std::HashMap` did. What changed in that refactor was
//! the cost of *hashing* and *lookup*, not allocation count — so the
//! honest thing to look for here is whether allocation count/size
//! changed at all (it plausibly didn't, much), not proof of some
//! elimination that was never the claim.
//!
//! dhat needs to instrument an actual running program end-to-end, not a
//! library in isolation, hence this being a thin binary rather than a
//! benchmark — it works by overriding the process's global allocator,
//! which only makes sense for something with a `main()`.
//!
//! Not run in this environment: no `cargo`/`rustc` available here, so
//! this is unverified against a real compiler, same caveat as
//! `benches/checker_iai.rs`.

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn large_source(function_count: usize) -> String {
    let mut src = String::with_capacity(function_count * 80);
    for i in 0..function_count {
        src.push_str(&format!("function fn{i}(a: number, b: number): number {{\n    return a + b;\n}}\n"));
    }
    src
}

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let source = large_source(1_000);
    let checker = ts_rust::TypeChecker::new();
    let result = checker.check_source(&source, "dhat_large.ts").unwrap();

    // Keep the result alive (and touched) until here so nothing gets
    // optimized away before the profiler's Drop impl writes
    // dhat-heap.json — same reasoning as the black_box calls in the
    // other two benchmark harnesses.
    std::hint::black_box(result);
}
