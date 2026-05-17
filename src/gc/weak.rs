//! Weak block, weak set, and finalization descriptors.
//!
//! Weak clearing, weak owner callbacks, and finalizers are collector decisions.
//! This module names the states and records that future GC code will consume.

use crate::gc::{GcRef, HeapId, JsCell, WeakId, WeakProcessingPhase};

/// Opaque weak-set identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakSetId(pub u64);

/// Opaque weak-block identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakBlockId(pub u64);

/// Opaque weak owner identity. The owner supplies clearing/finalize policy.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakHandleOwnerId(pub u64);

/// Weak slot state during weak processing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakSlotState {
    #[default]
    Live,
    Dead,
    Deallocated,
    ClearPending,
}

/// Weak edge category used by validation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WeakEdgeKind {
    #[default]
    Ordinary,
    EphemeronKey,
    EphemeronValue,
    FinalizerCell,
}

/// One weak slot descriptor.
#[derive(Clone, Copy, Debug)]
pub struct WeakSlotRecord {
    pub id: WeakId,
    pub set: WeakSetId,
    pub owner: Option<WeakHandleOwnerId>,
    pub target: Option<GcRef<JsCell>>,
    pub kind: WeakEdgeKind,
    pub state: WeakSlotState,
}

/// Result staged by sweeping a weak block.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WeakSweepResult {
    pub block_is_free: bool,
    pub block_is_logically_empty: bool,
    pub free_slot_count: usize,
}

/// Descriptor for a compact block of weak slots.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeakBlockDescriptor {
    pub id: WeakBlockId,
    pub set: WeakSetId,
    pub slot_capacity: usize,
    pub live_slot_count: usize,
    pub sweep_result: Option<WeakSweepResult>,
}

/// Descriptor for a weak set owned by a block or precise allocation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeakSetDescriptor {
    pub id: WeakSetId,
    pub heap: HeapId,
    pub blocks: Vec<WeakBlockId>,
    pub active_phase: Option<WeakProcessingPhase>,
}

/// Finalizer family.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FinalizerKind {
    #[default]
    CellDestructor,
    CCallback,
    HostCallback,
    Unconditional,
}

/// Opaque callback registered with the heap.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapFinalizerCallbackId(pub u64);

/// Cell-level finalizer record.
#[derive(Clone, Copy, Debug)]
pub struct FinalizerRecord {
    pub callback: HeapFinalizerCallbackId,
    pub target: GcRef<JsCell>,
    pub owner: Option<WeakHandleOwnerId>,
    pub kind: FinalizerKind,
}

/// Heap shutdown and end-phase callback descriptor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HeapFinalizerCallback {
    pub id: HeapFinalizerCallbackId,
    pub kind: FinalizerKind,
    pub user_data_tag: usize,
}
