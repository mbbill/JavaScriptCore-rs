//! Tracing interfaces used by the collector.

use std::collections::VecDeque;

use crate::gc::{CellId, CellType, GcRef, JsCell, MarkDependency};

/// Payload contract for visiting strong GC children.
///
/// Implementors expose borrowed child references to a collector visitor. The
/// trait does not grant ownership of children or authority to mutate fields.
pub trait Trace {
    fn trace(&self, tracer: &mut dyn Tracer);
}

/// Marking visitor interface.
///
/// The visitor owns marking work for the current collection epoch. Visited
/// cells remain heap-owned; weak visits only record validation candidates.
pub trait Tracer {
    fn visit_cell(&mut self, cell: GcRef<JsCell>);
    fn visit_weak_cell(&mut self, cell: GcRef<JsCell>);
    fn note_external_memory(&mut self, bytes: usize);
}

/// Why a root is being marked. This mirrors the separation between ordinary
/// tracing, conservative stack discovery, and verifier/debug visitors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkReason {
    StrongRoot,
    ConservativeRoot,
    WriteBarrier,
    WeakValidation,
    Verifier,
}

/// Marking constraint scheduling policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstraintMode {
    StopTheWorld,
    Concurrent,
    Sequential,
}

/// How often a marking constraint can produce newly greyed work.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConstraintVolatility {
    #[default]
    SeldomGreyed,
    GreyedByExecution,
    GreyedByMarking,
}

/// Whether a constraint may run concurrently with mutator-visible progress.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConstraintConcurrency {
    Sequential,
    #[default]
    Concurrent,
}

/// Whether a constraint can split work across helper visitors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConstraintParallelism {
    #[default]
    Sequential,
    Parallel,
}

/// When a marking constraint should be executed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConstraintExecutionPhase {
    #[default]
    MainThread,
    Fixpoint,
    Parallel,
    Verifier,
}

/// Named marking constraint. The callback body is intentionally absent; future
/// code can attach generated or hand-written visitors behind this descriptor.
/// Constraint descriptors own scheduling metadata only; collector phase state
/// supplies the mutation authority to execute them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkingConstraint {
    pub abbreviated_name: &'static str,
    pub name: &'static str,
    pub mode: ConstraintMode,
    pub phase: ConstraintExecutionPhase,
    pub volatility: ConstraintVolatility,
    pub concurrency: ConstraintConcurrency,
    pub parallelism: ConstraintParallelism,
    pub index: Option<u32>,
    pub last_visit_count: usize,
}

/// Ordered collection of marking constraints.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkingConstraintSet {
    constraints: Vec<MarkingConstraint>,
    pub iteration: u32,
    pub unexecuted_roots: usize,
    pub unexecuted_outgrowths: usize,
}

impl MarkingConstraintSet {
    pub fn constraints(&self) -> &[MarkingConstraint] {
        &self.constraints
    }
}

/// Descriptor node for pure marking-plan graph algorithms.
///
/// The node names a heap-cell identity and static cell facts. It does not
/// borrow storage, test mark bits, or trace payload fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkingGraphNode {
    pub cell: CellId,
    pub cell_type: CellType,
    pub external_bytes: usize,
}

/// Descriptor edge for a marking-plan graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkingGraphEdge {
    pub from: CellId,
    pub to: CellId,
    pub dependency: MarkDependency,
}

/// Immutable descriptor graph consumed by planning algorithms.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkingPlanGraph {
    nodes: Vec<MarkingGraphNode>,
    edges: Vec<MarkingGraphEdge>,
}

