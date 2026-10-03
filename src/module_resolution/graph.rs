use std::fs;
use std::path::{Path, PathBuf};

use crate::project::ProjectFiles;
use crate::types::FileId;

use super::discovery::{ModuleRequest, scan_module_requests};
use super::error::ModuleError;
use super::metadata::{FileFingerprint, FileState, content_hash};
use super::resolver::ModuleResolver;

// One import or re-export statement, kept whether or not it resolved. `target` is None
// for an import that did not, and it stays in the graph: dropping it would make a file
// with a broken import look like a file with no imports, and a later phase needs the
// specifier to report the missing module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleEdge {
    pub specifier: String,
    pub target: Option<FileId>,
    pub is_type: bool,
    pub is_import: bool,
}

// The files reachable from a project's entry points and the imports between them.
// Everything is indexed by FileId, which ProjectFiles hands out in discovery order, so
// the vectors below are plain arrays and a lookup never hashes a path.
//
// `dependencies` and `dependents` are the resolved edges with duplicates removed (two
// statements importing the same file are one dependency), in both directions, so
// "what must be checked first" and "what is affected by a change" are each one lookup.
#[derive(Debug)]
pub struct ModuleGraph {
    files: ProjectFiles,
    edges: Vec<Vec<ModuleEdge>>,
    dependencies: Vec<Vec<FileId>>,
    dependents: Vec<Vec<FileId>>,
    states: Vec<FileState>,
    syntax_errors: Vec<bool>,
}

impl ModuleGraph {
    /// Reads every file reachable from `entries` and resolves its imports. Entries must
    /// be absolute so that one file is never interned under two spellings of its path.
    pub fn build(entries: &[PathBuf], resolver: &ModuleResolver) -> Result<Self, ModuleError> {
        let mut files = ProjectFiles::new();
        for entry in entries {
            if !entry.is_absolute() {
                return Err(ModuleError::RelativeEntry(entry.clone()));
            }
            files.intern(entry.clone());
        }

        let mut edges = Vec::new();
        let mut states = Vec::new();
        let mut syntax_errors = Vec::new();

        // Files are interned as their importers are scanned, so the list grows while it
        // is walked. Visiting ids in order is a breadth-first walk with no queue.
        let mut next = 0;
        while next < files.len() {
            let id = file_id(next);
            let path = files
                .path(id)
                .expect("every interned id has a path")
                .to_path_buf();

            // The fingerprint is taken before the read. If the file changes in between,
            // the recorded fingerprint is the older one and the change is still seen.
            let fingerprint = FileFingerprint::of(&path).map_err(|source| ModuleError::Read {
                path: path.clone(),
                source,
            })?;
            let bytes = fs::read(&path).map_err(|source| ModuleError::Read {
                path: path.clone(),
                source,
            })?;
            states.push(FileState::new(fingerprint, content_hash(&bytes)));

            let mut file_edges = Vec::new();
            let mut failed = false;
            if is_scannable(&path) {
                let source = String::from_utf8_lossy(&bytes);
                let scan = scan_module_requests(&source, &path.to_string_lossy());
                failed = scan.parse_failed;
                for request in scan.requests {
                    let target = resolve_request(resolver, &path, &request)
                        .map(|resolved| files.intern(resolved));
                    file_edges.push(ModuleEdge {
                        specifier: request.specifier,
                        target,
                        is_type: request.is_type,
                        is_import: request.is_import,
                    });
                }
            }
            edges.push(file_edges);
            syntax_errors.push(failed);
            next += 1;
        }

        Ok(Self::assemble(files, edges, states, syntax_errors))
    }

    fn assemble(
        files: ProjectFiles,
        edges: Vec<Vec<ModuleEdge>>,
        states: Vec<FileState>,
        syntax_errors: Vec<bool>,
    ) -> Self {
        let mut dependencies = Vec::with_capacity(edges.len());
        let mut dependents = vec![Vec::new(); edges.len()];
        for (position, file_edges) in edges.iter().enumerate() {
            let mut targets: Vec<FileId> =
                file_edges.iter().filter_map(|edge| edge.target).collect();
            targets.sort();
            targets.dedup();
            // Importers are visited in id order, so each dependents list ends up sorted
            // without a sort of its own.
            for target in &targets {
                dependents[index_of(*target)].push(file_id(position));
            }
            dependencies.push(targets);
        }
        Self {
            files,
            edges,
            dependencies,
            dependents,
            states,
            syntax_errors,
        }
    }

