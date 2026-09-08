use std::hash::{BuildHasherDefault, Hasher};

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
            let mut word = [0u8; 8];
            word.copy_from_slice(&bytes[..8]);
            self.add_to_hash(u64::from_ne_bytes(word));
            bytes = &bytes[8..];
        }
        if bytes.len() >= 4 {
            let mut word = [0u8; 4];
            word.copy_from_slice(&bytes[..4]);
            self.add_to_hash(u32::from_ne_bytes(word) as u64);
            bytes = &bytes[4..];
        }
        if bytes.len() >= 2 {
            let mut word = [0u8; 2];
            word.copy_from_slice(&bytes[..2]);
            self.add_to_hash(u16::from_ne_bytes(word) as u64);
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