impl MarkingPlanGraph {
    pub fn nodes(&self) -> &[MarkingGraphNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[MarkingGraphEdge] {
        &self.edges
    }

    pub fn validate(&self) -> Result<(), MarkingPlanGraphError> {
        for (index, node) in self.nodes.iter().enumerate() {
            if node.cell == CellId::default() {
                return Err(MarkingPlanGraphError::InvalidCellId(node.cell));
            }
            if self.nodes[..index]
                .iter()
                .any(|previous| previous.cell == node.cell)
            {
                return Err(MarkingPlanGraphError::DuplicateNode(node.cell));
            }
        }

        for (index, edge) in self.edges.iter().enumerate() {
            if edge.from == CellId::default() || edge.to == CellId::default() {
                return Err(MarkingPlanGraphError::InvalidCellId(
                    if edge.from == CellId::default() {
                        edge.from
                    } else {
                        edge.to
                    },
                ));
            }
            if !self.has_node(edge.from) {
                return Err(MarkingPlanGraphError::UnknownEdgeEndpoint(edge.from));
            }
            if !self.has_node(edge.to) {
                return Err(MarkingPlanGraphError::UnknownEdgeEndpoint(edge.to));
            }
            if self.edges[..index].iter().any(|previous| {
                previous.from == edge.from
                    && previous.to == edge.to
                    && previous.dependency == edge.dependency
            }) {
                return Err(MarkingPlanGraphError::DuplicateEdge {
                    from: edge.from,
                    to: edge.to,
                    dependency: edge.dependency,
                });
            }
        }

        Ok(())
    }

    pub fn plan_from_roots(&self, roots: &[CellId]) -> Result<MarkingPlan, MarkingPlanGraphError> {
        self.validate()?;
        for root in roots {
            if !self.has_node(*root) {
                return Err(MarkingPlanGraphError::UnknownRoot(*root));
            }
        }

        let mut marked = Vec::new();
        let mut weak_candidates = Vec::new();
        let mut steps = Vec::new();
        let mut queue = VecDeque::new();

        for root in roots {
            if !marked.contains(root) {
                marked.push(*root);
                queue.push_back(*root);
                steps.push(MarkingPlanStep {
                    cell: *root,
                    reason: MarkReason::StrongRoot,
                    dependency: MarkDependency::Strong,
                    referrer: None,
                });
            }
        }

        while let Some(from) = queue.pop_front() {
            for edge in self.edges.iter().filter(|edge| edge.from == from) {
                if edge.dependency == MarkDependency::Weak {
                    if !weak_candidates.contains(&edge.to) {
                        weak_candidates.push(edge.to);
                    }
                    steps.push(MarkingPlanStep {
                        cell: edge.to,
                        reason: MarkReason::WeakValidation,
                        dependency: edge.dependency,
                        referrer: Some(from),
                    });
                    continue;
                }

                if !marked.contains(&edge.to) {
                    marked.push(edge.to);
                    queue.push_back(edge.to);
                    steps.push(MarkingPlanStep {
                        cell: edge.to,
                        reason: MarkReason::StrongRoot,
                        dependency: edge.dependency,
                        referrer: Some(from),
                    });
                }
            }
        }

        let external_bytes = marked
            .iter()
            .filter_map(|cell| self.node(*cell))
            .map(|node| node.external_bytes)
            .sum();

        Ok(MarkingPlan {
            steps,
            marked_cells: marked,
            weak_candidates,
            external_bytes,
        })
    }

    fn has_node(&self, cell: CellId) -> bool {
        self.nodes.iter().any(|node| node.cell == cell)
    }

    fn node(&self, cell: CellId) -> Option<&MarkingGraphNode> {
        self.nodes.iter().find(|node| node.cell == cell)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkingPlanGraphBuilder {
    graph: MarkingPlanGraph,
}

impl MarkingPlanGraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node(mut self, cell: CellId, cell_type: CellType, external_bytes: usize) -> Self {
        self.graph.nodes.push(MarkingGraphNode {
            cell,
            cell_type,
            external_bytes,
        });
        self
    }

    pub fn edge(mut self, from: CellId, to: CellId, dependency: MarkDependency) -> Self {
        self.graph.edges.push(MarkingGraphEdge {
            from,
            to,
            dependency,
        });
        self
    }

    pub fn build(self) -> Result<MarkingPlanGraph, MarkingPlanGraphError> {
        self.graph.validate()?;
        Ok(self.graph)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkingPlanGraphError {
    InvalidCellId(CellId),
    DuplicateNode(CellId),
    DuplicateEdge {
        from: CellId,
        to: CellId,
        dependency: MarkDependency,
    },
    UnknownEdgeEndpoint(CellId),
    UnknownRoot(CellId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkingPlanStep {
    pub cell: CellId,
    pub reason: MarkReason,
    pub dependency: MarkDependency,
    pub referrer: Option<CellId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkingPlan {
    pub steps: Vec<MarkingPlanStep>,
    pub marked_cells: Vec<CellId>,
    pub weak_candidates: Vec<CellId>,
    pub external_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marking_plan_walks_strong_edges_once_and_records_weak_candidates() {
        let graph = MarkingPlanGraphBuilder::new()
            .node(CellId(1), CellType::Object, 8)
            .node(CellId(2), CellType::String, 13)
            .node(CellId(3), CellType::Object, 21)
            .edge(CellId(1), CellId(2), MarkDependency::Strong)
            .edge(CellId(1), CellId(3), MarkDependency::Weak)
            .edge(CellId(2), CellId(1), MarkDependency::Strong)
            .build()
            .expect("valid descriptor graph");

        let plan = graph
            .plan_from_roots(&[CellId(1)])
            .expect("valid root plan");

        assert_eq!(plan.marked_cells, vec![CellId(1), CellId(2)]);
        assert_eq!(plan.weak_candidates, vec![CellId(3)]);
        assert_eq!(plan.external_bytes, 21);
    }

    #[test]
    fn marking_graph_rejects_unknown_edge_endpoint() {
        let graph = MarkingPlanGraphBuilder::new()
            .node(CellId(1), CellType::Object, 0)
            .edge(CellId(1), CellId(2), MarkDependency::Strong)
            .build();

        assert_eq!(
            graph,
            Err(MarkingPlanGraphError::UnknownEdgeEndpoint(CellId(2)))
        );
    }

    #[test]
    fn marking_plan_rejects_unknown_root() {
        let graph = MarkingPlanGraphBuilder::new()
            .node(CellId(1), CellType::Object, 0)
            .build()
            .expect("valid descriptor graph");

        assert_eq!(
            graph.plan_from_roots(&[CellId(2)]),
            Err(MarkingPlanGraphError::UnknownRoot(CellId(2)))
        );
    }
}
