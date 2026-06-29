//! Pending exception and termination state.

use crate::bytecode::BytecodeIndex;
use crate::gc::{
    Heap, HeapId, RootId, RootKind, RootRecord, RootSetSemanticError, TargetedRootRecord,
    TargetedRootSet,
};
use crate::runtime::CallFrameId;
use crate::value::{EncodedJsValue, JsValue};

/// Opaque identity for exception scopes used to enforce checking discipline.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct ExceptionScopeId(pub u64);

/// Pending JavaScript exception value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingException {
    pub value: JsValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationReason {
    Watchdog,
    HostRequest,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExceptionCheckState {
    #[default]
    Clean,
    PendingCheck,
    Suspended,
}

/// VM-wide exception and termination state.
#[derive(Clone, Debug, Default)]
pub struct ExceptionState {
    pending: Option<PendingException>,
    last: Option<PendingException>,
    termination: Option<TerminationReason>,
    check_state: ExceptionCheckState,
    scope_depth: usize,
    unwind: ExceptionUnwindState,
    // D3 (jit-runtime-bridge.md): the JIT's mirror of C++ `VM::m_exception` (a
    // single fixed-offset `EncodedJSValue` word, VM.h). 0 == VALUE_EMPTY == no
    // pending exception (JSCJSValue.h:487 `ValueEmpty`). The interpreter keeps its
    // own `Result`/`pending: Option<PendingException>` path UNTOUCHED (a
    // pre-existing divergence from C++'s single m_exception word; converge later);
    // this word is the JIT fast path's bakeable mirror only. After a slow-path
    // call, emitted code does `branchTestPtr(NonZero, AbsoluteAddress(addr-of
    // jit_pending))` -> the exception edge. Stable address via
    // `jit_pending_address()` so the JIT can bake it as an `AbsoluteAddress`.
    jit_pending: EncodedJsValue,
}

impl ExceptionState {
    pub fn pending(&self) -> Option<PendingException> {
        self.pending
    }

    /// D3 (jit-runtime-bridge.md): read the JIT m_exception mirror word. 0 ==
    /// VALUE_EMPTY == none. Faithful analog of reading `VM::m_exception`.
    pub fn jit_pending(&self) -> EncodedJsValue {
        self.jit_pending
    }

    /// D3: stamp the JIT m_exception mirror word (set on the slow-path shim's
    /// throw edge). Faithful analog of `VM::setException` writing `m_exception`.
    pub fn set_jit_pending(&mut self, value: EncodedJsValue) {
        self.jit_pending = value;
    }

    /// D3: the stable address of the JIT m_exception mirror word, which the
    /// baseline JIT bakes as an `AbsoluteAddress` for its post-call
    /// `branchTestPtr(NonZero, ...)` exception check (`VM::addressOfException`,
    /// VM.h). A raw `*const` (not a borrow proof) so the emitter can hold it
    /// across code generation; the word lives in the `Vm`-owned `ExceptionState`.
    pub fn jit_pending_address(&self) -> *const EncodedJsValue {
        &self.jit_pending
    }

    pub fn throw(&mut self, value: JsValue) {
        let pending = PendingException { value };
        self.pending = Some(pending);
        self.last = Some(pending);
        self.check_state = ExceptionCheckState::PendingCheck;
    }

    pub fn clear_pending(&mut self) -> Option<PendingException> {
        let pending = self.pending.take();
        if pending.is_some() {
            self.check_state = ExceptionCheckState::Clean;
        }
        pending
    }

    pub fn take_pending_for_handler(&mut self) -> Option<PendingException> {
        let pending = self.clear_pending()?;
        self.unwind.finish();
        Some(pending)
    }

    pub fn last(&self) -> Option<PendingException> {
        self.last
    }

    pub fn request_termination(&mut self, reason: TerminationReason) {
        self.termination = Some(reason);
        self.check_state = ExceptionCheckState::PendingCheck;
    }

    pub fn termination(&self) -> Option<TerminationReason> {
        self.termination
    }

    pub fn check_state(&self) -> ExceptionCheckState {
        self.check_state
    }

    pub fn scope_depth(&self) -> usize {
        self.scope_depth
    }

    pub fn enter_scope(&mut self) -> ExceptionScopeId {
        self.scope_depth = self.scope_depth.saturating_add(1);
        ExceptionScopeId(self.scope_depth as u64)
    }

    pub fn leave_scope(&mut self, _scope: ExceptionScopeId) {
        self.scope_depth = self.scope_depth.saturating_sub(1);
    }

    pub fn unwind_state(&self) -> &ExceptionUnwindState {
        &self.unwind
    }

    pub fn replace_unwind(&mut self, unwind: ExceptionUnwindState) {
        self.unwind = unwind;
    }

    pub fn root_descriptors(&self, heap: HeapId) -> Vec<ExceptionRootDescriptor> {
        let mut roots = Vec::new();
        if let Some(pending) = self.pending {
            roots.push(ExceptionRootDescriptor::new(
                heap,
                ExceptionRootSource::PendingException,
                pending.value,
            ));
        }
        if let Some(last) = self.last {
            roots.push(ExceptionRootDescriptor::new(
                heap,
                ExceptionRootSource::LastException,
                last.value,
            ));
        }
        roots.extend(self.unwind.root_descriptors(heap));
        roots
    }

    pub fn targeted_root_plan(
        &self,
        heap: &Heap,
    ) -> Result<ExceptionTargetedRootPlan, RootSetSemanticError> {
        ExceptionTargetedRootPlan::from_descriptors(heap, self.root_descriptors(heap.id()))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExceptionUnwindState {
    pending: Option<PendingException>,
    origin_frame: Option<CallFrameId>,
    origin_bytecode_index: Option<BytecodeIndex>,
    handler: Option<UnwindHandler>,
    popped_frames: Vec<CallFrameId>,
    phase: UnwindPhase,
}

impl ExceptionUnwindState {
    pub fn begin(
        pending: PendingException,
        origin_frame: Option<CallFrameId>,
        origin_bytecode_index: BytecodeIndex,
    ) -> Self {
        Self {
            pending: Some(pending),
            origin_frame,
            origin_bytecode_index: Some(origin_bytecode_index),
            handler: None,
            popped_frames: Vec::new(),
            phase: UnwindPhase::Searching,
        }
    }

    pub fn pending(&self) -> Option<PendingException> {
        self.pending
    }

    pub fn origin_frame(&self) -> Option<CallFrameId> {
        self.origin_frame
    }

    pub fn origin_bytecode_index(&self) -> Option<BytecodeIndex> {
        self.origin_bytecode_index
    }

    pub fn handler(&self) -> Option<UnwindHandler> {
        self.handler
    }

    pub fn popped_frames(&self) -> &[CallFrameId] {
        &self.popped_frames
    }

    pub fn phase(&self) -> UnwindPhase {
        self.phase
    }

    pub fn install_handler(&mut self, handler: UnwindHandler) {
        self.handler = Some(handler);
        self.phase = UnwindPhase::HandlerFound;
    }

    pub fn record_frame_popped(&mut self, frame: CallFrameId) {
        self.popped_frames.push(frame);
        self.phase = UnwindPhase::PoppingFrames;
    }

    pub fn finish(&mut self) {
        self.pending = None;
        self.phase = UnwindPhase::Complete;
    }

    pub fn root_descriptors(&self, heap: HeapId) -> Vec<ExceptionRootDescriptor> {
        self.pending
            .map(|pending| {
                ExceptionRootDescriptor::new(
                    heap,
                    ExceptionRootSource::UnwindPendingException,
                    pending.value,
                )
            })
            .into_iter()
            .collect()
    }

    pub fn targeted_root_plan(
        &self,
        heap: &Heap,
    ) -> Result<ExceptionTargetedRootPlan, RootSetSemanticError> {
        ExceptionTargetedRootPlan::from_descriptors(heap, self.root_descriptors(heap.id()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExceptionRootSource {
    PendingException,
    LastException,
    UnwindPendingException,
}

impl ExceptionRootSource {
    pub const ALL: [Self; 3] = [
        Self::PendingException,
        Self::LastException,
        Self::UnwindPendingException,
    ];

    const fn ordinal(self) -> u64 {
        match self {
            Self::PendingException => 1,
            Self::LastException => 2,
            Self::UnwindPendingException => 3,
        }
    }

    pub const fn root_record(self, heap: HeapId) -> RootRecord {
        RootRecord {
            id: RootId(3_000_000_u64 + self.ordinal()),
            kind: RootKind::VMRegister,
            heap,
        }
    }
}

pub fn exception_root_records(heap: HeapId) -> [RootRecord; 3] {
    ExceptionRootSource::ALL.map(|source| source.root_record(heap))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExceptionRootDescriptor {
    pub root: RootRecord,
    pub source: ExceptionRootSource,
    pub value: JsValue,
    pub cell_payload: Option<usize>,
}

impl ExceptionRootDescriptor {
    fn new(heap: HeapId, source: ExceptionRootSource, value: JsValue) -> Self {
        Self {
            root: source.root_record(heap),
            source,
            value,
            cell_payload: value.as_cell().map(|cell| cell.pointer_payload_bits()),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExceptionTargetedRootPlan {
    records: Vec<TargetedRootRecord>,
}

impl ExceptionTargetedRootPlan {
    pub fn from_descriptors(
        heap: &Heap,
        descriptors: impl IntoIterator<Item = ExceptionRootDescriptor>,
    ) -> Result<Self, RootSetSemanticError> {
        let mut records = Vec::new();
        for descriptor in descriptors {
            let Some(payload) = descriptor.cell_payload else {
                continue;
            };
            let Some(target) = heap.cell_for_payload(payload) else {
                continue;
            };
            records.push(TargetedRootRecord {
                root: descriptor.root,
                target,
            });
        }

        TargetedRootSet::from_records(heap.id(), records).map(|set| Self {
            records: set.records().to_vec(),
        })
    }

    pub fn records(&self) -> &[TargetedRootRecord] {
        &self.records
    }

    pub fn into_records(self) -> Vec<TargetedRootRecord> {
        self.records
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnwindPhase {
    #[default]
    Idle,
    Searching,
    HandlerFound,
    PoppingFrames,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnwindHandler {
    pub target: BytecodeIndex,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        static_cell_metadata_registry, AllocationMode, CellId, CellType, GcRef,
        HeapAllocationRequest,
    };
    use crate::value::EncodedJsValue;
    use std::pin::Pin;
    use std::ptr::NonNull;

    #[repr(transparent)]
    struct TestStringCell(String);

    fn heap_bound_string_value(
        heap: &mut Heap,
        text: &str,
    ) -> (Pin<Box<TestStringCell>>, JsValue, CellId) {
        let metadata = static_cell_metadata_registry()
            .metadata_for_type(CellType::String)
            .map(|descriptor| descriptor.metadata)
            .expect("string metadata");
        let allocation = heap
            .allocate_record(HeapAllocationRequest {
                heap: heap.id(),
                subspace: "auxiliary",
                metadata,
                byte_size: std::mem::size_of::<String>().max(1),
                mode: AllocationMode::Normal,
                may_trigger_collection: false,
            })
            .expect("string allocation");
        let string = Box::pin(TestStringCell(text.to_owned()));
        let ptr = NonNull::from(string.as_ref().get_ref());
        // SAFETY: The pinned test cell is kept alive by the test while the value is used.
        let value = JsValue::from_cell(unsafe { GcRef::from_non_null(ptr) });
        let payload = value.as_cell().expect("cell value").pointer_payload_bits();
        heap.bind_cell_payload(allocation.cell, payload)
            .expect("payload binding");
        heap.publish_cell(allocation.cell)
            .expect("publish string cell");
        (string, value, allocation.cell)
    }

    fn unbound_string_value(text: &str) -> (Pin<Box<TestStringCell>>, JsValue) {
        let string = Box::pin(TestStringCell(text.to_owned()));
        let ptr = NonNull::from(string.as_ref().get_ref());
        // SAFETY: The pinned test cell is kept alive by the test while the value is used.
        let value = JsValue::from_cell(unsafe { GcRef::from_non_null(ptr) });
        (string, value)
    }

    #[test]
    fn unwind_state_records_origin_handler_and_popped_frames() {
        let pending = PendingException {
            value: JsValue::from_i32(5),
        };
        let mut unwind = ExceptionUnwindState::begin(
            pending,
            Some(CallFrameId(9)),
            BytecodeIndex::from_offset(2),
        );

        assert_eq!(unwind.phase(), UnwindPhase::Searching);
        unwind.record_frame_popped(CallFrameId(9));
        unwind.install_handler(UnwindHandler {
            target: BytecodeIndex::from_offset(7),
        });

        assert_eq!(unwind.pending(), Some(pending));
        assert_eq!(unwind.popped_frames(), &[CallFrameId(9)]);
        assert_eq!(
            unwind.handler(),
            Some(UnwindHandler {
                target: BytecodeIndex::from_offset(7)
            })
        );
        assert_eq!(unwind.phase(), UnwindPhase::HandlerFound);
    }

    #[test]
    fn unwind_roots_keep_pending_exception_visible() {
        let pending = PendingException {
            value: JsValue::from_encoded(EncodedJsValue((0x55 << 8) | 0x20)),
        };
        let unwind = ExceptionUnwindState::begin(
            pending,
            Some(CallFrameId(9)),
            BytecodeIndex::from_offset(2),
        );

        let roots = unwind.root_descriptors(HeapId(4));

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].source, ExceptionRootSource::UnwindPendingException);
        assert_eq!(roots[0].root.heap, HeapId(4));
        assert_eq!(roots[0].cell_payload, Some(0x55));
    }

    #[test]
    fn handler_take_clears_pending_and_completes_unwind_without_forgetting_last() {
        let pending = PendingException {
            value: JsValue::from_encoded(EncodedJsValue((0x56 << 8) | 0x20)),
        };
        let mut state = ExceptionState::default();
        state.throw(pending.value);
        state.replace_unwind(ExceptionUnwindState::begin(
            pending,
            Some(CallFrameId(9)),
            BytecodeIndex::from_offset(2),
        ));

        let taken = state.take_pending_for_handler();

        assert_eq!(taken, Some(pending));
        assert_eq!(state.pending(), None);
        assert_eq!(state.last(), Some(pending));
        assert_eq!(state.unwind_state().pending(), None);
        assert_eq!(state.unwind_state().phase(), UnwindPhase::Complete);
        assert!(!state
            .root_descriptors(HeapId(4))
            .iter()
            .any(|root| root.source == ExceptionRootSource::UnwindPendingException));
    }

    #[test]
    fn exception_state_targeted_plan_maps_heap_known_exception_values() {
        let mut heap = Heap::new();
        let (_string, value, cell) = heap_bound_string_value(&mut heap, "pending");
        let mut state = ExceptionState::default();
        state.throw(value);

        let plan = state.targeted_root_plan(&heap).unwrap();

        assert_eq!(
            plan.records(),
            &[
                TargetedRootRecord {
                    root: ExceptionRootSource::PendingException.root_record(heap.id()),
                    target: cell,
                },
                TargetedRootRecord {
                    root: ExceptionRootSource::LastException.root_record(heap.id()),
                    target: cell,
                },
            ]
        );
    }

    #[test]
    fn unwind_state_targeted_plan_maps_heap_known_pending_exception() {
        let mut heap = Heap::new();
        let (_string, value, cell) = heap_bound_string_value(&mut heap, "unwind");
        let pending = PendingException { value };
        let unwind = ExceptionUnwindState::begin(
            pending,
            Some(CallFrameId(3)),
            BytecodeIndex::from_offset(5),
        );

        let plan = unwind.targeted_root_plan(&heap).unwrap();

        assert_eq!(
            plan.records(),
            &[TargetedRootRecord {
                root: ExceptionRootSource::UnwindPendingException.root_record(heap.id()),
                target: cell,
            }]
        );
    }

    #[test]
    fn exception_targeted_plan_skips_immediates_and_unknown_payloads() {
        let heap = Heap::new();
        let (_unknown, unknown_value) = unbound_string_value("unknown");
        let mut state = ExceptionState::default();
        state.throw(JsValue::from_i32(7));
        state.replace_unwind(ExceptionUnwindState::begin(
            PendingException {
                value: unknown_value,
            },
            Some(CallFrameId(3)),
            BytecodeIndex::from_offset(5),
        ));

        let plan = state.targeted_root_plan(&heap).unwrap();

        assert!(plan.records().is_empty());
    }
}
