//! VM entry and top-frame bookkeeping.
//!
//! Call frames are not owned by `Vm`; this module records frame addresses that
//! interpreter, generated code, debugger, and GC integration may need to see.

use core::marker::PhantomData;

use crate::gc::{HeapId, RootId, RootKind, RootRecord};
use crate::jit::{
    BaselineArityCheckNativeEntry, BaselineArityCheckUnavailableReason,
    BaselineNativeEntryDescriptor, BaselineNativeEntryToken, JitCodeId, JitType, MachineCodeHandle,
    MachineCodeRange, TierFallbackReason,
};
use crate::runtime::{
    ArityCheckMode, CallFrameId, CodeBlockId, CodeSpecializationKind, EntryFrameId, GlobalObjectId,
    NativeCodeId, ObjectId, ProtoCallFrame, RuntimeValue,
};

use super::call_frame_storage::VmPublishedTopCallFrame;
use super::entry_frame_storage::VmPublishedEntryFrame;
use super::tiering::{
    BaselineEntryGateOutcome, BaselineEntryGateRecord, BaselineNativeEntryExecutionPolicy,
    BaselineNativeEntryReadinessOutcome, BaselineNativeEntryReadinessRecord,
};

/// Opaque stack/frame address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct FrameAddress(pub usize);

/// VM entry reason. It controls whether reentry and user-observable work are allowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Script,
    HostCall,
    Microtask,
    Debugger,
    VmInquiry,
}

/// Active entry-frame and top-frame bookkeeping.
///
/// `enter_rooted` remains the legacy abstract interpreter path: current Rust
/// interpreter entry has no JSC `EntryFrame*` storage, so it may seed the
/// entry-frame slot from the same abstract address as the top call frame. The
/// dormant `enter_storage_backed` path below mirrors JSC's distinct
/// `VM::topCallFrame` / `VM::topEntryFrame` pair for future native entry.
#[derive(Clone, Debug, Default)]
pub struct VmEntryState {
    entry_depth: usize,
    entry_frame: Option<FrameAddress>,
    top_frame: Option<FrameAddress>,
    kind: Option<EntryKind>,
    disallow_user_observable_work: bool,
    records: Vec<VmEntryRecord>,
    exits: Vec<VmExitRecord>,
    launch_descriptors: Vec<VmEntryLaunchDescriptor>,
    native_call_frame_publications: Vec<VmNativeCallFramePublicationRecord>,
    native_call_frame_publication_exits: Vec<VmNativeCallFramePublicationExitRecord>,
    storage_backed_entries: Vec<VmStorageBackedEntryRecord>,
    storage_backed_entry_exits: Vec<VmStorageBackedEntryExitRecord>,
    next_ordinal: u64,
}

impl VmEntryState {
    pub fn entry_depth(&self) -> usize {
        self.entry_depth
    }

    pub fn top_frame(&self) -> Option<FrameAddress> {
        self.top_frame
    }

    pub fn entry_frame(&self) -> Option<FrameAddress> {
        self.entry_frame
    }

    pub fn kind(&self) -> Option<EntryKind> {
        self.kind
    }

    pub fn disallows_user_observable_work(&self) -> bool {
        self.disallow_user_observable_work
    }

    pub fn records(&self) -> &[VmEntryRecord] {
        &self.records
    }

    pub fn exits(&self) -> &[VmExitRecord] {
        &self.exits
    }

    pub fn launch_descriptors(&self) -> &[VmEntryLaunchDescriptor] {
        &self.launch_descriptors
    }

    pub fn native_call_frame_publications(&self) -> &[VmNativeCallFramePublicationRecord] {
        &self.native_call_frame_publications
    }

    pub fn native_call_frame_publication_exits(&self) -> &[VmNativeCallFramePublicationExitRecord] {
        &self.native_call_frame_publication_exits
    }

    #[allow(dead_code)]
    pub(crate) fn storage_backed_entries(&self) -> &[VmStorageBackedEntryRecord] {
        &self.storage_backed_entries
    }

    #[allow(dead_code)]
    pub(crate) fn storage_backed_entry_exits(&self) -> &[VmStorageBackedEntryExitRecord] {
        &self.storage_backed_entry_exits
    }

    pub(crate) fn record_launch_descriptor(&mut self, descriptor: VmEntryLaunchDescriptor) {
        self.launch_descriptors.push(descriptor);
    }

