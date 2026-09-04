//! Maps a value binding's SymbolId, assigned by oxc_semantic's binder, to
//! the TypeId we inferred or declared for it. This replaces the
//! hand-rolled scope chain from V1. oxc_semantic already resolves the
//! real scoping rules of JavaScript (hoisting, the temporal dead zone,
//! per-iteration `let` in loops), which a parent-pointer HashMap never
//! got fully right.
//!
//! Every binder-recognized symbol is expected to end up with an entry
//! here by the time checking runs, even if that entry is just the error
//! sentinel for a shape we don't understand yet (a destructured
//! parameter, for example). `is_declared` exists so callers can tell
//! "we deliberately have nothing to say about this symbol" apart from
//! "we forgot to register it", which is exactly the distinction that
//! would have caught the missing-parameter bug earlier.
//!
//! Backed by a `Vec<Option<TypeId>>` indexed directly by the symbol's
//! own index, not a `HashMap`. `oxc_semantic` assigns every symbol in a
//! file a dense, sequential `SymbolId` up front, and by the time
//! checking finishes nearly all of them end up declared here — a dense
//! key space is exactly what a `Vec` is for. This also mirrors what
//! `oxc_semantic` does internally for its own `SymbolTable`: `spans`,
//! `names`, `flags`, `scope_ids`, etc. are all `IndexVec<SymbolId, T>`,
//! not `HashMap`s. Direct indexing skips both the hash computation and
//! the bucket-array indirection a `HashMap` — even a fast one — still
//! pays on every lookup.
//!
//! Contrast with `bridge::narrow::NarrowState`, which is also keyed by
//! `SymbolId` but stays a small linear-scanned `Vec<(SymbolId, TypeId)>`
//! rather than this dense representation: a narrowing overlay is
//! created fresh per-branch and typically holds a handful of entries out
//! of however many symbols exist in the whole file, so a `Vec` sized to
//! the total symbol count would allocate memory proportional to file
//! size on every single branch. Same key type, opposite right answer —
//! the deciding factor is whether the *use* is dense or sparse, not the
//! key type alone.

use oxc_semantic::SymbolId;

use crate::arena::TypeId;

/// `SymbolId`'s own index as a `usize`, centralized in one place. If
/// `oxc_semantic` ever renames this accessor across a version bump,
/// there's exactly one call site to fix instead of several scattered
/// through this file.
#[inline]
fn symbol_index(symbol_id: SymbolId) -> usize {
    symbol_id.index()
}

pub struct SymbolTypeMap {
    types: Vec<Option<TypeId>>,
}

impl SymbolTypeMap {
    pub fn new() -> Self {
        Self { types: Vec::new() }
    }

    pub fn declare(&mut self, symbol_id: SymbolId, type_id: TypeId) {
        let index = symbol_index(symbol_id);
        if index >= self.types.len() {
            self.types.resize(index + 1, None);
        }
        self.types[index] = Some(type_id);
    }

    pub fn get(&self, symbol_id: SymbolId) -> Option<TypeId> {
        self.types.get(symbol_index(symbol_id)).copied().flatten()
    }

    // Not called anywhere yet (no caller currently needs the
    // registered/unregistered distinction), same as before this was
    // reintroduced — kept `#[allow(dead_code)]` rather than commented
    // out again so it doesn't silently bit-rot a second time.
    #[allow(dead_code)]
    pub fn is_declared(&self, symbol_id: SymbolId) -> bool {
        self.types.get(symbol_index(symbol_id)).is_some_and(Option::is_some)
    }
}

impl Default for SymbolTypeMap {
    fn default() -> Self {
        Self::new()
    }
}
