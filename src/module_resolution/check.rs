use std::fs;

use rayon::prelude::*;

use crate::CheckSession;
use crate::diagnostics::Diagnostic;
use crate::error::CheckerError;
use crate::types::FileId;

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
/// Each file is still checked on its own: an import is not yet looked up in the file it
/// names. The layering is here so that, once it is, a file's dependencies have always
/// been checked before it is.
pub fn check_project(graph: &ModuleGraph) -> ProjectReport {
    let mut files = Vec::new();
    for layer in graph.layers() {
        let mut reports: Vec<FileReport> = layer
            .par_iter()
            .filter(|id| graph.is_checkable(**id))
            .map(|id| check_file(graph, *id))
            .collect();
        files.append(&mut reports);
    }
    files.sort_by_key(|report| report.file_id);
    ProjectReport { files }
}

// One session per file. A session reuses its arena across checks of the same file, but
// here every file is checked once, and a session per file keeps the workers from
// sharing anything mutable.
fn check_file(graph: &ModuleGraph, id: FileId) -> FileReport {
    let outcome = match graph.files().path(id) {
        None => FileOutcome::ReadFailed(format!("no path recorded for file {}", id.index())),
        Some(path) => match fs::read_to_string(path) {
            Err(error) => FileOutcome::ReadFailed(format!("{}: {error}", path.display())),
            Ok(source) => {
                match CheckSession::new(id).check_source(&source, &path.to_string_lossy()) {
                    Ok(result) => FileOutcome::Checked(result.diagnostics),
                    Err(error) => FileOutcome::CheckFailed(error),
                }
            }
        },
    };
    FileReport {
        file_id: id,
        outcome,
    }
}
