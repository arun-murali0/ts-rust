use std::fs;
use std::sync::Arc;

use rayon::prelude::*;

use crate::diagnostics::Diagnostic;
use crate::error::CheckerError;
use crate::fxhash::FxHashMap;
use crate::keys::DeclKey;
use crate::portable::{DeclProvider, ImportSource, PortableDecl};
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
/// the result does not depend on how the threads interleave. Imports among the members of
/// a cycle find nothing yet (they share a layer), and an import of a file that is not
/// checked (a dependency's declaration file) binds nothing; both leave the name unknown,
/// which loses an error and never invents one.
pub fn check_project(graph: &ModuleGraph) -> ProjectReport {
    let mut files = Vec::new();
    let mut exports = ProjectExports::default();
    for layer in graph.layers() {
        let done: Vec<(FileReport, Option<UnitExports>)> = layer
            .par_iter()
            .filter(|id| graph.is_checkable(**id))
            // One worker per thread, so its arena is reused from file to file; the
            // worker clears it after every unit, so nothing carries over.
            .map_init(Worker::new, |worker, id| {
                check_file(graph, &exports, worker, *id)
            })
            .collect();
        for (report, unit_exports) in done {
            if let Some(unit_exports) = unit_exports {
                exports.add(report.file_id, unit_exports);
            }
            files.push(report);
        }
    }
    files.sort_by_key(|report| report.file_id);
    ProjectReport { files }
}

// What one checked file hands to the files that import it.
struct UnitExports {
    names: Vec<(String, DeclKey)>,
    decls: Vec<PortableDecl>,
}

// Everything the checked files export so far: which names each module exports, and the
// declarations by key. Keys carry their module, so one map holds every module's. Only ever
// looked up, never walked, so hash order cannot reach a result (LLD 8.2).
#[derive(Default)]
struct ProjectExports {
    names: FxHashMap<FileId, Vec<(String, DeclKey)>>,
    decls: FxHashMap<DeclKey, Arc<PortableDecl>>,
}

impl ProjectExports {
    fn add(&mut self, id: FileId, unit: UnitExports) {
        self.names.insert(id, unit.names);
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
}

impl DeclProvider for ImportsOf<'_> {
    fn decl(&self, key: DeclKey) -> Option<Arc<PortableDecl>> {
        self.exports.decls.get(&key).cloned()
    }
}

impl ImportSource for ImportsOf<'_> {
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
                };
                let mut ctx = UnitContext::new(id, &name, &source).with_imports(&imports);
                // A file with no stable key (inside node_modules, until package keys
                // exist) can import but exports nothing.
                if let Some(module) = graph.module_key(id) {
                    ctx = ctx.with_module(module);
                }
                match worker.check_unit(&ctx) {
                    Ok(output) => {
                        unit_exports = Some(UnitExports {
                            names: output.export_names,
                            decls: output.exports,
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
        },
        unit_exports,
    )
}