    fn publish_native_call_frame<'storage>(
        &mut self,
        request: VmNativeCallFramePublicationRequest<'storage>,
    ) -> Result<VmNativeCallFramePublicationGuard<'_, 'storage>, VmNativeCallFramePublicationError>
    {
        if self.entry_depth == 0 {
            return Err(VmNativeCallFramePublicationError::NotInsideVmEntry);
        }
        let current_entry_frame = self
            .entry_frame
            .ok_or(VmNativeCallFramePublicationError::CurrentEntryFrameMissing)?;
        let previous_top_frame = self
            .top_frame
            .ok_or(VmNativeCallFramePublicationError::CurrentTopFrameMissing)?;
        let active_entry_frame = request
            .scope
            .active_entry_frame
            .ok_or(VmNativeCallFramePublicationError::LaunchActiveEntryMissing)?;
        let active_top_call_frame = request
            .scope
            .active_top_call_frame
            .ok_or(VmNativeCallFramePublicationError::LaunchActiveTopFrameMissing)?;
        if request.call_frame.entry_frame != Some(active_entry_frame) {
            return Err(VmNativeCallFramePublicationError::EntryFrameMismatch {
                expected: active_entry_frame,
                actual: request.call_frame.entry_frame,
            });
        }
        if request.call_frame.frame != active_top_call_frame {
            return Err(VmNativeCallFramePublicationError::TopFrameMismatch {
                expected: active_top_call_frame,
                actual: request.call_frame.frame,
            });
        }
        if request.published_top_frame.entry_frame() != Some(active_entry_frame) {
            return Err(
                VmNativeCallFramePublicationError::PublishedEntryFrameMismatch {
                    expected: active_entry_frame,
                    actual: request.published_top_frame.entry_frame(),
                },
            );
        }
        if request.published_top_frame.frame() != active_top_call_frame {
            return Err(
                VmNativeCallFramePublicationError::PublishedTopFrameMismatch {
                    expected: active_top_call_frame,
                    actual: request.published_top_frame.frame(),
                },
            );
        }

        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let ordinal = self.next_ordinal;
        let published_top_frame_proof = request.published_top_frame;
        let published_top_frame = request.published_top_frame.address();
        let record = VmNativeCallFramePublicationRecord {
            ordinal,
            entry_depth: self.entry_depth,
            reason: request.reason,
            owner: request.owner,
            code_block: request.code_block,
            current_entry_frame,
            previous_top_frame: Some(previous_top_frame),
            published_top_frame,
            active_entry_frame,
            previous_entry_frame: request.scope.previous_entry_frame,
            saved_top_call_frame: request.scope.saved_top_call_frame,
            active_top_call_frame,
            call_frame: request.call_frame,
        };

        // C++ NativeCallFrameTracer/SlowPathFrameTracer assert that the
        // CallFrame* is below VM::topEntryFrame before writing VM::topCallFrame.
        // Rust's dormant storage skeleton uses boxed records, not a machine
        // stack, so address ordering would be meaningless here. Reachability is
        // instead restricted to a storage-backed VM-entry guard plus a
        // storage-derived CallFrame proof. This is not yet a conservative-root
        // proof or real native-execution publication.
        self.top_frame = Some(published_top_frame);
        self.native_call_frame_publications.push(record);

        Ok(VmNativeCallFramePublicationGuard {
            state: self,
            record,
            previous_top_frame: Some(previous_top_frame),
            published_top_frame: published_top_frame_proof,
            _borrow: PhantomData,
        })
    }

    pub fn enter(&mut self, top_frame: Option<FrameAddress>, kind: EntryKind) -> VmEntryGuard<'_> {
        self.enter_rooted(top_frame, kind, HeapId::default())
    }

    pub fn enter_rooted(
        &mut self,
        top_frame: Option<FrameAddress>,
        kind: EntryKind,
        heap: HeapId,
    ) -> VmEntryGuard<'_> {
        let previous_top_frame = self.top_frame;
        let previous_kind = self.kind;
        let previous_disallow = self.disallow_user_observable_work;
        let previous_entry_frame = self.entry_frame;
        if self.entry_depth == 0 {
            self.entry_frame = top_frame;
        }
        self.entry_depth += 1;
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let ordinal = self.next_ordinal;
        self.top_frame = top_frame;
        self.kind = Some(kind);
        self.disallow_user_observable_work = matches!(kind, EntryKind::VmInquiry);
        let root_scope = VmEntryRootScope {
            root: RootRecord {
                id: RootId(4_000_000_u64.saturating_add(ordinal)),
                kind: RootKind::Host,
                heap,
            },
            ordinal,
            kind,
            heap,
        };
        self.records.push(VmEntryRecord {
            ordinal,
            depth: self.entry_depth,
            entry_frame: self.entry_frame,
            previous_entry_frame,
            previous_top_frame,
            top_frame,
            kind,
            root_scope,
        });
        VmEntryGuard {
            state: self,
            ordinal,
            root_scope,
            previous_top_frame,
            previous_kind,
            previous_disallow,
            _borrow: PhantomData,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn enter_storage_backed<'storage>(
        &mut self,
        top_call_frame: FrameAddress,
        published_entry_frame: VmPublishedEntryFrame<'storage>,
        kind: EntryKind,
        heap: HeapId,
    ) -> Result<VmStorageBackedEntryGuard<'_, 'storage>, VmStorageBackedEntryError> {
        let previous_top_call_frame = self.top_frame;
        let previous_top_entry_frame = self.entry_frame;
        if published_entry_frame.previous_top_call_frame() != previous_top_call_frame {
            return Err(VmStorageBackedEntryError::PreviousTopCallFrameMismatch {
                expected: previous_top_call_frame,
                actual: published_entry_frame.previous_top_call_frame(),
            });
        }
        if published_entry_frame.previous_top_entry_frame() != previous_top_entry_frame {
            return Err(VmStorageBackedEntryError::PreviousTopEntryFrameMismatch {
                expected: previous_top_entry_frame,
                actual: published_entry_frame.previous_top_entry_frame(),
            });
        }

        let previous_kind = self.kind;
        let previous_disallow = self.disallow_user_observable_work;
        self.entry_depth += 1;
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let ordinal = self.next_ordinal;
        let top_entry_frame = published_entry_frame.address();
        self.top_frame = Some(top_call_frame);
        // C++ LLInt publishes `(sp, cfr)` into the adjacent
        // VM::topCallFrame / VM::topEntryFrame pair. Rust keeps the existing
        // abstract entry guard for interpreter execution, but this dormant path
        // requires the entry-frame address to come from VM entry-frame storage.
        self.entry_frame = Some(top_entry_frame);
        self.kind = Some(kind);
        self.disallow_user_observable_work = matches!(kind, EntryKind::VmInquiry);
        let root_scope = VmEntryRootScope {
            root: RootRecord {
                id: RootId(4_000_000_u64.saturating_add(ordinal)),
                kind: RootKind::Host,
                heap,
            },
            ordinal,
            kind,
            heap,
        };
        let record = VmStorageBackedEntryRecord {
            ordinal,
            depth: self.entry_depth,
            previous_top_call_frame,
            previous_top_entry_frame,
            top_call_frame,
            top_entry_frame,
            entry: published_entry_frame.entry(),
            previous_entry_frame: published_entry_frame.previous_entry_frame(),
            saved_top_call_frame: published_entry_frame.saved_top_call_frame(),
            kind,
            root_scope,
        };
        self.storage_backed_entries.push(record);

        Ok(VmStorageBackedEntryGuard {
            state: self,
            record,
            root_scope,
            previous_top_call_frame,
            previous_top_entry_frame,
            previous_kind,
            previous_disallow,
            published_entry_frame,
            _borrow: PhantomData,
        })
    }

    pub fn root_scopes(&self) -> impl Iterator<Item = VmEntryRootScope> + '_ {
        let legacy_scopes =
            self.records
                .iter()
                .map(|record| record.root_scope)
                .filter(move |scope| {
                    !self
                        .exits
                        .iter()
                        .any(|exit| exit.closed_root_scope.ordinal == scope.ordinal)
                });
        let storage_scopes = self
            .storage_backed_entries
            .iter()
            .map(|record| record.root_scope)
            .filter(move |scope| {
                !self
                    .storage_backed_entry_exits
                    .iter()
                    .any(|exit| exit.closed_root_scope.ordinal == scope.ordinal)
            });
        legacy_scopes.chain(storage_scopes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmStorageBackedEntryRecord {
    pub ordinal: u64,
    pub depth: usize,
    pub previous_top_call_frame: Option<FrameAddress>,
    pub previous_top_entry_frame: Option<FrameAddress>,
    pub top_call_frame: FrameAddress,
    pub top_entry_frame: FrameAddress,
    pub entry: EntryFrameId,
    pub previous_entry_frame: Option<EntryFrameId>,
    pub saved_top_call_frame: Option<CallFrameId>,
    pub kind: EntryKind,
    pub root_scope: VmEntryRootScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmStorageBackedEntryExitRecord {
    pub ordinal: u64,
    pub depth_before_exit: usize,
    pub restored_top_call_frame: Option<FrameAddress>,
    pub restored_top_entry_frame: Option<FrameAddress>,
    pub restored_kind: Option<EntryKind>,
    pub closed_entry: VmStorageBackedEntryRecord,
    pub closed_root_scope: VmEntryRootScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum VmStorageBackedEntryError {
    PreviousTopCallFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    PreviousTopEntryFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
}

#[allow(dead_code)]
pub(crate) struct VmStorageBackedEntryGuard<'vm, 'storage> {
    state: &'vm mut VmEntryState,
    record: VmStorageBackedEntryRecord,
    root_scope: VmEntryRootScope,
    previous_top_call_frame: Option<FrameAddress>,
    previous_top_entry_frame: Option<FrameAddress>,
    previous_kind: Option<EntryKind>,
    previous_disallow: bool,
    published_entry_frame: VmPublishedEntryFrame<'storage>,
    _borrow: PhantomData<&'vm mut VmEntryState>,
}

#[allow(dead_code)]
impl<'storage> VmStorageBackedEntryGuard<'_, 'storage> {
    pub(crate) fn record(&self) -> VmStorageBackedEntryRecord {
        self.record
    }

    pub(crate) fn top_call_frame(&self) -> Option<FrameAddress> {
        self.state.top_frame
    }

    pub(crate) fn top_entry_frame(&self) -> Option<FrameAddress> {
        self.state.entry_frame
    }

    pub(crate) fn published_entry_frame(&self) -> VmPublishedEntryFrame<'storage> {
        self.published_entry_frame
    }

    pub(crate) fn publish_native_call_frame<'frame>(
        &mut self,
        request: VmNativeCallFramePublicationRequest<'frame>,
    ) -> Result<VmNativeCallFramePublicationGuard<'_, 'frame>, VmNativeCallFramePublicationError>
    {
        self.state.publish_native_call_frame(request)
    }

    pub(crate) fn enter_storage_backed<'nested>(
        &mut self,
        top_call_frame: FrameAddress,
        published_entry_frame: VmPublishedEntryFrame<'nested>,
        kind: EntryKind,
        heap: HeapId,
    ) -> Result<VmStorageBackedEntryGuard<'_, 'nested>, VmStorageBackedEntryError> {
        self.state
            .enter_storage_backed(top_call_frame, published_entry_frame, kind, heap)
    }
}

impl Drop for VmStorageBackedEntryGuard<'_, '_> {
    fn drop(&mut self) {
        let depth_before_exit = self.state.entry_depth;
        self.state.entry_depth = self.state.entry_depth.saturating_sub(1);
        self.state.top_frame = self.previous_top_call_frame;
        self.state.entry_frame = self.previous_top_entry_frame;
        self.state.kind = self.previous_kind;
        self.state.disallow_user_observable_work = self.previous_disallow;
        self.state
            .storage_backed_entry_exits
            .push(VmStorageBackedEntryExitRecord {
                ordinal: self.record.ordinal,
                depth_before_exit,
                restored_top_call_frame: self.state.top_frame,
                restored_top_entry_frame: self.state.entry_frame,
                restored_kind: self.state.kind,
                closed_entry: self.record,
                closed_root_scope: self.root_scope,
            });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) struct VmNativeCallFramePublicationRequest<'frame> {
    pub(crate) reason: VmNativeCallFramePublicationReason,
    pub(crate) owner: CodeBlockId,
    pub(crate) code_block: CodeBlockId,
    pub(crate) scope: VmEntryLaunchScope,
    pub(crate) call_frame: VmEntryCallFrameMetadata,
    pub(crate) published_top_frame: VmPublishedTopCallFrame<'frame>,
}

