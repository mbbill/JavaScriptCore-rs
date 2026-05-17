//! Heap snapshot, statistics, and profiling descriptors.

use crate::gc::{GcRef, HeapCellKind, HeapEpoch, HeapId, JsCell};

/// Opaque heap snapshot identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapSnapshotId(pub u64);

/// Stable node identifier used by heap snapshots.
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

/// Builder state for a future heap snapshot collection.
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
