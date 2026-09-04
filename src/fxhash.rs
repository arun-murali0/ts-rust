//! A small, self-contained copy of the FxHash algorithm — the same
//! non-cryptographic hash `rustc` itself uses internally (via the
//! `rustc-hash` crate) for its own symbol tables.
//!
//! Vendored in-tree rather than added as a dependency on `rustc-hash`:
//! it's a few dozen lines, has no dependencies of its own, and this way
//! adding it never depends on being able to resolve one more
//! version-pinned crate from crates.io. If a project preference later
//! shifts back toward pulling in the real `rustc-hash` crate instead,
//! this module can be deleted and `crate::fxhash::FxHashMap` swapped for
//! `rustc_hash::FxHashMap` with no call-site changes — the type alias
//! shape is identical.
//!
//! Not cryptographically secure and not DoS-resistant — deliberately.
//! The stdlib's default `HashMap` hasher (SipHash) pays for
//! DoS-resistance so that untrusted input (e.g. a web server hashing
//! attacker-controlled request data) can't be crafted into worst-case
//! hash collisions. Nothing hashed by this crate is adversarial: keys
//! here are type/interface/class names from source the caller already
//! chose to type-check, not data from an untrusted network peer. Paying
//! SipHash's cost here buys nothing.
//!
//! Only used for `Namespace`'s `String`-keyed map. `SymbolTypeMap` and
//! `NarrowState` are keyed by `SymbolId`, which is dense and sequential
//! (assigned in order by `oxc_semantic`'s binder) — for that shape, a
//! `Vec` indexed directly by the symbol's own index beats *any* hash
//! function, since it skips hashing entirely. See `symbol_map.rs` and
//! `bridge/narrow.rs`.

use std::hash::{BuildHasherDefault, Hasher};

/// A fixed odd constant used to mix each word into the running hash.
/// Same constant `rustc-hash`/Firefox's implementation uses; its exact
/// value doesn't matter for correctness (any large odd constant with a
/// reasonable bit pattern works), only that it's fixed and applied
/// consistently.
const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add_to_hash(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            self.add_to_hash(u64::from_ne_bytes(bytes[..8].try_into().unwrap()));
            bytes = &bytes[8..];
        }
        if bytes.len() >= 4 {
            self.add_to_hash(u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as u64);
            bytes = &bytes[4..];
        }
        if bytes.len() >= 2 {
            self.add_to_hash(u16::from_ne_bytes(bytes[..2].try_into().unwrap()) as u64);
            bytes = &bytes[2..];
        }
        if let Some(&byte) = bytes.first() {
            self.add_to_hash(byte as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hash;

    #[test]
    fn same_value_hashes_identically_every_time() {
        let mut a = FxHasher::default();
        let mut b = FxHasher::default();
        "some-type-name".hash(&mut a);
        "some-type-name".hash(&mut b);
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn different_values_usually_hash_differently() {
        // Not a guarantee for every possible pair (that's what makes it a
        // hash, not a bijection) but these two specific strings should
        // not collide, and a hasher this simple getting it wrong on the
        // very first check would be a real red flag.
        let mut a = FxHasher::default();
        let mut b = FxHasher::default();
        "Foo".hash(&mut a);
        "Bar".hash(&mut b);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn works_as_an_actual_hashmap_hasher() {
        let mut map: FxHashMap<String, i32> = FxHashMap::default();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        assert_eq!(map.get("a"), Some(&1));
        assert_eq!(map.get("b"), Some(&2));
        assert_eq!(map.get("c"), None);
    }
}