#[allow(dead_code)]
impl<'frame> VmNativeCallFramePublicationRequest<'frame> {
    pub(crate) fn baseline_native_entry(
        descriptor: &VmEntryLaunchDescriptor,
        published_top_frame: VmPublishedTopCallFrame<'frame>,
    ) -> Self {
        Self {
            reason: VmNativeCallFramePublicationReason::BaselineNativeEntry,
            owner: descriptor.owner,
            code_block: descriptor.code_block,
            scope: descriptor.scope,
            call_frame: descriptor.call_frame,
            published_top_frame,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmNativeCallFramePublicationReason {
    BaselineNativeEntry,
    BaselineNativeSideExitReentry,
    BaselineNativeSlowPathHandoff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmNativeCallFramePublicationRecord {
    pub ordinal: u64,
    pub entry_depth: usize,
    pub reason: VmNativeCallFramePublicationReason,
    pub owner: CodeBlockId,
    pub code_block: CodeBlockId,
    pub current_entry_frame: FrameAddress,
    pub previous_top_frame: Option<FrameAddress>,
    pub published_top_frame: FrameAddress,
    pub active_entry_frame: EntryFrameId,
    pub previous_entry_frame: Option<EntryFrameId>,
    pub saved_top_call_frame: Option<CallFrameId>,
    pub active_top_call_frame: CallFrameId,
    pub call_frame: VmEntryCallFrameMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmNativeCallFramePublicationExitRecord {
    pub ordinal: u64,
    pub depth_before_exit: usize,
    pub restored_top_frame: Option<FrameAddress>,
    pub closed_publication: VmNativeCallFramePublicationRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum VmNativeCallFramePublicationError {
    NotInsideVmEntry,
    CurrentEntryFrameMissing,
    CurrentTopFrameMissing,
    LaunchActiveEntryMissing,
    LaunchActiveTopFrameMissing,
    EntryFrameMismatch {
        expected: EntryFrameId,
        actual: Option<EntryFrameId>,
    },
    TopFrameMismatch {
        expected: CallFrameId,
        actual: CallFrameId,
    },
    PublishedEntryFrameMismatch {
        expected: EntryFrameId,
        actual: Option<EntryFrameId>,
    },
    PublishedTopFrameMismatch {
        expected: CallFrameId,
        actual: CallFrameId,
    },
}

#[allow(dead_code)]
pub struct VmNativeCallFramePublicationGuard<'vm, 'storage> {
    state: &'vm mut VmEntryState,
    record: VmNativeCallFramePublicationRecord,
    previous_top_frame: Option<FrameAddress>,
    published_top_frame: VmPublishedTopCallFrame<'storage>,
    _borrow: PhantomData<&'vm mut VmEntryState>,
}

#[allow(dead_code)]
impl<'vm, 'storage> VmNativeCallFramePublicationGuard<'vm, 'storage> {
    pub fn record(&self) -> VmNativeCallFramePublicationRecord {
        self.record
    }

    pub fn top_frame(&self) -> Option<FrameAddress> {
        self.state.top_frame
    }

    pub(crate) fn published_top_frame(&self) -> VmPublishedTopCallFrame<'storage> {
        self.published_top_frame
    }

    #[allow(dead_code)]
    pub(crate) fn publish_native_call_frame<'nested>(
        &mut self,
        request: VmNativeCallFramePublicationRequest<'nested>,
    ) -> Result<VmNativeCallFramePublicationGuard<'_, 'nested>, VmNativeCallFramePublicationError>
    {
        self.state.publish_native_call_frame(request)
    }
}

impl Drop for VmNativeCallFramePublicationGuard<'_, '_> {
    fn drop(&mut self) {
        let depth_before_exit = self.state.entry_depth;
        self.state.top_frame = self.previous_top_frame;
        self.state.native_call_frame_publication_exits.push(
            VmNativeCallFramePublicationExitRecord {
                ordinal: self.record.ordinal,
                depth_before_exit,
                restored_top_frame: self.state.top_frame,
                closed_publication: self.record,
            },
        );
    }
}

/// Scoped VM entry guard.
///
/// The raw pointer is an ABI-boundary placeholder for future interpreter/JIT
/// entry code. It is not exposed publicly.
pub struct VmEntryGuard<'vm> {
    state: &'vm mut VmEntryState,
    ordinal: u64,
    root_scope: VmEntryRootScope,
    previous_top_frame: Option<FrameAddress>,
    previous_kind: Option<EntryKind>,
    previous_disallow: bool,
    _borrow: PhantomData<&'vm mut VmEntryState>,
}

impl VmEntryGuard<'_> {
    pub fn top_frame(&self) -> Option<FrameAddress> {
        self.state.top_frame
    }
}

impl Drop for VmEntryGuard<'_> {
    fn drop(&mut self) {
        let exit_depth = self.state.entry_depth;
        self.state.entry_depth = self.state.entry_depth.saturating_sub(1);
        self.state.top_frame = self.previous_top_frame;
        self.state.kind = self.previous_kind;
        self.state.disallow_user_observable_work = self.previous_disallow;
        if self.state.entry_depth == 0 {
            self.state.entry_frame = None;
        }
        self.state.exits.push(VmExitRecord {
            ordinal: self.ordinal,
            depth_before_exit: exit_depth,
            restored_top_frame: self.state.top_frame,
            restored_kind: self.state.kind,
            closed_root_scope: self.root_scope,
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmEntryRootScope {
    pub root: RootRecord,
    pub ordinal: u64,
    pub kind: EntryKind,
    pub heap: HeapId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmEntryRecord {
    pub ordinal: u64,
    pub depth: usize,
    pub entry_frame: Option<FrameAddress>,
    pub previous_entry_frame: Option<FrameAddress>,
    pub previous_top_frame: Option<FrameAddress>,
    pub top_frame: Option<FrameAddress>,
    pub kind: EntryKind,
    pub root_scope: VmEntryRootScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmExitRecord {
    pub ordinal: u64,
    pub depth_before_exit: usize,
    pub restored_top_frame: Option<FrameAddress>,
    pub restored_kind: Option<EntryKind>,
    pub closed_root_scope: VmEntryRootScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VmEntryLaunchDescriptor {
    pub owner: CodeBlockId,
    pub code_block: CodeBlockId,
    pub scope: VmEntryLaunchScope,
    pub call_frame: VmEntryCallFrameMetadata,
    pub baseline_entry_gate: BaselineEntryGateRecord,
    pub readiness_ordinal: u64,
    pub readiness_bytecode_snapshot_present: bool,
    pub native_entry: BaselineNativeEntryDescriptor,
    pub dispatch: VmEntryDispatchSelection,
    pub fallback_route: VmEntryLaunchFallbackRoute,
}

impl VmEntryLaunchDescriptor {
    pub fn baseline_native_entry(
        scope: VmEntryLaunchScope,
        call_frame: VmEntryCallFrameMetadata,
        baseline_entry_gate: BaselineEntryGateRecord,
        readiness: &BaselineNativeEntryReadinessRecord,
    ) -> Result<Self, VmEntryLaunchValidationError> {
        validate_baseline_native_launch_scope(scope, call_frame, &baseline_entry_gate, readiness)?;
        let native_entry = readiness.descriptor.ok_or(
            VmEntryLaunchValidationError::ReadinessDescriptorMissing {
                readiness_ordinal: readiness.ordinal,
            },
        )?;
        let fallback_reason = match readiness.execution_policy {
            BaselineNativeEntryExecutionPolicy::Disabled => TierFallbackReason::NativeEntryDisabled,
            BaselineNativeEntryExecutionPolicy::Enabled => TierFallbackReason::UnsupportedTier,
        };
        Ok(Self {
            owner: baseline_entry_gate.owner,
            code_block: call_frame.code_block.unwrap_or(baseline_entry_gate.owner),
            scope,
            call_frame,
            baseline_entry_gate,
            readiness_ordinal: readiness.ordinal,
            readiness_bytecode_snapshot_present: readiness.bytecode_snapshot.is_some(),
            native_entry,
            dispatch: VmEntryDispatchSelection::BaselineNative(
                BaselineNativeDispatchTokenSelection::select(native_entry, call_frame.arity_mode),
            ),
            fallback_route: VmEntryLaunchFallbackRoute {
                reason: fallback_reason,
                throw_route: VmEntryThrowRoute::InterpreterExceptionCheck,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmEntryLaunchScope {
    pub owner: CodeBlockId,
    pub entry_code_block: Option<CodeBlockId>,
    pub active_entry_frame: Option<EntryFrameId>,
    pub previous_entry_frame: Option<EntryFrameId>,
    pub saved_top_call_frame: Option<CallFrameId>,
    pub active_top_call_frame: Option<CallFrameId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmEntryCallFrameMetadata {
    pub frame: CallFrameId,
    pub entry_frame: Option<EntryFrameId>,
    pub caller_frame: Option<CallFrameId>,
    pub code_block: Option<CodeBlockId>,
    pub callee: Option<ObjectId>,
    pub callee_value: Option<RuntimeValue>,
    pub context: Option<ObjectId>,
    pub global_object: Option<GlobalObjectId>,
    pub entry_value: VmEntryLaunchArgumentValue,
    pub argument_count_including_this: u32,
    pub provided_argument_count: u32,
    pub padded_argument_count: u32,
    pub specialization: CodeSpecializationKind,
    pub arity_mode: ArityCheckMode,
}

impl VmEntryCallFrameMetadata {
    pub fn from_proto_call_frame(request: VmEntryProtoCallFrameMetadata<'_>) -> Self {
        let argument_count_including_this = request
            .proto
            .argument_count_including_this
            .max(request.proto.argument_count.saturating_add(1))
            .max(request.provided_argument_count.saturating_add(1));
        let padded_argument_count = request
            .proto
            .padded_argument_count
            .max(argument_count_including_this);
        let entry_value = request
            .construct_new_target
            .map(VmEntryLaunchArgumentValue::ConstructNewTarget)
            .unwrap_or(VmEntryLaunchArgumentValue::This(request.proto.this_value));
        Self {
            frame: request.frame,
            entry_frame: request.entry_frame,
            caller_frame: request.caller_frame,
            code_block: request.proto.code_block,
            callee: request.proto.callee,
            callee_value: None,
            context: request.proto.context,
            global_object: request.global_object,
            entry_value,
            argument_count_including_this,
            provided_argument_count: request.provided_argument_count,
            padded_argument_count,
            specialization: request.specialization,
            arity_mode: request.arity_mode,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VmEntryProtoCallFrameMetadata<'proto> {
    pub frame: CallFrameId,
    pub entry_frame: Option<EntryFrameId>,
    pub caller_frame: Option<CallFrameId>,
    pub proto: &'proto ProtoCallFrame,
    pub provided_argument_count: u32,
    pub specialization: CodeSpecializationKind,
    pub arity_mode: ArityCheckMode,
    pub global_object: Option<GlobalObjectId>,
    pub construct_new_target: Option<RuntimeValue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmEntryLaunchArgumentValue {
    This(RuntimeValue),
    ConstructNewTarget(RuntimeValue),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmEntryDispatchSelection {
    BaselineNative(BaselineNativeDispatchTokenSelection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineNativeDispatchTokenSelection {
    NormalEntry {
        token: BaselineNativeEntryToken,
    },
    ArityCheckEntry {
        token: BaselineNativeEntryToken,
    },
    ArityCheckUnavailable {
        reason: BaselineArityCheckUnavailableReason,
    },
}

impl BaselineNativeDispatchTokenSelection {
    pub fn select(descriptor: BaselineNativeEntryDescriptor, arity_mode: ArityCheckMode) -> Self {
        match arity_mode {
            ArityCheckMode::AlreadyChecked => Self::NormalEntry {
                token: descriptor.normal_entry,
            },
            ArityCheckMode::MustCheckArity => match descriptor.arity_check_entry {
                BaselineArityCheckNativeEntry::Token(token) => Self::ArityCheckEntry { token },
                BaselineArityCheckNativeEntry::Unavailable(reason) => {
                    Self::ArityCheckUnavailable { reason }
                }
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmEntryLaunchFallbackRoute {
    pub reason: TierFallbackReason,
    pub throw_route: VmEntryThrowRoute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmEntryThrowRoute {
    InterpreterExceptionCheck,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VmEntryLaunchValidationError {
    GateOutcome {
        actual: BaselineEntryGateOutcome,
    },
    GateRequestedTier {
        actual: JitType,
    },
    MissingReadinessOrdinal,
    ReadinessOrdinalMismatch {
        expected: u64,
        actual: u64,
    },
    ReadinessOutcomeNotReady {
        ordinal: u64,
    },
    ReadinessExecutionPolicyMismatch {
        ordinal: u64,
        expected: BaselineNativeEntryExecutionPolicy,
        actual: BaselineNativeEntryExecutionPolicy,
    },
    ReadinessExecutionPolicyNotDisabled {
        ordinal: u64,
        actual: BaselineNativeEntryExecutionPolicy,
    },
    OwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    NativeArtifactMissing,
    ArtifactIdMismatch {
        expected: JitCodeId,
        actual: Option<JitCodeId>,
    },
    NativeCodeMismatch {
        expected: NativeCodeId,
        actual: Option<NativeCodeId>,
    },
    MachineCodeMismatch {
        expected: MachineCodeHandle,
        actual: Option<MachineCodeHandle>,
    },
    MachineRangeMismatch {
        expected: MachineCodeRange,
        actual: Option<MachineCodeRange>,
    },
    ReadinessDescriptorMissing {
        readiness_ordinal: u64,
    },
    DescriptorOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    DescriptorArtifactIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    DescriptorNativeCodeMismatch {
        expected: NativeCodeId,
        actual: NativeCodeId,
    },
    DescriptorMachineCodeMismatch {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    DescriptorMachineRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    DescriptorEntrypointMismatch,
    DescriptorAbiProofMismatch,
    ActiveEntryMissing,
    ActiveTopFrameMissing,
    ActiveTopFrameMismatch {
        expected: CallFrameId,
        actual: CallFrameId,
    },
    EntryCodeBlockMismatch {
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    TopFrameCodeBlockMismatch {
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    TopFrameEntryMismatch {
        expected: EntryFrameId,
        actual: Option<EntryFrameId>,
    },
    ArgumentCountMismatch {
        provided_argument_count: u32,
        argument_count_including_this: u32,
    },
    PaddedArgumentCountTooSmall {
        argument_count_including_this: u32,
        padded_argument_count: u32,
    },
}

fn validate_baseline_native_launch_scope(
    scope: VmEntryLaunchScope,
    call_frame: VmEntryCallFrameMetadata,
    baseline_entry_gate: &BaselineEntryGateRecord,
    readiness: &BaselineNativeEntryReadinessRecord,
) -> Result<(), VmEntryLaunchValidationError> {
    let (expected_readiness_outcome, expected_execution_policy) = match baseline_entry_gate.outcome
    {
        BaselineEntryGateOutcome::NativeEntryReady => (
            BaselineNativeEntryReadinessOutcome::Ready,
            BaselineNativeEntryExecutionPolicy::Enabled,
        ),
        BaselineEntryGateOutcome::NativeEntryReadyButExecutionDisabled => (
            BaselineNativeEntryReadinessOutcome::ReadyButExecutionDisabled,
            BaselineNativeEntryExecutionPolicy::Disabled,
        ),
        actual => return Err(VmEntryLaunchValidationError::GateOutcome { actual }),
    };
    if baseline_entry_gate.requested_tier != JitType::Baseline {
        return Err(VmEntryLaunchValidationError::GateRequestedTier {
            actual: baseline_entry_gate.requested_tier,
        });
    }
    let expected_readiness_ordinal = baseline_entry_gate
        .native_entry_readiness_ordinal
        .ok_or(VmEntryLaunchValidationError::MissingReadinessOrdinal)?;
    if readiness.ordinal != expected_readiness_ordinal {
        return Err(VmEntryLaunchValidationError::ReadinessOrdinalMismatch {
            expected: expected_readiness_ordinal,
            actual: readiness.ordinal,
        });
    }
    if readiness.outcome != expected_readiness_outcome {
        return Err(VmEntryLaunchValidationError::ReadinessOutcomeNotReady {
            ordinal: readiness.ordinal,
        });
    }
    if readiness.execution_policy != expected_execution_policy {
        return Err(
            VmEntryLaunchValidationError::ReadinessExecutionPolicyMismatch {
                ordinal: readiness.ordinal,
                expected: expected_execution_policy,
                actual: readiness.execution_policy,
            },
        );
    }
    validate_owner(scope.owner, baseline_entry_gate.owner)?;
    validate_owner(readiness.owner, baseline_entry_gate.owner)?;

    let artifact = baseline_entry_gate
        .native_artifact
        .ok_or(VmEntryLaunchValidationError::NativeArtifactMissing)?;
    validate_owner(artifact.owner, baseline_entry_gate.owner)?;
    if readiness.artifact_id != Some(artifact.id) {
        return Err(VmEntryLaunchValidationError::ArtifactIdMismatch {
            expected: artifact.id,
            actual: readiness.artifact_id,
        });
    }
    if readiness.native_code != Some(artifact.native_code) {
        return Err(VmEntryLaunchValidationError::NativeCodeMismatch {
            expected: artifact.native_code,
            actual: readiness.native_code,
        });
    }
    if readiness.machine_code != Some(artifact.machine_code) {
        return Err(VmEntryLaunchValidationError::MachineCodeMismatch {
            expected: artifact.machine_code,
            actual: readiness.machine_code,
        });
    }
    if readiness.machine_range != Some(artifact.machine_code.range) {
        return Err(VmEntryLaunchValidationError::MachineRangeMismatch {
            expected: artifact.machine_code.range,
            actual: readiness.machine_range,
        });
    }

    let descriptor =
        readiness
            .descriptor
            .ok_or(VmEntryLaunchValidationError::ReadinessDescriptorMissing {
                readiness_ordinal: readiness.ordinal,
            })?;
    if descriptor.owner != artifact.owner {
        return Err(VmEntryLaunchValidationError::DescriptorOwnerMismatch {
            expected: artifact.owner,
            actual: descriptor.owner,
        });
    }
    if descriptor.artifact_id != artifact.id {
        return Err(VmEntryLaunchValidationError::DescriptorArtifactIdMismatch {
            expected: artifact.id,
            actual: descriptor.artifact_id,
        });
    }
    if descriptor.native_symbol != artifact.native_code {
        return Err(VmEntryLaunchValidationError::DescriptorNativeCodeMismatch {
            expected: artifact.native_code,
            actual: descriptor.native_symbol,
        });
    }
    if descriptor.machine_code != artifact.machine_code {
        return Err(
            VmEntryLaunchValidationError::DescriptorMachineCodeMismatch {
                expected: artifact.machine_code,
                actual: descriptor.machine_code,
            },
        );
    }
    if descriptor.machine_range != artifact.machine_code.range {
        return Err(
            VmEntryLaunchValidationError::DescriptorMachineRangeMismatch {
                expected: artifact.machine_code.range,
                actual: descriptor.machine_range,
            },
        );
    }
    if descriptor.entrypoint != artifact.entrypoint {
        return Err(VmEntryLaunchValidationError::DescriptorEntrypointMismatch);
    }
    if descriptor.baseline_abi_proof != artifact.baseline_abi_proof {
        return Err(VmEntryLaunchValidationError::DescriptorAbiProofMismatch);
    }

    let active_entry = scope
        .active_entry_frame
        .ok_or(VmEntryLaunchValidationError::ActiveEntryMissing)?;
    let active_top_frame = scope
        .active_top_call_frame
        .ok_or(VmEntryLaunchValidationError::ActiveTopFrameMissing)?;
    if active_top_frame != call_frame.frame {
        return Err(VmEntryLaunchValidationError::ActiveTopFrameMismatch {
            expected: active_top_frame,
            actual: call_frame.frame,
        });
    }
    if scope.entry_code_block != Some(baseline_entry_gate.owner) {
        return Err(VmEntryLaunchValidationError::EntryCodeBlockMismatch {
            expected: baseline_entry_gate.owner,
            actual: scope.entry_code_block,
        });
    }
    if call_frame.code_block != Some(baseline_entry_gate.owner) {
        return Err(VmEntryLaunchValidationError::TopFrameCodeBlockMismatch {
            expected: baseline_entry_gate.owner,
            actual: call_frame.code_block,
        });
    }
    if call_frame.entry_frame != Some(active_entry) {
        return Err(VmEntryLaunchValidationError::TopFrameEntryMismatch {
            expected: active_entry,
            actual: call_frame.entry_frame,
        });
    }
    if call_frame.argument_count_including_this
        < call_frame.provided_argument_count.saturating_add(1)
    {
        return Err(VmEntryLaunchValidationError::ArgumentCountMismatch {
            provided_argument_count: call_frame.provided_argument_count,
            argument_count_including_this: call_frame.argument_count_including_this,
        });
    }
    if call_frame.padded_argument_count < call_frame.argument_count_including_this {
        return Err(VmEntryLaunchValidationError::PaddedArgumentCountTooSmall {
            argument_count_including_this: call_frame.argument_count_including_this,
            padded_argument_count: call_frame.padded_argument_count,
        });
    }

    Ok(())
}

fn validate_owner(
    actual: CodeBlockId,
    expected: CodeBlockId,
) -> Result<(), VmEntryLaunchValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(VmEntryLaunchValidationError::OwnerMismatch { expected, actual })
    }
}

#[cfg(test)]
mod tests {
    use super::super::call_frame_storage::JscCallFrameStorage;
    use super::super::entry_frame_storage::{JscEntryFrameRegistration, JscEntryFrameStorage};
    use super::*;
    use crate::bytecode::register::CallFrameSlotLayout;
    use crate::gc::CellId;
    use crate::interpreter::{FrameState, InstalledCallFrame, RegisterWindow};
    use crate::jit::code::BaselineEntryArtifact;
    use crate::jit::{
        BaselineGeneratedCodeBodyCapability, BaselineNativeEntryCallableAuthority,
        BaselineSupportedOpcodeSubset, CodeFinalizationAuthority, CodeLiveness, CodeOrigin,
        CodeOriginKind, CodeOwnership, EntryAbi, Entrypoint, EntrypointKind,
        ExecutableAllocationId, ExecutableAllocationLifecycle, ExecutableMemoryProtection,
        ExecutableMutationAuthority, JitCodeArtifact, MachineCodeOwnership,
    };

    #[test]
    fn legacy_entry_guard_records_abstract_entry_and_exit_restoration() {
        let mut state = VmEntryState::default();
        {
            let _outer = state.enter_rooted(Some(FrameAddress(10)), EntryKind::Script, HeapId(8));
        }

        assert_eq!(state.entry_depth(), 0);
        assert_eq!(state.records().len(), 1);
        assert_eq!(state.exits().len(), 1);
        // Legacy Rust interpreter entry has not yet been moved onto JSC-shaped
        // EntryFrame storage, so outer entry_frame still mirrors top_frame.
        assert_eq!(state.records()[0].entry_frame, Some(FrameAddress(10)));
        assert_eq!(state.records()[0].top_frame, Some(FrameAddress(10)));
        assert_eq!(state.records()[0].root_scope.heap, HeapId(8));
        assert_eq!(state.exits()[0].restored_top_frame, None);
        assert_eq!(state.root_scopes().count(), 0);
    }

    #[test]
    fn storage_backed_entry_records_distinct_top_call_and_entry_frames_and_restores_both() {
        let mut storage = JscEntryFrameStorage::default();
        let published_entry_frame =
            published_entry_frame(&mut storage, EntryFrameId(1), None, None, None, None);
        let top_entry_frame = published_entry_frame.address();
        let top_call_frame = distinct_top_call_frame(top_entry_frame, 1);
        let mut state = VmEntryState::default();
        {
            let guard = state
                .enter_storage_backed(
                    top_call_frame,
                    published_entry_frame,
                    EntryKind::Script,
                    HeapId(9),
                )
                .expect("storage-backed VM entry");

            assert_eq!(guard.top_call_frame(), Some(top_call_frame));
            assert_eq!(guard.top_entry_frame(), Some(top_entry_frame));
            assert_ne!(guard.top_call_frame(), guard.top_entry_frame());
            assert_eq!(guard.published_entry_frame(), published_entry_frame);
            let record = guard.record();
            assert_eq!(record.previous_top_call_frame, None);
            assert_eq!(record.previous_top_entry_frame, None);
            assert_eq!(record.top_call_frame, top_call_frame);
            assert_eq!(record.top_entry_frame, top_entry_frame);
            assert_eq!(record.entry, EntryFrameId(1));
            assert_eq!(record.root_scope.heap, HeapId(9));
        }

        assert_eq!(state.entry_depth(), 0);
        assert_eq!(state.top_frame(), None);
        assert_eq!(state.entry_frame(), None);
        assert_eq!(state.storage_backed_entries().len(), 1);
        assert_eq!(state.storage_backed_entry_exits().len(), 1);
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_call_frame,
            None
        );
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_entry_frame,
            None
        );
        assert_eq!(state.root_scopes().count(), 0);
    }

    #[test]
    fn nested_storage_backed_entries_restore_inner_then_outer_top_pair() {
        let mut storage = JscEntryFrameStorage::default();
        let outer_handle = storage.register_entry_frame(entry_registration(
            EntryFrameId(1),
            None,
            None,
            None,
            None,
        ));
        let outer_top_entry_frame = storage
            .entry_frame_address(outer_handle)
            .expect("outer entry-frame address");
        let outer_top_call_frame = distinct_top_call_frame(outer_top_entry_frame, 2);
        let inner_handle = storage.register_entry_frame(entry_registration(
            EntryFrameId(2),
            Some(EntryFrameId(1)),
            Some(CallFrameId(7)),
            Some(outer_top_call_frame),
            Some(outer_top_entry_frame),
        ));
        let outer_entry_frame = storage
            .published_entry_frame(outer_handle)
            .expect("outer published entry frame");
        let inner_entry_frame = storage
            .published_entry_frame(inner_handle)
            .expect("inner published entry frame");
        let inner_top_entry_frame = inner_entry_frame.address();
        let inner_top_call_frame = distinct_top_call_frame(inner_top_entry_frame, 3);
        let mut state = VmEntryState::default();

        {
            let mut outer = state
                .enter_storage_backed(
                    outer_top_call_frame,
                    outer_entry_frame,
                    EntryKind::Script,
                    HeapId(10),
                )
                .expect("outer storage-backed VM entry");
            assert_eq!(outer.top_call_frame(), Some(outer_top_call_frame));
            assert_eq!(outer.top_entry_frame(), Some(outer_top_entry_frame));
            {
                let inner = outer
                    .enter_storage_backed(
                        inner_top_call_frame,
                        inner_entry_frame,
                        EntryKind::HostCall,
                        HeapId(11),
                    )
                    .expect("inner storage-backed VM entry");
                assert_eq!(inner.top_call_frame(), Some(inner_top_call_frame));
                assert_eq!(inner.top_entry_frame(), Some(inner_top_entry_frame));
                assert_eq!(
                    inner.record().previous_top_call_frame,
                    Some(outer_top_call_frame)
                );
                assert_eq!(
                    inner.record().previous_top_entry_frame,
                    Some(outer_top_entry_frame)
                );
            }
            assert_eq!(outer.top_call_frame(), Some(outer_top_call_frame));
            assert_eq!(outer.top_entry_frame(), Some(outer_top_entry_frame));
        }

        assert_eq!(state.top_frame(), None);
        assert_eq!(state.entry_frame(), None);
        assert_eq!(state.storage_backed_entries().len(), 2);
        assert_eq!(state.storage_backed_entry_exits().len(), 2);
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_call_frame,
            Some(outer_top_call_frame)
        );
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_entry_frame,
            Some(outer_top_entry_frame)
        );
        assert_eq!(
            state.storage_backed_entry_exits()[1].restored_top_call_frame,
            None
        );
        assert_eq!(
            state.storage_backed_entry_exits()[1].restored_top_entry_frame,
            None
        );
    }

    #[test]
    fn storage_backed_entry_rejects_previous_top_call_frame_mismatch_without_mutation() {
        let previous_top_call_frame = FrameAddress(0x1100);
        let previous_top_entry_frame = FrameAddress(0x2200);
        let mut storage = JscEntryFrameStorage::default();
        let published_entry_frame = published_entry_frame(
            &mut storage,
            EntryFrameId(3),
            None,
            None,
            Some(FrameAddress(0x1110)),
            Some(previous_top_entry_frame),
        );
        let mut state = active_storage_entry_state(
            Some(previous_top_entry_frame),
            Some(previous_top_call_frame),
        );

        let error = state
            .enter_storage_backed(
                distinct_top_call_frame(published_entry_frame.address(), 4),
                published_entry_frame,
                EntryKind::Script,
                HeapId(12),
            )
            .err()
            .expect("previous top-call-frame rejection");

        assert_eq!(
            error,
            VmStorageBackedEntryError::PreviousTopCallFrameMismatch {
                expected: Some(previous_top_call_frame),
                actual: Some(FrameAddress(0x1110))
            }
        );
        assert_unmutated_storage_entry_state(
            &state,
            Some(previous_top_entry_frame),
            Some(previous_top_call_frame),
        );
    }

    #[test]
    fn storage_backed_entry_rejects_previous_top_entry_frame_mismatch_without_mutation() {
        let previous_top_call_frame = FrameAddress(0x3300);
        let previous_top_entry_frame = FrameAddress(0x4400);
        let mut storage = JscEntryFrameStorage::default();
        let published_entry_frame = published_entry_frame(
            &mut storage,
            EntryFrameId(4),
            None,
            None,
            Some(previous_top_call_frame),
            Some(FrameAddress(0x4410)),
        );
        let mut state = active_storage_entry_state(
            Some(previous_top_entry_frame),
            Some(previous_top_call_frame),
        );

        let error = state
            .enter_storage_backed(
                distinct_top_call_frame(published_entry_frame.address(), 5),
                published_entry_frame,
                EntryKind::Script,
                HeapId(13),
            )
            .err()
            .expect("previous top-entry-frame rejection");

        assert_eq!(
            error,
            VmStorageBackedEntryError::PreviousTopEntryFrameMismatch {
                expected: Some(previous_top_entry_frame),
                actual: Some(FrameAddress(0x4410))
            }
        );
        assert_unmutated_storage_entry_state(
            &state,
            Some(previous_top_entry_frame),
            Some(previous_top_call_frame),
        );
    }

    #[test]
    fn storage_backed_entry_guard_retains_published_entry_frame_proof() {
        let mut storage = JscEntryFrameStorage::default();
        let handle = storage.register_entry_frame(entry_registration(
            EntryFrameId(5),
            None,
            None,
            None,
            None,
        ));
        {
            let published_entry_frame = storage
                .published_entry_frame(handle)
                .expect("published entry-frame proof");
            let top_call_frame = distinct_top_call_frame(published_entry_frame.address(), 6);
            let mut state = VmEntryState::default();
            {
                let guard = state
                    .enter_storage_backed(
                        top_call_frame,
                        published_entry_frame,
                        EntryKind::Script,
                        HeapId(14),
                    )
                    .expect("storage-backed VM entry");
                assert_eq!(guard.published_entry_frame(), published_entry_frame);
            }
        }

        assert!(storage.retire(handle));
    }

    #[test]
    fn native_call_frame_publication_records_and_restores_top_frame() {
        let owner = CodeBlockId(CellId(14));
        let mut entry_storage = JscEntryFrameStorage::default();
        let published_entry_frame =
            published_entry_frame(&mut entry_storage, EntryFrameId(1), None, None, None, None);
        let top_entry_frame = published_entry_frame.address();
        let entry_top_call_frame = distinct_top_call_frame(top_entry_frame, 14);
        let mut call_storage = JscCallFrameStorage::default();
        let published_top_frame = published_top_call_frame(
            &mut call_storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(1)),
            20,
        );
        let published_address = published_top_frame.address();
        let mut state = VmEntryState::default();
        {
            let mut entry = state
                .enter_storage_backed(
                    entry_top_call_frame,
                    published_entry_frame,
                    EntryKind::Script,
                    HeapId(8),
                )
                .expect("storage-backed VM entry");
            assert_eq!(entry.top_call_frame(), Some(entry_top_call_frame));
            assert_eq!(entry.top_entry_frame(), Some(top_entry_frame));
            {
                let publication = entry
                    .publish_native_call_frame(native_publication_request(
                        owner,
                        published_top_frame,
                    ))
                    .expect("native call-frame publication");
                assert_eq!(publication.top_frame(), Some(published_address));
                // The publication guard retains this storage-derived proof for
                // the same scope that `VmEntryState::top_frame` carries the
                // underlying address. A mutable retire of `storage` here would
                // conflict with that live storage lease.
                assert_eq!(publication.published_top_frame(), published_top_frame);

                let record = publication.record();
                assert_eq!(
                    record.reason,
                    VmNativeCallFramePublicationReason::BaselineNativeEntry
                );
                assert_eq!(record.owner, owner);
                assert_eq!(record.code_block, owner);
                assert_eq!(record.current_entry_frame, top_entry_frame);
                assert_eq!(record.previous_top_frame, Some(entry_top_call_frame));
                assert_eq!(record.published_top_frame, published_address);
                assert_eq!(record.active_entry_frame, EntryFrameId(1));
                assert_eq!(record.active_top_call_frame, CallFrameId(2));
            }
            assert_eq!(entry.top_call_frame(), Some(entry_top_call_frame));
            assert_eq!(entry.top_entry_frame(), Some(top_entry_frame));
        }

        assert_eq!(state.entry_depth(), 0);
        assert_eq!(state.top_frame(), None);
        assert_eq!(state.entry_frame(), None);
        assert_eq!(state.native_call_frame_publications().len(), 1);
        assert_eq!(state.native_call_frame_publication_exits().len(), 1);
        assert_eq!(
            state.native_call_frame_publication_exits()[0].restored_top_frame,
            Some(entry_top_call_frame)
        );
        assert_eq!(state.storage_backed_entry_exits().len(), 1);
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_call_frame,
            None
        );
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_entry_frame,
            None
        );
    }

    #[test]
    fn nested_native_call_frame_publication_restores_inner_then_outer_frame() {
        let owner = CodeBlockId(CellId(15));
        let mut entry_storage = JscEntryFrameStorage::default();
        let published_entry_frame =
            published_entry_frame(&mut entry_storage, EntryFrameId(1), None, None, None, None);
        let top_entry_frame = published_entry_frame.address();
        let entry_top_call_frame = distinct_top_call_frame(top_entry_frame, 15);
        let mut call_storage = JscCallFrameStorage::default();
        let outer_handle = call_storage.register_installed_frame(&installed_frame(
            CallFrameId(2),
            Some(EntryFrameId(1)),
            None,
            owner,
            20,
        ));
        let inner_handle = call_storage.register_installed_frame(&installed_frame(
            CallFrameId(2),
            Some(EntryFrameId(1)),
            None,
            owner,
            30,
        ));
        let outer_top_frame = call_storage
            .published_top_call_frame(outer_handle)
            .expect("outer published top frame");
        let inner_top_frame = call_storage
            .published_top_call_frame(inner_handle)
            .expect("inner published top frame");
        let outer_address = outer_top_frame.address();
        let inner_address = inner_top_frame.address();
        let mut state = VmEntryState::default();
        {
            let mut entry = state
                .enter_storage_backed(
                    entry_top_call_frame,
                    published_entry_frame,
                    EntryKind::Script,
                    HeapId(8),
                )
                .expect("storage-backed VM entry");
            {
                let mut outer = entry
                    .publish_native_call_frame(native_publication_request(owner, outer_top_frame))
                    .expect("outer native call-frame publication");
                assert_eq!(outer.top_frame(), Some(outer_address));
                {
                    let inner = outer
                        .publish_native_call_frame(native_publication_request(
                            owner,
                            inner_top_frame,
                        ))
                        .expect("inner native call-frame publication");
                    assert_eq!(inner.record().previous_top_frame, Some(outer_address));
                    assert_eq!(inner.top_frame(), Some(inner_address));
                }
                assert_eq!(outer.top_frame(), Some(outer_address));
            }
            assert_eq!(entry.top_call_frame(), Some(entry_top_call_frame));
            assert_eq!(entry.top_entry_frame(), Some(top_entry_frame));
        }

        assert_eq!(state.top_frame(), None);
        assert_eq!(state.entry_frame(), None);
        assert_eq!(state.native_call_frame_publications().len(), 2);
        assert_eq!(state.native_call_frame_publication_exits().len(), 2);
        assert_eq!(
            state.native_call_frame_publication_exits()[0].restored_top_frame,
            Some(outer_address)
        );
        assert_eq!(
            state.native_call_frame_publication_exits()[1].restored_top_frame,
            Some(entry_top_call_frame)
        );
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_call_frame,
            None
        );
        assert_eq!(
            state.storage_backed_entry_exits()[0].restored_top_entry_frame,
            None
        );
    }

    #[test]
    fn internal_native_call_frame_publication_rejects_missing_active_vm_entry() {
        let owner = CodeBlockId(CellId(16));
        let mut storage = JscCallFrameStorage::default();
        let published_top_frame = published_top_call_frame(
            &mut storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(1)),
            20,
        );
        let mut state = VmEntryState::default();
        // Same-module tests may exercise the private validator directly. The
        // reachable publication API is scoped under VmStorageBackedEntryGuard.
        let error = state
            .publish_native_call_frame(native_publication_request(owner, published_top_frame))
            .err()
            .expect("publication rejection");

        assert_eq!(error, VmNativeCallFramePublicationError::NotInsideVmEntry);
        assert!(state.native_call_frame_publications().is_empty());
    }

    #[test]
    fn internal_native_call_frame_publication_rejects_missing_current_entry_or_top_frame() {
        let owner = CodeBlockId(CellId(17));
        let mut storage = JscCallFrameStorage::default();
        let published_top_frame = published_top_call_frame(
            &mut storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(1)),
            20,
        );
        let mut missing_entry_frame = active_native_entry_state(None, Some(FrameAddress(10)));
        let entry_error = missing_entry_frame
            .publish_native_call_frame(native_publication_request(owner, published_top_frame))
            .err()
            .expect("entry-frame rejection");
        assert_eq!(
            entry_error,
            VmNativeCallFramePublicationError::CurrentEntryFrameMissing
        );

        let mut missing_top_frame = active_native_entry_state(Some(FrameAddress(10)), None);
        let top_error = missing_top_frame
            .publish_native_call_frame(native_publication_request(owner, published_top_frame))
            .err()
            .expect("top-frame rejection");
        assert_eq!(
            top_error,
            VmNativeCallFramePublicationError::CurrentTopFrameMissing
        );
    }

    #[test]
    fn internal_native_call_frame_publication_rejects_missing_launch_entry_or_top_frame() {
        let owner = CodeBlockId(CellId(18));
        let mut storage = JscCallFrameStorage::default();
        let published_top_frame = published_top_call_frame(
            &mut storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(1)),
            20,
        );
        let mut missing_launch_entry = native_publication_request(owner, published_top_frame);
        missing_launch_entry.scope.active_entry_frame = None;
        let entry_error = active_native_entry_state(Some(FrameAddress(10)), Some(FrameAddress(10)))
            .publish_native_call_frame(missing_launch_entry)
            .err()
            .expect("launch entry-frame rejection");
        assert_eq!(
            entry_error,
            VmNativeCallFramePublicationError::LaunchActiveEntryMissing
        );

        let mut missing_launch_top = native_publication_request(owner, published_top_frame);
        missing_launch_top.scope.active_top_call_frame = None;
        let top_error = active_native_entry_state(Some(FrameAddress(10)), Some(FrameAddress(10)))
            .publish_native_call_frame(missing_launch_top)
            .err()
            .expect("launch top-frame rejection");
        assert_eq!(
            top_error,
            VmNativeCallFramePublicationError::LaunchActiveTopFrameMissing
        );
    }

    #[test]
    fn internal_native_call_frame_publication_rejects_symbolic_entry_or_top_mismatch() {
        let owner = CodeBlockId(CellId(19));
        let mut storage = JscCallFrameStorage::default();
        let published_top_frame = published_top_call_frame(
            &mut storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(1)),
            20,
        );
        let mut entry_mismatch = native_publication_request(owner, published_top_frame);
        entry_mismatch.call_frame.entry_frame = Some(EntryFrameId(9));
        let entry_error = active_native_entry_state(Some(FrameAddress(10)), Some(FrameAddress(10)))
            .publish_native_call_frame(entry_mismatch)
            .err()
            .expect("symbolic entry-frame rejection");
        assert_eq!(
            entry_error,
            VmNativeCallFramePublicationError::EntryFrameMismatch {
                expected: EntryFrameId(1),
                actual: Some(EntryFrameId(9))
            }
        );

        let mut top_mismatch = native_publication_request(owner, published_top_frame);
        top_mismatch.call_frame.frame = CallFrameId(9);
        let top_error = active_native_entry_state(Some(FrameAddress(10)), Some(FrameAddress(10)))
            .publish_native_call_frame(top_mismatch)
            .err()
            .expect("symbolic top-frame rejection");
        assert_eq!(
            top_error,
            VmNativeCallFramePublicationError::TopFrameMismatch {
                expected: CallFrameId(2),
                actual: CallFrameId(9)
            }
        );
    }

    #[test]
    fn internal_native_call_frame_publication_rejects_storage_proof_entry_or_top_mismatch() {
        let owner = CodeBlockId(CellId(20));
        let mut entry_storage = JscCallFrameStorage::default();
        let wrong_entry_top_frame = published_top_call_frame(
            &mut entry_storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(9)),
            20,
        );
        let entry_error = active_native_entry_state(Some(FrameAddress(10)), Some(FrameAddress(10)))
            .publish_native_call_frame(native_publication_request(owner, wrong_entry_top_frame))
            .err()
            .expect("storage entry-frame rejection");
        assert_eq!(
            entry_error,
            VmNativeCallFramePublicationError::PublishedEntryFrameMismatch {
                expected: EntryFrameId(1),
                actual: Some(EntryFrameId(9))
            }
        );

        let mut top_storage = JscCallFrameStorage::default();
        let wrong_top_frame = published_top_call_frame(
            &mut top_storage,
            owner,
            CallFrameId(9),
            Some(EntryFrameId(1)),
            24,
        );
        let top_error = active_native_entry_state(Some(FrameAddress(10)), Some(FrameAddress(10)))
            .publish_native_call_frame(native_publication_request(owner, wrong_top_frame))
            .err()
            .expect("storage top-frame rejection");
        assert_eq!(
            top_error,
            VmNativeCallFramePublicationError::PublishedTopFrameMismatch {
                expected: CallFrameId(2),
                actual: CallFrameId(9)
            }
        );
    }

    #[test]
    fn native_call_frame_publication_exit_closes_matching_record_on_drop() {
        let owner = CodeBlockId(CellId(21));
        let mut entry_storage = JscEntryFrameStorage::default();
        let published_entry_frame =
            published_entry_frame(&mut entry_storage, EntryFrameId(1), None, None, None, None);
        let top_entry_frame = published_entry_frame.address();
        let entry_top_call_frame = distinct_top_call_frame(top_entry_frame, 16);
        let mut call_storage = JscCallFrameStorage::default();
        let published_top_frame = published_top_call_frame(
            &mut call_storage,
            owner,
            CallFrameId(2),
            Some(EntryFrameId(1)),
            20,
        );
        let mut state = VmEntryState::default();
        let publication_ordinal;
        {
            let mut entry = state
                .enter_storage_backed(
                    entry_top_call_frame,
                    published_entry_frame,
                    EntryKind::Script,
                    HeapId(8),
                )
                .expect("storage-backed VM entry");
            {
                let publication = entry
                    .publish_native_call_frame(native_publication_request(
                        owner,
                        published_top_frame,
                    ))
                    .expect("native call-frame publication");
                publication_ordinal = publication.record().ordinal;
            }
        }

        let exits = state.native_call_frame_publication_exits();
        assert_eq!(exits.len(), 1);
        assert_eq!(exits[0].ordinal, publication_ordinal);
        assert_eq!(
            exits[0].closed_publication,
            state.native_call_frame_publications()[0]
        );
    }

    #[test]
    fn function_entry_descriptor_copies_proto_call_frame_metadata_and_pads_args() {
        let owner = CodeBlockId(CellId(10));
        let function = ObjectId(CellId(11));
        let context = ObjectId(CellId(12));
        let global = GlobalObjectId(ObjectId(CellId(13)));
        let proto = ProtoCallFrame {
            code_block: Some(owner),
            callee: Some(function),
            argument_count: 2,
            argument_count_including_this: 3,
            padded_argument_count: 0,
            this_value: RuntimeValue::from_i32(41),
            context: Some(context),
            lexical_realm: None,
        };

        let call_frame =
            VmEntryCallFrameMetadata::from_proto_call_frame(VmEntryProtoCallFrameMetadata {
                frame: CallFrameId(2),
                entry_frame: Some(EntryFrameId(1)),
                caller_frame: Some(CallFrameId(1)),
                proto: &proto,
                provided_argument_count: 4,
                specialization: CodeSpecializationKind::Call,
                arity_mode: ArityCheckMode::AlreadyChecked,
                global_object: Some(global),
                construct_new_target: None,
            });

        assert_eq!(call_frame.code_block, Some(owner));
        assert_eq!(call_frame.callee, Some(function));
        assert_eq!(call_frame.context, Some(context));
        assert_eq!(call_frame.global_object, Some(global));
        assert_eq!(
            call_frame.entry_value,
            VmEntryLaunchArgumentValue::This(RuntimeValue::from_i32(41))
        );
        assert_eq!(call_frame.provided_argument_count, 4);
        assert_eq!(call_frame.argument_count_including_this, 5);
        assert_eq!(call_frame.padded_argument_count, 5);
    }

    #[test]
    fn baseline_native_dispatch_selection_is_symbolic_for_arity_modes() {
        let owner = CodeBlockId(CellId(20));
        let descriptor = baseline_entry_artifact(owner, 20)
            .validate_native_entry_descriptor()
            .expect("native entry descriptor");

        assert_eq!(
            BaselineNativeDispatchTokenSelection::select(
                descriptor,
                ArityCheckMode::AlreadyChecked
            ),
            BaselineNativeDispatchTokenSelection::NormalEntry {
                token: descriptor.normal_entry
            }
        );
        assert_eq!(
            BaselineNativeDispatchTokenSelection::select(
                descriptor,
                ArityCheckMode::MustCheckArity
            ),
            BaselineNativeDispatchTokenSelection::ArityCheckUnavailable {
                reason: BaselineArityCheckUnavailableReason::NotEmitted
            }
        );
    }

    #[test]
    fn mismatched_owner_artifact_or_readiness_ordinal_rejects_launch_descriptor() {
        let owner = CodeBlockId(CellId(30));
        let artifact = baseline_entry_artifact(owner, 30);
        let readiness = readiness_record(owner, artifact, 7);
        let gate = baseline_gate(owner, artifact, readiness.ordinal);
        let scope = launch_scope(owner);
        let call_frame = launch_call_frame(owner);
        assert!(VmEntryLaunchDescriptor::baseline_native_entry(
            scope,
            call_frame,
            gate.clone(),
            &readiness,
        )
        .is_ok());

        let wrong_owner = CodeBlockId(CellId(31));
        let wrong_owner_scope = VmEntryLaunchScope {
            owner: wrong_owner,
            ..scope
        };
        assert_eq!(
            VmEntryLaunchDescriptor::baseline_native_entry(
                wrong_owner_scope,
                call_frame,
                gate.clone(),
                &readiness,
            ),
            Err(VmEntryLaunchValidationError::OwnerMismatch {
                expected: owner,
                actual: wrong_owner
            })
        );

        let mut wrong_artifact_gate = gate.clone();
        wrong_artifact_gate.native_artifact = Some(baseline_entry_artifact(owner, 31));
        assert!(matches!(
            VmEntryLaunchDescriptor::baseline_native_entry(
                scope,
                call_frame,
                wrong_artifact_gate,
                &readiness,
            ),
            Err(VmEntryLaunchValidationError::ArtifactIdMismatch { .. })
        ));

        let wrong_ordinal_gate = baseline_gate(owner, artifact, readiness.ordinal + 1);
        assert_eq!(
            VmEntryLaunchDescriptor::baseline_native_entry(
                scope,
                call_frame,
                wrong_ordinal_gate,
                &readiness,
            ),
            Err(VmEntryLaunchValidationError::ReadinessOrdinalMismatch {
                expected: readiness.ordinal + 1,
                actual: readiness.ordinal
            })
        );
    }

    #[test]
    fn enabled_native_launch_descriptor_keeps_missing_arity_entry_uncallable() {
        let owner = CodeBlockId(CellId(40));
        let artifact = baseline_entry_artifact(owner, 40);
        let readiness = enabled_readiness_record(owner, artifact, 8);
        let gate = enabled_baseline_gate(owner, artifact, readiness.ordinal);
        let scope = launch_scope(owner);
        let mut call_frame = launch_call_frame(owner);
        call_frame.arity_mode = ArityCheckMode::MustCheckArity;

        let descriptor =
            VmEntryLaunchDescriptor::baseline_native_entry(scope, call_frame, gate, &readiness)
                .expect("enabled native launch descriptor");

        assert_eq!(
            descriptor.dispatch,
            VmEntryDispatchSelection::BaselineNative(
                BaselineNativeDispatchTokenSelection::ArityCheckUnavailable {
                    reason: BaselineArityCheckUnavailableReason::NotEmitted
                }
            )
        );
        assert_eq!(
            descriptor.fallback_route.reason,
            TierFallbackReason::UnsupportedTier
        );
    }

    fn launch_scope(owner: CodeBlockId) -> VmEntryLaunchScope {
        VmEntryLaunchScope {
            owner,
            entry_code_block: Some(owner),
            active_entry_frame: Some(EntryFrameId(1)),
            previous_entry_frame: None,
            saved_top_call_frame: None,
            active_top_call_frame: Some(CallFrameId(2)),
        }
    }

    fn launch_call_frame(owner: CodeBlockId) -> VmEntryCallFrameMetadata {
        VmEntryCallFrameMetadata {
            frame: CallFrameId(2),
            entry_frame: Some(EntryFrameId(1)),
            caller_frame: None,
            code_block: Some(owner),
            callee: None,
            callee_value: None,
            context: None,
            global_object: None,
            entry_value: VmEntryLaunchArgumentValue::This(RuntimeValue::undefined()),
            argument_count_including_this: 1,
            provided_argument_count: 0,
            padded_argument_count: 1,
            specialization: CodeSpecializationKind::Call,
            arity_mode: ArityCheckMode::AlreadyChecked,
        }
    }

    fn native_publication_request<'storage>(
        owner: CodeBlockId,
        published_top_frame: VmPublishedTopCallFrame<'storage>,
    ) -> VmNativeCallFramePublicationRequest<'storage> {
        VmNativeCallFramePublicationRequest {
            reason: VmNativeCallFramePublicationReason::BaselineNativeEntry,
            owner,
            code_block: owner,
            scope: launch_scope(owner),
            call_frame: launch_call_frame(owner),
            published_top_frame,
        }
    }

    fn published_entry_frame<'storage>(
        storage: &'storage mut JscEntryFrameStorage,
        entry: EntryFrameId,
        previous_entry_frame: Option<EntryFrameId>,
        saved_top_call_frame: Option<CallFrameId>,
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> VmPublishedEntryFrame<'storage> {
        let handle = storage.register_entry_frame(entry_registration(
            entry,
            previous_entry_frame,
            saved_top_call_frame,
            previous_top_call_frame,
            previous_top_entry_frame,
        ));
        storage
            .published_entry_frame(handle)
            .expect("storage-derived published entry frame")
    }

    fn entry_registration(
        entry: EntryFrameId,
        previous_entry_frame: Option<EntryFrameId>,
        saved_top_call_frame: Option<CallFrameId>,
        previous_top_call_frame: Option<FrameAddress>,
        previous_top_entry_frame: Option<FrameAddress>,
    ) -> JscEntryFrameRegistration {
        JscEntryFrameRegistration {
            entry,
            previous_entry_frame,
            saved_top_call_frame,
            previous_top_call_frame,
            previous_top_entry_frame,
        }
    }

    fn distinct_top_call_frame(top_entry_frame: FrameAddress, salt: usize) -> FrameAddress {
        FrameAddress(top_entry_frame.0 ^ 0x55aa_55aa_usize.wrapping_add(salt))
    }

    fn published_top_call_frame<'storage>(
        storage: &'storage mut JscCallFrameStorage,
        owner: CodeBlockId,
        frame: CallFrameId,
        entry: Option<EntryFrameId>,
        base: usize,
    ) -> VmPublishedTopCallFrame<'storage> {
        let installed_frame = installed_frame(frame, entry, None, owner, base);
        let handle = storage.register_installed_frame(&installed_frame);
        storage
            .published_top_call_frame(handle)
            .expect("storage-derived published top frame")
    }

    fn installed_frame(
        id: CallFrameId,
        entry: Option<EntryFrameId>,
        caller: Option<CallFrameId>,
        owner: CodeBlockId,
        base: usize,
    ) -> InstalledCallFrame {
        InstalledCallFrame {
            id,
            entry,
            caller,
            code_block: Some(owner),
            callee: None,
            callee_value: None,
            lexical_scope: None,
            bytecode_index: None,
            return_address: None,
            return_continuation: None,
            argument_count_including_this: 1,
            register_window: RegisterWindow {
                owner: id,
                base,
                local_count: 4,
                argument_base: base + 4,
                argument_count: 1,
                this_offset: CallFrameSlotLayout::JSC_RUST.this_argument_offset,
            },
            state: FrameState::Executing,
        }
    }

    fn active_storage_entry_state(
        entry_frame: Option<FrameAddress>,
        top_frame: Option<FrameAddress>,
    ) -> VmEntryState {
        VmEntryState {
            entry_depth: 1,
            entry_frame,
            top_frame,
            kind: Some(EntryKind::Script),
            disallow_user_observable_work: false,
            ..VmEntryState::default()
        }
    }

    fn assert_unmutated_storage_entry_state(
        state: &VmEntryState,
        entry_frame: Option<FrameAddress>,
        top_frame: Option<FrameAddress>,
    ) {
        assert_eq!(state.entry_depth(), 1);
        assert_eq!(state.entry_frame(), entry_frame);
        assert_eq!(state.top_frame(), top_frame);
        assert_eq!(state.kind(), Some(EntryKind::Script));
        assert!(!state.disallows_user_observable_work());
        assert!(state.storage_backed_entries().is_empty());
        assert!(state.storage_backed_entry_exits().is_empty());
    }

    fn active_native_entry_state(
        entry_frame: Option<FrameAddress>,
        top_frame: Option<FrameAddress>,
    ) -> VmEntryState {
        VmEntryState {
            entry_depth: 1,
            entry_frame,
            top_frame,
            kind: Some(EntryKind::Script),
            ..VmEntryState::default()
        }
    }

    fn baseline_gate(
        owner: CodeBlockId,
        artifact: BaselineEntryArtifact,
        readiness_ordinal: u64,
    ) -> BaselineEntryGateRecord {
        BaselineEntryGateRecord {
            owner,
            requested_tier: JitType::Baseline,
            native_artifact: Some(artifact),
            native_entry_readiness_ordinal: Some(readiness_ordinal),
            generated_artifact: None,
            outcome: BaselineEntryGateOutcome::NativeEntryReadyButExecutionDisabled,
        }
    }

    fn enabled_baseline_gate(
        owner: CodeBlockId,
        artifact: BaselineEntryArtifact,
        readiness_ordinal: u64,
    ) -> BaselineEntryGateRecord {
        BaselineEntryGateRecord {
            owner,
            requested_tier: JitType::Baseline,
            native_artifact: Some(artifact),
            native_entry_readiness_ordinal: Some(readiness_ordinal),
            generated_artifact: None,
            outcome: BaselineEntryGateOutcome::NativeEntryReady,
        }
    }

    fn readiness_record(
        owner: CodeBlockId,
        artifact: BaselineEntryArtifact,
        ordinal: u64,
    ) -> BaselineNativeEntryReadinessRecord {
        BaselineNativeEntryReadinessRecord {
            ordinal,
            owner,
            materialization_ordinal: 1,
            install_ordinal: 2,
            artifact_id: Some(artifact.id),
            native_code: Some(artifact.native_code),
            machine_code: Some(artifact.machine_code),
            machine_range: Some(artifact.machine_code.range),
            bytecode_snapshot: None,
            body_capability: None,
            execution_policy: BaselineNativeEntryExecutionPolicy::Disabled,
            descriptor: Some(
                artifact
                    .validate_native_entry_descriptor()
                    .expect("native entry descriptor"),
            ),
            callable: None,
            outcome: BaselineNativeEntryReadinessOutcome::ReadyButExecutionDisabled,
        }
    }

    fn enabled_readiness_record(
        owner: CodeBlockId,
        artifact: BaselineEntryArtifact,
        ordinal: u64,
    ) -> BaselineNativeEntryReadinessRecord {
        let descriptor = artifact
            .validate_native_entry_descriptor()
            .expect("native entry descriptor");
        BaselineNativeEntryReadinessRecord {
            ordinal,
            owner,
            materialization_ordinal: 1,
            install_ordinal: 2,
            artifact_id: Some(artifact.id),
            native_code: Some(artifact.native_code),
            machine_code: Some(artifact.machine_code),
            machine_range: Some(artifact.machine_code.range),
            bytecode_snapshot: None,
            body_capability: Some(
                BaselineGeneratedCodeBodyCapability::from_supported_opcode_subset(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                ),
            ),
            execution_policy: BaselineNativeEntryExecutionPolicy::Enabled,
            descriptor: Some(descriptor),
            callable: Some(
                BaselineNativeEntryCallableAuthority::new_p6_pure_baseline_native_entry_shim(
                    descriptor,
                ),
            ),
            outcome: BaselineNativeEntryReadinessOutcome::Ready,
        }
    }

    fn baseline_entry_artifact(owner: CodeBlockId, id: u64) -> BaselineEntryArtifact {
        baseline_artifact(owner, id)
            .validate_baseline_entry_artifact(owner)
            .expect("baseline entry artifact")
    }

    fn baseline_artifact(owner: CodeBlockId, id: u64) -> JitCodeArtifact {
        let code = JitCodeId(id);
        let native_code = NativeCodeId(id as u32 + 100);
        let allocation = ExecutableAllocationId(id + 200);
        JitCodeArtifact {
            id: code,
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(owner),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(native_code),
            machine_code: Some(MachineCodeHandle {
                allocation,
                owner: MachineCodeOwnership::CodeBlock(owner),
                range: MachineCodeRange {
                    allocation,
                    start_offset: 0,
                    size_bytes: 64,
                },
                symbol: Some(native_code),
                protection: ExecutableMemoryProtection::Executable,
                lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            }),
            entrypoint: Entrypoint {
                kind: EntrypointKind::GeneratedCode,
                abi: EntryAbi::GeneratedCode,
                code: Some(code),
                boundary: None,
            },
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Live,
            finalization_authority: CodeFinalizationAuthority::MainThread,
        }
    }
}
