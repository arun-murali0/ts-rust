// Stable identity for modules and declarations (LLD 3.1, 3.2).
//
// Every key is XXH3-128 over an explicit byte stream and never #[derive(Hash)], whose
// layout is not a stable format. The first byte of every stream is ENC_VERSION, so a
// change to any encoding bumps it and old fingerprints stop matching instead of mixing
// formats. Nothing run-local goes in: no FileId, no TypeId, no absolute project root,
// no modification time.

use std::path::{Component, Path};

use xxhash_rust::xxh3::Xxh3;

pub const ENC_VERSION: u8 = 1;

macro_rules! key128 {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u128);

        impl $name {
            pub const fn from_u128(bits: u128) -> Self {
                Self(bits)
            }
            pub const fn as_u128(self) -> u128 {
                self.0
            }
            pub const fn to_le_bytes(self) -> [u8; 16] {
                self.0.to_le_bytes()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({:032x})"), self.0)
            }
        }
    };
}

key128!(
    /// Identity of a module, by the rule in LLD 3.1.1. Survives moving the project.
    ModuleKey
);
key128!(
    /// Identity of one declaration: its module, local name and merge-group position.
    DeclKey
);
key128!(
    /// Hash of a declaration's own public shape (LLD 3.3). Other declarations appear
    /// in it by `DeclKey` only, so a body-only edit leaves it unchanged.
    SigHash
);
key128!(
    /// Merkle hash of one type node (LLD 3.2).
    NodeHash
);

// The byte stream a hash is computed over. Integers are little endian, strings and
// byte runs carry a u32 length first, so no two field layouts can read the same.
pub(crate) struct StreamWriter(Xxh3);

impl StreamWriter {
    pub(crate) fn new() -> Self {
        let mut w = Self(Xxh3::new());
        w.u8(ENC_VERSION);
        w
    }
    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.0.update(&[v]);
        self
    }
    pub(crate) fn u32(&mut self, v: u32) -> &mut Self {
        self.0.update(&v.to_le_bytes());
        self
    }
    pub(crate) fn u64(&mut self, v: u64) -> &mut Self {
        self.0.update(&v.to_le_bytes());
        self
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.u32(u32::try_from(b.len()).expect("a hashed field is under 4 GiB"));
        self.0.update(b);
        self
    }
    pub(crate) fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }
    pub(crate) fn raw128(&mut self, v: u128) -> &mut Self {
        self.0.update(&v.to_le_bytes());
        self
    }
    pub(crate) fn finish(&self) -> u128 {
        self.0.digest128()
    }
}

// Module kind bytes (LLD 3.1.1). 0x04 is not in the LLD: it names the built-in library
// declarations (`Promise`, `Array` ...) so they have a DeclKey without a file.
const KIND_PROJECT: u8 = 0x01;
const KIND_PACKAGE: u8 = 0x02;
const KIND_EXTERNAL: u8 = 0x03;
const KIND_BUILTIN: u8 = 0x04;

impl ModuleKey {
    /// `path` is relative to the project root, with `/` separators.
    pub fn project(path: &str) -> Self {
        let mut w = StreamWriter::new();
        w.u8(KIND_PROJECT).str(path);
        Self(w.finish())
    }

    /// `name` is verbatim, scope included. `subpath` is relative to the package
    /// directory, with `/` separators. A package is named for what it is, not where it
    /// was installed, so pnpm's layout and a moved project give the same key.
    pub fn package(name: &str, version: &str, subpath: &str) -> Self {
        let mut w = StreamWriter::new();
        w.u8(KIND_PACKAGE).str(name).str(version).str(subpath);
        Self(w.finish())
    }

    /// `path` is the absolute canonical path, with `/` separators.
    pub fn external(path: &str) -> Self {
        let mut w = StreamWriter::new();
        w.u8(KIND_EXTERNAL).str(path);
        Self(w.finish())
    }

    /// The module of the built-in library declarations.
    pub fn builtin() -> Self {
        let mut w = StreamWriter::new();
        w.u8(KIND_BUILTIN);
        Self(w.finish())
    }

    /// The `project` key for `file` if it lies under `root` and does not pass through
    /// a `node_modules` directory; None otherwise (the caller then looks for a package
    /// or falls back to `external`). Purely lexical: no filesystem access, so it works
    /// for in-memory and wasm inputs. `.` components are dropped, `..` is refused.
    pub fn project_path(root: &Path, file: &Path) -> Option<String> {
        let rel = file.strip_prefix(root).ok()?;
        let mut parts: Vec<&str> = Vec::new();
        for c in rel.components() {
            match c {
                Component::Normal(s) => {
                    let s = s.to_str()?;
                    if s == "node_modules" {
                        return None;
                    }
                    parts.push(s);
                }
                Component::CurDir => {}
                _ => return None,
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join("/"))
    }
}

/// What a declaration is, for the merge-group disambiguator. The tag values are part of
/// the encoding: never renumber one, bump `ENC_VERSION` instead.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum DeclKind {
    Interface = 1,
    Class = 2,
    Alias = 3,
    Function = 4,
    Var = 5,
    Enum = 6,
    Namespace = 7,
}

impl DeclKey {
    /// `index` is the position in source order among declarations of the same kind
    /// and name in this module (overloads, merged declarations), so each is distinct
    /// without the key depending on a span.
    pub fn new(module: ModuleKey, name: &str, kind: DeclKind, index: u32) -> Self {
        let mut w = StreamWriter::new();
        w.u8(0xDE)
            .raw128(module.as_u128())
            .str(name)
            .u8(kind as u8)
            .u32(index);
        Self(w.finish())
    }

    /// A declaration of the built-in library.
    pub fn builtin(name: &str) -> Self {
        Self::new(ModuleKey::builtin(), name, DeclKind::Interface, 0)
    }

    /// A key for something that owns type parameters but is not itself a declaration,
    /// such as the second generic function type inside one alias. `ordinal` counts
    /// those in source order within `self`. This keeps `TypeParam(owner, index)` unique
    /// without changing the wire format.
    pub fn child(self, ordinal: u32) -> Self {
        let mut w = StreamWriter::new();
        w.u8(0xDC).raw128(self.as_u128()).u32(ordinal);
        Self(w.finish())
    }
}

/// A type parameter's stable identity: the declaration (or `child` of one) that
/// declares it, and its position in that list.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TypeParamKey {
    pub owner: DeclKey,
    pub index: u32,
}