    pub fn files(&self) -> &ProjectFiles {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Every import and re-export statement of `id`, in source order.
    pub fn edges(&self, id: FileId) -> &[ModuleEdge] {
        slice_of(&self.edges, id)
    }

    /// The files `id` imports, once each, in id order.
    pub fn dependencies(&self, id: FileId) -> &[FileId] {
        slice_of(&self.dependencies, id)
    }

    /// The files that import `id`, in id order.
    pub fn dependents(&self, id: FileId) -> &[FileId] {
        slice_of(&self.dependents, id)
    }

    /// Whether the parser reported a syntax error in `id`. Such a file may have fewer
    /// edges than its source suggests.
    pub fn has_syntax_errors(&self, id: FileId) -> bool {
        self.syntax_errors
            .get(index_of(id))
            .is_some_and(|failed| *failed)
    }

    pub fn unresolved(&self) -> impl Iterator<Item = (FileId, &ModuleEdge)> {
        self.edges
            .iter()
            .enumerate()
            .flat_map(|(position, file_edges)| {
                file_edges
                    .iter()
                    .filter(|edge| edge.target.is_none())
                    .map(move |edge| (file_id(position), edge))
            })
    }

    /// Whether the checker should be run on `id`: TypeScript source that is not inside
    /// `node_modules`. Declaration files of dependencies are in the graph because
    /// their imports matter, but checking someone else's package is not this
    /// project's job.
    pub fn is_checkable(&self, id: FileId) -> bool {
        self.files
            .path(id)
            .is_some_and(|path| is_typescript(path) && !in_node_modules(path))
    }

    /// The files whose contents differ from what the graph read. A rewrite that leaves
    /// the bytes alone is not a change.
    pub fn changed_files(&self) -> Vec<FileId> {
        (0..self.states.len())
            .map(file_id)
            .filter(|id| {
                self.files
                    .path(*id)
                    .is_none_or(|path| !self.states[index_of(*id)].is_unchanged(path))
            })
            .collect()
    }

    /// Groups of files that import each other, directly or through others. A file that
    /// imports itself is a group of one. Members are sorted, and so are the groups.
    pub fn cycles(&self) -> Vec<Vec<FileId>> {
        let mut cycles: Vec<Vec<FileId>> = self
            .components()
            .into_iter()
            .filter(|component| {
                component.len() > 1 || self.dependencies(component[0]).contains(&component[0])
            })
            .collect();
        cycles.sort();
        cycles
    }

    /// Files grouped so that everything in a layer depends only on earlier layers,
    /// except for imports among members of one cycle, which share a layer. A file
    /// that merely imports a cycle comes after it and is not part of it.
    pub fn layers(&self) -> Vec<Vec<FileId>> {
        let components = self.components();
        let mut component_of: Vec<usize> = vec![0; self.len()];
        for (position, component) in components.iter().enumerate() {
            for member in component {
                component_of[index_of(*member)] = position;
            }
        }

        // `components` lists a component after every component it imports from, so each
        // depth is final by the time something imports it.
        let mut depth: Vec<usize> = vec![0; components.len()];
        let mut layers: Vec<Vec<FileId>> = Vec::new();
        for (position, component) in components.iter().enumerate() {
            let mut level = 0usize;
            for member in component {
                for dependency in self.dependencies(*member) {
                    let other = component_of[index_of(*dependency)];
                    if other != position {
                        level = level.max(depth[other] + 1);
                    }
                }
            }
            depth[position] = level;
            if layers.len() <= level {
                layers.resize_with(level + 1, Vec::new);
            }
            layers[level].extend(component.iter().copied());
        }
        for layer in &mut layers {
            layer.sort();
        }
        layers
    }

    // Strongly connected components, Tarjan's algorithm with an explicit stack. An
    // import chain is as deep as the project's longest dependency path, and recursion
    // would put that on the thread's call stack, where a long enough chain overflows it.
    //
    // Components come out in reverse topological order: one is emitted only after every
    // component it imports from. `layers` relies on that.
    fn components(&self) -> Vec<Vec<FileId>> {
        const UNVISITED: u32 = u32::MAX;

        let count = self.len();
        let mut order = vec![UNVISITED; count];
        let mut low_link = vec![0u32; count];
        let mut on_stack = vec![false; count];
        let mut stack: Vec<usize> = Vec::new();
        let mut components = Vec::new();
        let mut next_order = 0u32;
        // Each frame is a file and how many of its dependencies have been looked at.
        let mut frames: Vec<(usize, usize)> = Vec::new();

        for root in 0..count {
            if order[root] != UNVISITED {
                continue;
            }
            order[root] = next_order;
            low_link[root] = next_order;
            next_order += 1;
            stack.push(root);
            on_stack[root] = true;
            frames.push((root, 0));

            while let Some(frame) = frames.last_mut() {
                let node = frame.0;
                let dependencies = &self.dependencies[node];
                if frame.1 < dependencies.len() {
                    let next = index_of(dependencies[frame.1]);
                    frame.1 += 1;
                    if order[next] == UNVISITED {
                        order[next] = next_order;
                        low_link[next] = next_order;
                        next_order += 1;
                        stack.push(next);
                        on_stack[next] = true;
                        frames.push((next, 0));
                    } else if on_stack[next] {
                        low_link[node] = low_link[node].min(order[next]);
                    }
                    continue;
                }

                frames.pop();
                if let Some(parent) = frames.last() {
                    let parent = parent.0;
                    low_link[parent] = low_link[parent].min(low_link[node]);
                }
                if low_link[node] == order[node] {
                    let mut component = Vec::new();
                    while let Some(member) = stack.pop() {
                        on_stack[member] = false;
                        component.push(file_id(member));
                        if member == node {
                            break;
                        }
                    }
                    component.sort();
                    components.push(component);
                }
            }
        }
        components
    }
}

// A value import is resolved the way a runtime would, falling back to declaration
// files when that finds nothing, because a package can ship types and no code. A type
// import skips the first step: its target is always a declaration, so whichever
// JavaScript entry the plain resolver would pick is the wrong file.
fn resolve_request(
    resolver: &ModuleResolver,
    from: &Path,
    request: &ModuleRequest,
) -> Option<PathBuf> {
    if request.is_type {
        return resolver
            .resolve_dts_from_file(from, &request.specifier)
            .ok();
    }
    resolver
        .resolve_from_file(from, &request.specifier)
        .or_else(|_| resolver.resolve_dts_from_file(from, &request.specifier))
        .ok()
}

fn extension(path: &Path) -> Option<&str> {
    path.extension().and_then(|ext| ext.to_str())
}

// Resolved imports can land on JSON or other assets. They are nodes, so the importer's
// edge has somewhere to point, but there is no module syntax in them to scan.
fn is_scannable(path: &Path) -> bool {
    matches!(
        extension(path),
        Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs")
    )
}

fn is_typescript(path: &Path) -> bool {
    matches!(extension(path), Some("ts" | "tsx" | "mts" | "cts"))
}

fn in_node_modules(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "node_modules")
}

