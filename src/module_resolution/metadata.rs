use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use xxhash_rust::xxh3::xxh3_64;

// What a single stat call says about a file. Cheap to take and to compare, but only as
// trustworthy as the filesystem's clock: a rewrite that keeps the size and lands in the
// same timestamp tick is invisible to it, the same blind spot `make` has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileFingerprint {
    pub fn of(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// XXH3 of the file's bytes. Not a security hash; it only has to tell two versions of
/// a file apart, quickly.
pub fn content_hash(bytes: &[u8]) -> u64 {
    xxh3_64(bytes)
}

// A file as it was when the graph read it. The fingerprint is the fast answer and the
// hash is the confirmation: a touched file whose bytes did not change has a new
// fingerprint and the old hash, and is still unchanged.
#[derive(Clone, Copy, Debug)]
pub(super) struct FileState {
    fingerprint: FileFingerprint,
    hash: u64,
}

impl FileState {
    pub(super) fn new(fingerprint: FileFingerprint, hash: u64) -> Self {
        Self { fingerprint, hash }
    }

    // An unreadable file is reported as changed. Calling it unchanged would keep stale
    // results for a file that may be gone; calling it changed only costs a re-check.
    pub(super) fn is_unchanged(&self, path: &Path) -> bool {
        let Ok(current) = FileFingerprint::of(path) else {
            return false;
        };
        if current == self.fingerprint {
            return true;
        }
        fs::read(path).is_ok_and(|bytes| content_hash(&bytes) == self.hash)
    }
}

#[cfg(test)]
mod tests {
    use super::content_hash;

    #[test]
    fn the_hash_separates_contents_of_equal_length() {
        assert_ne!(content_hash(b"const a = 1;"), content_hash(b"const a = 2;"));
        assert_eq!(content_hash(b"same"), content_hash(b"same"));
    }
}
