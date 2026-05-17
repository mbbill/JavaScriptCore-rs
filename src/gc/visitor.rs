//! Marking worklists, slot visitors, and conservative root scan descriptors.

use crate::gc::{ConservativeRootSpan, GcRef, HeapEpoch, HeapId, JsCell, MarkReason, RootRecord};

/// Opaque marking worklist identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct MarkWorklistId(pub u64);

/// Worklist role in the collector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkWorklistKind {
    #[default]
    Mutator,
    Collector,
    Shared,
    Auxiliary,
    Verifier,
}

/// Dependency attached to a mark operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkDependency {
    #[default]
    Strong,
    Hidden,
    Weak,
    Conservative,
    Auxiliary,
}

/// One item that would be appended to a mark stack.
#[derive(Clone, Copy, Debug)]
pub struct MarkWorkItem {
    pub cell: GcRef<JsCell>,
    pub reason: MarkReason,
    pub dependency: MarkDependency,
}

/// Observable mark-stack counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkWorklistStats {
    pub pending_cells: usize,
    pub pending_bytes: usize,
    pub donated_cells: usize,
}

/// Descriptor for one mark worklist.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkWorklistDescriptor {
    pub id: MarkWorklistId,
    pub kind: MarkWorklistKind,
    pub stats: MarkWorklistStats,
}

/// Drain mode used by a slot visitor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DrainMode {
    #[default]
    Main,
    Helper,
    PassiveParallel,
    Incremental,
}

/// Outcome of a drain increment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DrainResult {
    #[default]
    Done,
    TimedOut,
    NeedsMoreWork,
}

/// Conservative scan source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConservativeRootSource {
    #[default]
    MachineStack,
    VMRegisters,
    JitStubRoutines,
    Host,
}

/// Conservative roots found in raw stack or register spans.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConservativeRoots {
    spans: Vec<ConservativeRootSpan>,
    candidate_addresses: Vec<usize>,
}

impl ConservativeRoots {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_span(&mut self, span: ConservativeRootSpan) {
        self.spans.push(span);
    }

    pub fn spans(&self) -> &[ConservativeRootSpan] {
        &self.spans
    }

    pub fn candidate_addresses(&self) -> &[usize] {
        &self.candidate_addresses
    }
}

/// Slot visitor state visible to tracing and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotVisitorDescriptor {
    pub heap: HeapId,
    pub code_name: &'static str,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub bytes_visited: usize,
    pub mutator_is_stopped: bool,
    pub is_parallel: bool,
}

impl SlotVisitorDescriptor {
    pub fn new(heap: HeapId, code_name: &'static str, marking_epoch: HeapEpoch) -> Self {
        Self {
            heap,
            code_name,
            marking_epoch,
            worklist: MarkWorklistId::default(),
            bytes_visited: 0,
            mutator_is_stopped: false,
            is_parallel: false,
        }
    }

    pub fn append_cell(&self, cell: GcRef<JsCell>, reason: MarkReason) -> MarkWorkItem {
        MarkWorkItem {
            cell,
            reason,
            dependency: MarkDependency::Strong,
        }
    }

    pub fn append_hidden_cell(&self, cell: GcRef<JsCell>, reason: MarkReason) -> MarkWorkItem {
        MarkWorkItem {
            cell,
            reason,
            dependency: MarkDependency::Hidden,
        }
    }

    pub fn append_weak_cell(&self, cell: GcRef<JsCell>) -> MarkWorkItem {
        MarkWorkItem {
            cell,
            reason: MarkReason::WeakValidation,
            dependency: MarkDependency::Weak,
        }
    }
}

/// Root visitation plan for a collection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RootMarkingPlan {
    pub precise_roots: Vec<RootRecord>,
    pub conservative_spans: Vec<ConservativeRootSpan>,
    pub source: ConservativeRootSource,
}
