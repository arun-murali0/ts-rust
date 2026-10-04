use std::collections::BTreeMap;

use petgraph::Direction;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::types::FileId;

// The import edges of a project as a petgraph graph, built from a ModuleGraph with
// ModuleGraph::topology. LLD 4.1 and 6.6 give petgraph two jobs: the order files run
// in, with the cycles between them, and the reverse edges for display and tooling.
// Deciding what must be recomputed after a change is not one of them; that belongs to
// the query layer (see incremental.rs), and walking reverse edges to mark files dirty
// would be a second mechanism that can disagree with it. So there is deliberately no
// "what does a change reach" search here.
//
// ModuleGraph already finds its own components with a hand-written iterative Tarjan,
// from before petgraph was in the tree. This is the petgraph version of the same
// answer, and a test in tests/module_resolution.rs holds the two to the same result.
// Nodes carry FileId and NodeIndex stays private, so nothing outside this file
// depends on how the graph is stored. Edge kinds (LLD 4.2) are not weights yet.
#[derive(Debug, Default)]
pub struct ModuleTopology {
    graph: DiGraph<FileId, ()>,
    nodes: BTreeMap<FileId, NodeIndex>,
}

impl ModuleTopology {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that from imports to. Adding the same pair twice is one edge.
    pub fn add_dependency(&mut self, from: FileId, to: FileId) {
        let from_node = self.node(from);
        let to_node = self.node(to);
        if self.graph.find_edge(from_node, to_node).is_none() {
            self.graph.add_edge(from_node, to_node, ());
        }
    }

    /// Records a file with no edges yet, so it is part of the topology either way.
    pub fn add_file(&mut self, file: FileId) {
        self.node(file);
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The files file imports, in id order.
    pub fn dependencies(&self, file: FileId) -> Vec<FileId> {
        self.neighbors(file, Direction::Outgoing)
    }

    /// The files that import file, in id order. For display and tooling only.
    pub fn dependents(&self, file: FileId) -> Vec<FileId> {
        self.neighbors(file, Direction::Incoming)
    }

    /// The strongly connected components, dependencies first: a component comes after
    /// every component its files import. Members are in id order. Files that are not
    /// in a cycle are components of one.
    pub fn components(&self) -> Vec<Vec<FileId>> {
        // petgraph emits components in reverse topological order of the condensation,
        // which for importer -> imported edges is dependencies first. LLD 4.1 asks for
        // that to be asserted and not assumed, so components_come_dependencies_first
        // below pins it for the pinned version.
        tarjan_scc(&self.graph)
            .into_iter()
            .map(|component| {
                let mut members: Vec<FileId> =
                    component.into_iter().map(|node| self.graph[node]).collect();
                members.sort();
                members
            })
            .collect()
    }

    /// The components that are real cycles: more than one file, or a file that imports
    /// itself. Ordered by their first member.
    pub fn cycles(&self) -> Vec<Vec<FileId>> {
        let mut cycles: Vec<Vec<FileId>> = self
            .components()
            .into_iter()
            .filter(|members| match members.as_slice() {
                [only] => self.dependencies(*only).contains(only),
                _ => true,
            })
            .collect();
        cycles.sort();
        cycles
    }

    fn node(&mut self, file: FileId) -> NodeIndex {
        if let Some(&node) = self.nodes.get(&file) {
            return node;
        }
        let node = self.graph.add_node(file);
        self.nodes.insert(file, node);
        node
    }

    fn neighbors(&self, file: FileId, direction: Direction) -> Vec<FileId> {
        let Some(&node) = self.nodes.get(&file) else {
            return Vec::new();
        };
        let mut files: Vec<FileId> = self
            .graph
            .neighbors_directed(node, direction)
            .map(|neighbor| self.graph[neighbor])
            .collect();
        // petgraph lists the newest edge first, so sort: the answer should not depend
        // on the order the edges were added in.
        files.sort();
        files
    }
}

#[cfg(test)]
mod tests {
    use super::ModuleTopology;
    use crate::types::FileId;

    fn id(index: u32) -> FileId {
        FileId::new(index)
    }

    fn position(components: &[Vec<FileId>], file: FileId) -> usize {
        components
            .iter()
            .position(|members| members.contains(&file))
            .expect("every file is in some component")
    }

    #[test]
    fn reverse_lookup_returns_importers() {
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(1), id(2));
        assert_eq!(topology.dependencies(id(1)), [id(2)]);
        assert_eq!(topology.dependents(id(2)), [id(1)]);
        assert!(topology.dependents(id(1)).is_empty());
    }

    #[test]
    fn neighbors_come_back_in_id_order_whatever_order_they_were_added_in() {
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(0), id(3));
        topology.add_dependency(id(0), id(1));
        topology.add_dependency(id(0), id(2));
        assert_eq!(topology.dependencies(id(0)), [id(1), id(2), id(3)]);
    }

    #[test]
    fn the_same_import_added_twice_is_one_edge() {
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(1), id(2));
        topology.add_dependency(id(1), id(2));
        assert_eq!(topology.dependencies(id(1)), [id(2)]);
        assert_eq!(topology.len(), 2);
    }

    #[test]
    fn components_come_dependencies_first() {
        // 1 imports 2 imports 3, so the order has to be 3, 2, 1.
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(1), id(2));
        topology.add_dependency(id(2), id(3));
        assert_eq!(
            topology.components(),
            [vec![id(3)], vec![id(2)], vec![id(1)]]
        );
    }

    #[test]
    fn a_diamond_puts_the_shared_dependency_before_everything_that_imports_it() {
        // 0 imports 1 and 2, and both import 3.
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(0), id(1));
        topology.add_dependency(id(0), id(2));
        topology.add_dependency(id(1), id(3));
        topology.add_dependency(id(2), id(3));
        let components = topology.components();
        assert!(position(&components, id(3)) < position(&components, id(1)));
        assert!(position(&components, id(3)) < position(&components, id(2)));
        assert!(position(&components, id(1)) < position(&components, id(0)));
        assert!(position(&components, id(2)) < position(&components, id(0)));
    }

    #[test]
    fn a_ring_is_one_component_that_comes_before_its_importer() {
        // 1 -> 2 -> 3 -> 1, and 0 imports the ring.
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(0), id(1));
        topology.add_dependency(id(1), id(2));
        topology.add_dependency(id(2), id(3));
        topology.add_dependency(id(3), id(1));
        assert_eq!(
            topology.components(),
            [vec![id(1), id(2), id(3)], vec![id(0)]]
        );
        assert_eq!(topology.cycles(), [vec![id(1), id(2), id(3)]]);
    }

    #[test]
    fn a_file_that_imports_itself_is_a_cycle_and_a_plain_leaf_is_not() {
        let mut topology = ModuleTopology::new();
        topology.add_dependency(id(4), id(4));
        topology.add_dependency(id(5), id(6));
        assert_eq!(topology.cycles(), [vec![id(4)]]);
    }

    #[test]
    fn an_empty_topology_has_no_components() {
        let topology = ModuleTopology::new();
        assert!(topology.is_empty());
        assert!(topology.components().is_empty());
        assert!(topology.cycles().is_empty());
    }
}