fn file_id(index: usize) -> FileId {
    FileId::new(u32::try_from(index).expect("project contains too many files"))
}

fn index_of(id: FileId) -> usize {
    id.index() as usize
}

fn slice_of<T>(table: &[Vec<T>], id: FileId) -> &[T] {
    table.get(index_of(id)).map(Vec::as_slice).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::{ModuleEdge, ModuleGraph, file_id};
    use crate::project::ProjectFiles;
    use crate::types::FileId;

    // A graph over `count` files named n0.ts, n1.ts, ... with the given import pairs,
    // for the algorithms, which do not care where an edge came from.
    fn graph(count: usize, imports: &[(usize, usize)]) -> ModuleGraph {
        let mut files = ProjectFiles::new();
        for position in 0..count {
            files.intern(format!("n{position}.ts"));
        }
        let mut edges = vec![Vec::new(); count];
        for (from, to) in imports {
            edges[*from].push(ModuleEdge {
                specifier: format!("./n{to}"),
                target: Some(file_id(*to)),
                is_type: false,
                is_import: true,
            });
        }
        let states = Vec::new();
        ModuleGraph::assemble(files, edges, states, vec![false; count])
    }

    fn ids(indexes: &[usize]) -> Vec<FileId> {
        indexes.iter().map(|index| file_id(*index)).collect()
    }

    #[test]
    fn a_long_import_chain_does_not_overflow_the_stack() {
        let length = 50_000;
        let imports: Vec<_> = (0..length - 1).map(|n| (n, n + 1)).collect();
        let graph = graph(length, &imports);

        assert!(graph.cycles().is_empty());
        let layers = graph.layers();
        assert_eq!(layers.len(), length);
        assert_eq!(layers[0], ids(&[length - 1]));
        assert_eq!(layers[length - 1], ids(&[0]));
    }

    #[test]
    fn a_ring_is_one_cycle_and_its_importer_is_not_in_it() {
        // 0 imports the ring 1 -> 2 -> 3 -> 1.
        let graph = graph(4, &[(0, 1), (1, 2), (2, 3), (3, 1)]);
        assert_eq!(graph.cycles(), vec![ids(&[1, 2, 3])]);
    }

    #[test]
    fn a_file_importing_itself_is_a_cycle_of_one() {
        let graph = graph(2, &[(0, 0), (1, 0)]);
        assert_eq!(graph.cycles(), vec![ids(&[0])]);
    }

    #[test]
    fn cycle_members_share_a_layer_and_their_importer_follows() {
        // 0 imports the ring 1 <-> 2, and the ring imports the leaf 3.
        let graph = graph(4, &[(0, 1), (1, 2), (2, 1), (2, 3)]);
        assert_eq!(graph.layers(), vec![ids(&[3]), ids(&[1, 2]), ids(&[0])]);
    }

    #[test]
    fn a_diamond_puts_both_sides_in_one_layer() {
        let graph = graph(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
        assert_eq!(graph.layers(), vec![ids(&[3]), ids(&[1, 2]), ids(&[0])]);
    }

    #[test]
    fn dependents_are_the_reverse_of_dependencies() {
        let graph = graph(3, &[(0, 2), (1, 2), (0, 2)]);
        assert_eq!(graph.dependencies(file_id(0)), ids(&[2]));
        assert_eq!(graph.dependents(file_id(2)), ids(&[0, 1]));
        assert!(graph.dependents(file_id(0)).is_empty());
    }
}
