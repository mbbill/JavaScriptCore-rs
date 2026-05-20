//! Heap snapshot, statistics, and profiling descriptors.

use crate::gc::{CellId, GcRef, HeapCellKind, HeapEpoch, HeapId, JsCell};

/// Opaque heap snapshot identity.
///
/// Snapshot IDs name diagnostic records. They do not identify heaps or cells.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapSnapshotId(pub u64);

/// Stable node identifier used by heap snapshots.
///
/// Node IDs are local to a snapshot graph and must not be reused as `CellId`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapSnapshotNodeId(pub u32);

/// Heap snapshot mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapSnapshotKind {
    #[default]
    Inspector,
    GcDebugging,
}

/// Node in a heap snapshot.
///
/// The node borrows a cell reference for diagnostics. It owns retained-size
/// metadata only; the heap keeps storage and liveness authority.
#[derive(Clone, Copy, Debug)]
pub struct HeapSnapshotNode {
    pub id: HeapSnapshotNodeId,
    pub cell: GcRef<JsCell>,
    pub class_name: &'static str,
    pub retained_size: usize,
}

/// Edge type used by heap snapshot serialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapSnapshotEdgeType {
    #[default]
    Internal,
    Property,
    Index,
    Variable,
    Weak,
}

/// Edge name for snapshot output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapSnapshotEdgeName {
    None,
    String(&'static str),
    Index(u32),
}

/// Edge in a heap snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapSnapshotEdge {
    pub from: HeapSnapshotNodeId,
    pub to: HeapSnapshotNodeId,
    pub edge_type: HeapSnapshotEdgeType,
    pub name: HeapSnapshotEdgeName,
}

/// Snapshot descriptor. It owns metadata only, not heap cells.
///
/// Snapshot edges and nodes are diagnostic observations. They must not be used
/// to keep cells live or to mutate heap topology.
#[derive(Clone, Debug, Default)]
pub struct HeapSnapshot {
    pub id: HeapSnapshotId,
    pub previous: Option<HeapSnapshotId>,
    pub kind: HeapSnapshotKind,
    pub nodes: Vec<HeapSnapshotNode>,
    pub edges: Vec<HeapSnapshotEdge>,
    pub finalized: bool,
    pub overflowed: bool,
}

/// ID-only heap snapshot node recorded by the heap integration layer.
///
/// This is the form used before object payload tracing can borrow concrete
/// cells. The `cell` is canonical heap-cell identity, while `id` remains local
/// to one snapshot graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapSnapshotCellRecord {
    pub id: HeapSnapshotNodeId,
    pub cell: CellId,
    pub class_name: &'static str,
    pub retained_size: usize,
}

/// Heap-owned snapshot graph record.
///
/// Snapshot records are diagnostics. They do not root cells and cannot be used
/// to mutate the heap graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeapSnapshotRecord {
    pub id: HeapSnapshotId,
    pub previous: Option<HeapSnapshotId>,
    pub kind: HeapSnapshotKind,
    pub epoch: HeapEpoch,
    pub nodes: Vec<HeapSnapshotCellRecord>,
    pub edges: Vec<HeapSnapshotEdge>,
    pub finalized: bool,
    pub overflowed: bool,
}

impl HeapSnapshotRecord {
    pub fn new(
        id: HeapSnapshotId,
        previous: Option<HeapSnapshotId>,
        kind: HeapSnapshotKind,
        epoch: HeapEpoch,
    ) -> Self {
        Self {
            id,
            previous,
            kind,
            epoch,
            nodes: Vec::new(),
            edges: Vec::new(),
            finalized: false,
            overflowed: false,
        }
    }

    pub fn validate(&self) -> Result<(), HeapSnapshotValidationError> {
        if self.id == HeapSnapshotId::default() {
            return Err(HeapSnapshotValidationError::InvalidSnapshotId(self.id));
        }
        for (index, node) in self.nodes.iter().enumerate() {
            if node.id == HeapSnapshotNodeId::default() {
                return Err(HeapSnapshotValidationError::InvalidNodeId(node.id));
            }
            if node.cell == CellId::default() {
                return Err(HeapSnapshotValidationError::InvalidCellId(node.cell));
            }
            if self.nodes[..index]
                .iter()
                .any(|previous| previous.id == node.id)
            {
                return Err(HeapSnapshotValidationError::DuplicateNode(node.id));
            }
        }

        for edge in &self.edges {
            if !self.nodes.iter().any(|node| node.id == edge.from) {
                return Err(HeapSnapshotValidationError::UnknownEdgeEndpoint(edge.from));
            }
            if !self.nodes.iter().any(|node| node.id == edge.to) {
                return Err(HeapSnapshotValidationError::UnknownEdgeEndpoint(edge.to));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapSnapshotValidationError {
    InvalidSnapshotId(HeapSnapshotId),
    InvalidNodeId(HeapSnapshotNodeId),
    InvalidCellId(CellId),
    DuplicateNode(HeapSnapshotNodeId),
    UnknownEdgeEndpoint(HeapSnapshotNodeId),
}

/// Builder state for a future heap snapshot collection.
///
/// The builder owns graph-construction counters, not heap traversal authority.
/// Traversal must be supplied by an active collector or diagnostic visitor.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeapSnapshotBuilder {
    pub snapshot: HeapSnapshotId,
    pub kind: HeapSnapshotKind,
    pub next_node: HeapSnapshotNodeId,
    pub include_weak_edges: bool,
}

/// Space-level counters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeapSpaceStatistics {
    pub name: &'static str,
    pub cell_kind: HeapCellKind,
    pub object_count: usize,
    pub live_bytes: usize,
    pub capacity_bytes: usize,
    pub free_bytes: usize,
}

/// Whole-heap counters consumed by heuristics and diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeapStatistics {
    pub heap: HeapId,
    pub epoch: HeapEpoch,
    pub object_count: usize,
    pub protected_object_count: usize,
    pub extra_memory_size: usize,
    pub external_memory_size: usize,
    pub total_gc_time_micros: u64,
    pub spaces: Vec<HeapSpaceStatistics>,
}
