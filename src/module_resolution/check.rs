use std::fs;
use std::sync::Arc;

use rayon::prelude::*;

use crate::diagnostics::Diagnostic;
use crate::error::CheckerError;
use crate::fxhash::FxHashMap;
use crate::keys::DeclKey;
use crate::portable::{DeclProvider, ImportSource, ModuleExports, PortableDecl};
use crate::types::FileId;
use crate::unit::{UnitContext, UnitError, Worker};

use super::graph::ModuleGraph;

// What happened to one file. A file that could not be read or parsed is reported as
// such and is never left out, because a report that silently omits files reads as "all
// clear" for exactly the files nobody looked at.
#[derive(Debug)]
pub enum FileOutcome {
    Checked(Vec<Diagnostic>),
    ReadFailed(String),
    CheckFailed(CheckerError),
}

#[derive(Debug)]
pub struct FileReport {
    pub file_id: FileId,
    pub outcome: FileOutcome,
    /// The declarations of other files this file's check read, sorted and without
    /// duplicates. A key carries its module, so this is both the file-level dependency
    /// and the finer one: a change that leaves all of these signatures alone cannot
    /// change this file's diagnostics (LLD 6.2).
    pub reads: Vec<DeclKey>,
}

#[derive(Debug, Default)]
pub struct ProjectReport {
    pub files: Vec<FileReport>,
}

impl ProjectReport {
    pub fn diagnostic_count(&self) -> usize {
        self.files
            .iter()
            .map(|report| match &report.outcome {
                FileOutcome::Checked(diagnostics) => diagnostics.len(),
                FileOutcome::ReadFailed(_) | FileOutcome::CheckFailed(_) => 0,
            })
            .sum()
    }

    /// Files that were not checked at all, as opposed to checked and found wrong.
    pub fn failure_count(&self) -> usize {
        self.files
            .iter()
            .filter(|report| !matches!(report.outcome, FileOutcome::Checked(_)))
            .count()
    }
}

/// Checks every checkable file of `graph`, one layer at a time, the files of a layer in
/// parallel. The report is sorted by file id, so it does not depend on which thread
/// finished first.
///
/// An import is looked up in the file it names: a file is checked only after everything
/// it imports has been, and what those files export (their annotated interfaces, aliases,
/// functions and constants, as portable declarations) is what its imports bind to. Files
/// of one layer are checked against the same snapshot, taken before the layer starts, so
/// the result does not depend on how the threads interleave. The members of a cycle share
/// a layer, so their first pass finds nothing among themselves; they are then checked
/// again against what that pass exported, until what they export stops changing (a few
/// rounds at most). An import of a file that is not checked (a dependency's declaration
/// file) binds nothing, which leaves the name unknown: it loses an error and never
/// invents one.
pub fn check_project(graph: &ModuleGraph) -> ProjectReport {
    let mut files = Vec::new();
    let mut exports = ProjectExports::default();
    let in_cycle: std::collections::HashSet<FileId> =
        graph.cycles().into_iter().flatten().collect();
    for layer in graph.layers() {
        let mut done = check_files(graph, &exports, &layer);
        let members: Vec<FileId> = layer
            .iter()
            .copied()
            .filter(|id| in_cycle.contains(id))
            .collect();
        if !members.is_empty() {
            settle_cycle(graph, &mut exports, &members, &mut done);
        }
        for (report, unit_exports) in done {
            if let Some(unit_exports) = unit_exports {
                exports.add(report.file_id, unit_exports);
            }
            // A package's declaration file is read for what it exports and never reported.
            if !graph.is_dependency_declaration(report.file_id) {
                files.push(report);
            }
        }
    }
    files.sort_by_key(|report| report.file_id);
    ProjectReport { files }
}

// The most times the members of a cycle are checked again after the first pass.
const CYCLE_ROUNDS: usize = 4;

