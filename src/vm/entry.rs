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
    next_ordinal: u64,
}

impl VmEntryState {
    pub fn entry_depth(&self) -> usize {
        self.entry_depth
    }

    pub fn top_frame(&self) -> Option<FrameAddress> {
        self.top_frame
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

    pub(crate) fn record_launch_descriptor(&mut self, descriptor: VmEntryLaunchDescriptor) {
        self.launch_descriptors.push(descriptor);
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

    pub fn root_scopes(&self) -> impl Iterator<Item = VmEntryRootScope> + '_ {
        self.records
            .iter()
            .map(|record| record.root_scope)
            .filter(move |scope| {
                !self
                    .exits
                    .iter()
                    .any(|exit| exit.closed_root_scope.ordinal == scope.ordinal)
            })
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
    use super::*;
    use crate::gc::CellId;
    use crate::jit::code::BaselineEntryArtifact;
    use crate::jit::BaselineNativeEntryCallableAuthority;
    use crate::jit::{
        CodeFinalizationAuthority, CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership,
        EntryAbi, Entrypoint, EntrypointKind, ExecutableAllocationId,
        ExecutableAllocationLifecycle, ExecutableMemoryProtection, ExecutableMutationAuthority,
        JitCodeArtifact, MachineCodeOwnership,
    };

    #[test]
    fn entry_guard_records_entry_and_exit_restoration() {
        let mut state = VmEntryState::default();
        {
            let _outer = state.enter_rooted(Some(FrameAddress(10)), EntryKind::Script, HeapId(8));
        }

        assert_eq!(state.entry_depth(), 0);
        assert_eq!(state.records().len(), 1);
        assert_eq!(state.exits().len(), 1);
        assert_eq!(state.records()[0].top_frame, Some(FrameAddress(10)));
        assert_eq!(state.records()[0].root_scope.heap, HeapId(8));
        assert_eq!(state.exits()[0].restored_top_frame, None);
        assert_eq!(state.root_scopes().count(), 0);
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
