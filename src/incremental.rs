use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use salsa::{Database as SalsaDatabase, Durability, Setter};

use crate::types::FileId;

// One project file as the query layer sees it. The file id is a plain u32 and not a
// FileId, so the checker's own ids stay out of Salsa's handle types until a query needs
// them. `file_id` and `path` never change after the input is created; `text` and `hash`
// change only through Database::upsert_source, which keeps the point where a new
// revision starts in one place. The text is an Arc<str> so a worker thread can hold it
// without copying the file.
#[salsa::input(debug)]
pub struct SourceFile {
    #[returns(copy)]
    pub file_id: u32,
    #[returns(deref)]
    pub path: PathBuf,
    #[returns(ref)]
    pub text: Arc<str>,
    #[returns(copy)]
    pub hash: u64,
}

// The first query, here to give the layer something to memoize and to test reuse
// against. Real queries (module header, resolved imports, per-unit checking) come with
// the stages that need them.
#[salsa::tracked(returns(copy))]
pub fn source_len(db: &dyn IncrementalDb, file: SourceFile) -> u32 {
    u32::try_from(file.text(db).len()).unwrap_or(u32::MAX)
}

// Who owns a file decides how long Salsa may trust what it derived from it. Project
// files change on every save. A file under node_modules almost never does, and giving it
// high durability lets a query that reads only such files skip re-verification while
// project files are being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Project,
    Dependency,
}

impl SourceKind {
    fn durability(self) -> Durability {
        match self {
            Self::Project => Durability::LOW,
            Self::Dependency => Durability::HIGH,
        }
    }
}

#[salsa::db]
pub trait IncrementalDb: SalsaDatabase {}

// The database the project drives. `files` maps the checker's FileId to its input, so a
// file is updated in place and never gets a second input for the same id.
#[salsa::db]
#[derive(Clone, Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
    files: BTreeMap<FileId, SourceFile>,
}

#[salsa::db]
impl SalsaDatabase for Database {}

#[salsa::db]
impl IncrementalDb for Database {}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn source(&self, file: FileId) -> Option<SourceFile> {
        self.files.get(&file).copied()
    }

    /// Creates the input for `file`, or brings it up to date. A hash equal to the stored
    /// one changes nothing, so a save without edits starts no revision and every query
    /// that read the file keeps its result. Otherwise the input is rewritten in place.
    pub fn upsert_source(
        &mut self,
        file: FileId,
        path: PathBuf,
        text: Arc<str>,
        hash: u64,
        kind: SourceKind,
    ) -> SourceFile {
        if let Some(existing) = self.source(file) {
            if existing.hash(self) != hash {
                existing.set_text(self).to(text);
                existing.set_hash(self).to(hash);
            }
            return existing;
        }

        let durability = kind.durability();
        let input = SourceFile::builder(file.index(), path, text, hash)
            .file_id_durability(Durability::HIGH)
            .path_durability(Durability::HIGH)
            .text_durability(durability)
            .hash_durability(durability)
            .new(self);
        self.files.insert(file, input);
        input
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Database, SourceKind, source_len};
    use crate::types::FileId;

    // A database that counts query executions, so a test can tell a result that was
    // recomputed from one that was reused.
    fn counting_database() -> (Database, Arc<AtomicUsize>) {
        let executions = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&executions);
        let database = Database {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                if let salsa::EventKind::WillExecute { .. } = event.kind {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }))),
            files: BTreeMap::new(),
        };
        (database, executions)
    }

    fn id(index: u32) -> FileId {
        FileId::new(index)
    }

    #[test]
    fn a_save_without_edits_does_not_rerun_the_query() {
        let (mut db, executions) = counting_database();
        let file = db.upsert_source(
            id(7),
            PathBuf::from("src/a.ts"),
            Arc::from("const a = 1;"),
            11,
            SourceKind::Project,
        );
        assert_eq!(source_len(&db, file), 12);
        assert_eq!(executions.load(Ordering::SeqCst), 1);

        let again = db.upsert_source(
            id(7),
            PathBuf::from("src/a.ts"),
            Arc::from("const a = 1;"),
            11,
            SourceKind::Project,
        );
        assert_eq!(again, file);
        assert_eq!(source_len(&db, again), 12);
        assert_eq!(executions.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_changed_hash_updates_the_input_in_place_and_reruns_the_query() {
        let (mut db, executions) = counting_database();
        let file = db.upsert_source(
            id(1),
            PathBuf::from("a.ts"),
            Arc::from("a"),
            10,
            SourceKind::Project,
        );
        assert_eq!(source_len(&db, file), 1);

        let updated = db.upsert_source(
            id(1),
            PathBuf::from("a.ts"),
            Arc::from("ab"),
            20,
            SourceKind::Project,
        );
        assert_eq!(updated, file);
        assert_eq!(source_len(&db, updated), 2);
        assert_eq!(executions.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_file_is_found_by_its_file_id() {
        let mut db = Database::new();
        assert!(db.source(id(3)).is_none());
        let file = db.upsert_source(
            id(3),
            PathBuf::from("node_modules/p/index.d.ts"),
            Arc::from("export {};"),
            5,
            SourceKind::Dependency,
        );
        assert_eq!(db.source(id(3)), Some(file));
        assert_eq!(file.file_id(&db), 3);
        assert_eq!(file.path(&db), Path::new("node_modules/p/index.d.ts"));
    }
}