// Checks the files of one layer in parallel, against `exports` as it stands.
fn check_files(
    graph: &ModuleGraph,
    exports: &ProjectExports,
    ids: &[FileId],
) -> Vec<(FileReport, Option<UnitExports>)> {
    ids.par_iter()
        .filter(|id| graph.is_checkable(**id) || graph.is_dependency_declaration(**id))
        // One worker per thread, so its arena is reused from file to file; the
        // worker clears it after every unit, so nothing carries over.
        .map_init(Worker::new, |worker, id| {
            check_file(graph, exports, worker, *id)
        })
        .collect()
}

// Members of a cycle import each other, so the first pass finds nothing among them. What
// the first pass exports is then handed to a second pass over the members, and so on
// until what they export stops changing (or CYCLE_ROUNDS passes have run). Every pass
// reads the snapshot the previous one left, so the result does not depend on thread
// timing. `done` holds the members' results from the last pass.
fn settle_cycle(
    graph: &ModuleGraph,
    exports: &mut ProjectExports,
    members: &[FileId],
    done: &mut [(FileReport, Option<UnitExports>)],
) {
    for _ in 0..CYCLE_ROUNDS {
        let before = fingerprint(done, members);
        for (report, unit_exports) in done.iter() {
            if let Some(unit_exports) = unit_exports {
                if members.contains(&report.file_id) {
                    exports.add(report.file_id, unit_exports.clone());
                }
            }
        }
        let again = check_files(graph, exports, members);
        let settled = fingerprint(&again, members) == before;
        for (report, unit_exports) in again {
            if let Some(slot) = done.iter_mut().find(|(r, _)| r.file_id == report.file_id) {
                *slot = (report, unit_exports);
            }
        }
        if settled {
            return;
        }
    }
}

// What the members export, in a form two passes can be compared by.
fn fingerprint(
    done: &[(FileReport, Option<UnitExports>)],
    members: &[FileId],
) -> Vec<(
    FileId,
    bool,
    Vec<(String, DeclKey)>,
    Vec<crate::keys::SigHash>,
)> {
    let mut seen: Vec<_> = done
        .iter()
        .filter(|(report, _)| members.contains(&report.file_id))
        .filter_map(|(report, unit)| {
            unit.as_ref().map(|unit| {
                (
                    report.file_id,
                    unit.complete,
                    unit.names.clone(),
                    unit.decls.iter().map(|decl| decl.sig).collect(),
                )
            })
        })
        .collect();
    seen.sort_by_key(|entry| entry.0);
    seen
}

// What one checked file hands to the files that import it.
#[derive(Clone)]
struct UnitExports {
    names: Vec<(String, DeclKey)>,
    decls: Vec<PortableDecl>,
    // Whether `names` is the module's whole export list (see UnitOutput).
    complete: bool,
}

// Everything the checked files export so far: which names each module exports, and the
// declarations by key. Keys carry their module, so one map holds every module's. Only ever
// looked up, never walked, so hash order cannot reach a result (LLD 8.2).
#[derive(Default)]
struct ProjectExports {
    names: FxHashMap<FileId, Vec<(String, DeclKey)>>,
    complete: FxHashMap<FileId, bool>,
    decls: FxHashMap<DeclKey, Arc<PortableDecl>>,
}

impl ProjectExports {
    fn add(&mut self, id: FileId, unit: UnitExports) {
        self.names.insert(id, unit.names);
        self.complete.insert(id, unit.complete);
        for decl in unit.decls {
            self.decls.insert(decl.key, Arc::new(decl));
        }
    }
}

// The imports of one file: its specifiers are resolved through the graph's own edges, so
// the resolution the graph already did (and cached) is the one used here.
struct ImportsOf<'a> {
    graph: &'a ModuleGraph,
    exports: &'a ProjectExports,
    from: FileId,
    // Every declaration the unit asked for, in the order it asked. A unit runs on one
    // thread, so the lock is never contended; it is there because the provider is
    // handed out as a shared reference.
    reads: std::sync::Mutex<Vec<DeclKey>>,
}

impl ImportsOf<'_> {
    fn take_reads(&self) -> Vec<DeclKey> {
        let mut reads = std::mem::take(&mut *self.reads.lock().unwrap_or_else(|e| e.into_inner()));
        reads.sort();
        reads.dedup();
        reads
    }
}

impl DeclProvider for ImportsOf<'_> {
    fn decl(&self, key: DeclKey) -> Option<Arc<PortableDecl>> {
        self.reads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(key);
        self.exports.decls.get(&key).cloned()
    }
}

// An import is answered through the edge the graph already resolved for this file and
// specifier, so a specifier means the same file here as when the graph was built and
// nothing is resolved during a check. A star re-export never carries `default`, as in the
// language.
impl ImportSource for ImportsOf<'_> {
    fn export_all(&self, specifier: &str) -> Vec<(String, DeclKey)> {
        let Some(target) = self
            .graph
            .edges(self.from)
            .iter()
            .find(|edge| edge.specifier == specifier)
            .and_then(|edge| edge.target)
        else {
            return Vec::new();
        };
        self.exports
            .names
            .get(&target)
            .into_iter()
            .flatten()
            .filter(|(name, _)| name != "default")
            .cloned()
            .collect()
    }

    fn module_exports(&self, specifier: &str) -> ModuleExports {
        let Some(edge) = self
            .graph
            .edges(self.from)
            .iter()
            .find(|edge| edge.specifier == specifier)
        else {
            return ModuleExports::Unknown;
        };
        let Some(target) = edge.target else {
            return ModuleExports::Unresolved;
        };
        match self.exports.complete.get(&target) {
            Some(complete) => ModuleExports::Known {
                complete: *complete,
            },
            None => ModuleExports::Unknown,
        }
    }

    fn export_keys(&self, specifier: &str, name: &str) -> Vec<DeclKey> {
        let Some(target) = self
            .graph
            .edges(self.from)
            .iter()
            .find(|edge| edge.specifier == specifier)
            .and_then(|edge| edge.target)
        else {
            return Vec::new();
        };
        self.exports
            .names
            .get(&target)
            .into_iter()
            .flatten()
            .filter(|(exported, _)| exported == name)
            .map(|(_, key)| *key)
            .collect()
    }
}

// One unit per file. The worker is the thread's own, so nothing mutable is shared.
fn check_file(
    graph: &ModuleGraph,
    exports: &ProjectExports,
    worker: &mut Worker,
    id: FileId,
) -> (FileReport, Option<UnitExports>) {
    let mut unit_exports = None;
    let mut reads = Vec::new();
    let outcome = match graph.files().path(id) {
        None => FileOutcome::ReadFailed(format!("no path recorded for file {}", id.index())),
        Some(path) => match fs::read_to_string(path) {
            Err(error) => FileOutcome::ReadFailed(format!("{}: {error}", path.display())),
            Ok(source) => {
                let name = path.to_string_lossy();
                let imports = ImportsOf {
                    graph,
                    exports,
                    from: id,
                    reads: std::sync::Mutex::new(Vec::new()),
                };
                let mut ctx = UnitContext::new(id, &name, &source).with_imports(&imports);
                // A file with no stable key (inside node_modules, until package keys
                // exist) can import but exports nothing.
                if let Some(module) = graph.module_key(id) {
                    ctx = ctx.with_module(module);
                }
                let checked = worker.check_unit(&ctx);
                reads = imports.take_reads();
                match checked {
                    Ok(output) => {
                        unit_exports = Some(UnitExports {
                            names: output.export_names,
                            decls: output.exports,
                            complete: output.exports_complete,
                        });
                        FileOutcome::Checked(output.diagnostics)
                    }
                    Err(UnitError::Checker(error)) => FileOutcome::CheckFailed(error),
                    // No token is given to these units, so nothing cancels them.
                    Err(UnitError::Cancelled) => {
                        FileOutcome::ReadFailed("the check was cancelled".to_string())
                    }
                }
            }
        },
    };
    (
        FileReport {
            file_id: id,
            outcome,
            reads,
        },
        unit_exports,
    )
}
