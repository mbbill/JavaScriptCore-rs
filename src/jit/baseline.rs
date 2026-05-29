//! Typed baseline generated-code executor for the P6 subset.
//!
//! This is a generated-body stand-in, not a VM entrypoint. It validates the
//! generated-code artifact, executes the active interpreter frame through the
//! interpreter register boundary, and hands unsupported cases back to the
//! interpreter as an explicit baseline fallback request.

use crate::bytecode::instruction::{
    DecodedInstruction, InstructionDecodeError, Operand, OperandAccessError,
};
use crate::bytecode::{
    BytecodeIndex, BytecodeRootMapId, CodeBlock, CoreOpcode, Opcode, PropertyCacheKey,
    VirtualRegister,
};
use crate::gc::StructureId;
use crate::interpreter::{
    baseline_active_frame, baseline_read_register, baseline_return_register,
    baseline_write_register, create_call_return_continuation, BaselineFallbackRequest,
    CallReturnContinuation, CallReturnContinuationRequest, CallReturnKind, DispatchHost,
    ExecutionCompletion, ExecutionError, GeneratedNativeIntrinsicCallRequest,
    GeneratedNativeIntrinsicCallResult, InterpreterExecutionState, RegisterWindow,
};
use crate::jit::ic::{
    GeneratedCallLinkDirectCall, GeneratedPropertyStoreMutationCommit,
    GeneratedPropertyStoreMutationMissReason, GeneratedPropertyStoreMutationRequest,
    GeneratedPropertyStoreMutationResult,
};
#[cfg(test)]
pub(crate) use crate::jit::plan::BaselineGeneratedRuntimeHelperProof;
use crate::jit::plan::{
    derive_baseline_generated_property_handoff_plan_from_code_block,
    validate_baseline_generated_property_handoff_site_against_current_code_block,
    validate_baseline_generated_property_handoff_site_metadata,
    BaselineBytecodeSnapshotFingerprint, BaselineGeneratedOwnerContinuationMapMetadata,
    BaselineGeneratedOwnerContinuationSite, BaselineGeneratedRuntimeBoundaryProof,
    CompilerSafepointId,
};
pub(crate) use crate::jit::plan::{
    BaselineGeneratedPropertyHandoffPlan, BaselineGeneratedPropertyHandoffSite,
    BaselineGeneratedRuntimeHelperPlan,
};
use crate::jit::{
    BaselineBytecodeEligibilityProof, BaselineGeneratedCodeArtifact,
    BaselineNativeEntryCallableAuthority, BaselineNativeEntryCallableValidationError,
    BaselineSupportedOpcodeSubset, CacheKey, CallBoundaryId, GeneratedCallLinkCandidate,
    GeneratedCallLinkCandidateTable, GeneratedCallLinkDirectCallStatus,
    GeneratedCallLinkProbeMissReason, GeneratedCallLinkProbeRequest, GeneratedCallLinkProbeResult,
    GeneratedGuardedPropertyLoadProbeMissReason, GeneratedGuardedPropertyLoadProbeRequest,
    GeneratedGuardedPropertyLoadProbeResult, GeneratedPropertyHasMegamorphicCandidateTable,
    GeneratedPropertyHasMegamorphicLookup, GeneratedPropertyLoadMegamorphicCandidateTable,
    GeneratedPropertyLoadMegamorphicHolderProbeRequest, GeneratedPropertyLoadMegamorphicLookup,
    GeneratedPropertyLoadProbeMissReason, GeneratedPropertyLoadProbeRequest,
    GeneratedPropertyLoadProbeResult, GeneratedPropertyStoreMegamorphicCandidateTable,
    GeneratedPropertyStoreMegamorphicLookup, GeneratedPropertyStoreProbeMissReason,
    GeneratedPropertyStoreProbeRequest, GeneratedPropertyStoreProbeResult,
    InlineCacheFallbackSemantics, InlineCacheKind, InlineCacheSlotId, JitCodeValidationError,
    JitPlanValidationError, PropertyLoadAccessCasePlan, PropertyLoadAccessCasePlanKind,
    PropertyLoadAccessCasePlanTable, PropertyLoadGuardChainOutcome, PropertyLoadGuardRequirement,
    PropertyLoadGuardedCandidateKind, PropertyLoadGuardedCandidateTable,
    PropertyStoreAccessCasePlan, PropertyStoreAccessCasePlanKind, PropertyStoreMutationCandidate,
    PropertyStoreMutationCandidateTable, WatchpointSetId,
};
use crate::object::PropertyOffset;
use crate::runtime::{CallFrameId, CodeBlockId, ExecutableId, ObjectId, RuntimeValue};
use crate::strings::{PropertyIndex, PropertyKey};
use crate::value::{NumberValue, ValueKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedExecutionResult {
    Completed(ExecutionCompletion),
    Fallback(BaselineGeneratedFallback),
    JsCall(BaselineGeneratedJsCallHandoff),
    Property(BaselineGeneratedPropertyHandoff),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedExecutionWithRuntimeHelpersResult {
    Completed(ExecutionCompletion),
    Fallback(BaselineGeneratedFallback),
    JsCall(BaselineGeneratedJsCallHandoff),
    Property(BaselineGeneratedPropertyHandoff),
    RuntimeHelper(BaselineGeneratedRuntimeHelperHandoff),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedLoopHintObservation {
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedExecutionMetrics {
    pub(crate) executed_bytecode_count: u64,
    loop_hint_observations: Vec<BaselineGeneratedLoopHintObservation>,
}

impl BaselineGeneratedExecutionMetrics {
    fn record_loop_hint(&mut self, bytecode_index: BytecodeIndex) {
        if let Some(observation) = self
            .loop_hint_observations
            .iter_mut()
            .find(|observation| observation.bytecode_index == bytecode_index)
        {
            observation.count = observation.count.saturating_add(1);
            return;
        }

        self.loop_hint_observations
            .push(BaselineGeneratedLoopHintObservation {
                bytecode_index,
                count: 1,
            });
    }

    pub(crate) fn loop_hint_observations(&self) -> &[BaselineGeneratedLoopHintObservation] {
        &self.loop_hint_observations
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedExecutionValidation {
    Full,
    Prevalidated {
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
    pub(crate) frame: CallFrameId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) destination: VirtualRegister,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyLoadProbeMissRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) key: CacheKey,
    pub(crate) base_structure: Option<StructureId>,
    pub(crate) offset: Option<PropertyOffset>,
    pub(crate) reason: GeneratedPropertyLoadProbeMissReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedGuardedPropertyLoadProbeMissRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) slot: InlineCacheSlotId,
    pub(crate) guard_plan_ordinal: u64,
    pub(crate) materialization_ordinal: u64,
    pub(crate) dependency_ordinals: Vec<u64>,
    pub(crate) binding_set_ids: Vec<WatchpointSetId>,
    pub(crate) candidate_kind: PropertyLoadGuardedCandidateKind,
    pub(crate) base_structure: StructureId,
    pub(crate) reason: GeneratedGuardedPropertyLoadProbeMissReason,
    pub(crate) requirement: PropertyLoadGuardRequirement,
    pub(crate) key: CacheKey,
    pub(crate) prototype_depth: u16,
    pub(crate) chain_index: Option<usize>,
    pub(crate) outcome: PropertyLoadGuardChainOutcome,
}

pub(crate) struct BaselineGeneratedPropertyLoadExecutionSidecar<'plan, 'host> {
    plan_table: &'plan PropertyLoadAccessCasePlanTable,
    guarded_candidate_table: &'plan PropertyLoadGuardedCandidateTable,
    dispatch_host: &'host mut dyn DispatchHost,
    destination_root_sync_requests: Vec<BaselineGeneratedPropertyLoadDestinationRootSyncRequest>,
    probe_miss_records: Vec<BaselineGeneratedPropertyLoadProbeMissRecord>,
    guarded_probe_miss_records: Vec<BaselineGeneratedGuardedPropertyLoadProbeMissRecord>,
}

impl<'plan, 'host> BaselineGeneratedPropertyLoadExecutionSidecar<'plan, 'host> {
    #[allow(dead_code)]
    pub(crate) fn new(
        plan_table: &'plan PropertyLoadAccessCasePlanTable,
        guarded_candidate_table: &'plan PropertyLoadGuardedCandidateTable,
        dispatch_host: &'host mut dyn DispatchHost,
    ) -> Self {
        Self {
            plan_table,
            guarded_candidate_table,
            dispatch_host,
            destination_root_sync_requests: Vec::new(),
            probe_miss_records: Vec::new(),
            guarded_probe_miss_records: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn destination_root_sync_requests(
        &self,
    ) -> &[BaselineGeneratedPropertyLoadDestinationRootSyncRequest] {
        &self.destination_root_sync_requests
    }

    #[allow(dead_code)]
    pub(crate) fn probe_miss_records(&self) -> &[BaselineGeneratedPropertyLoadProbeMissRecord] {
        &self.probe_miss_records
    }

    #[allow(dead_code)]
    pub(crate) fn guarded_probe_miss_records(
        &self,
    ) -> &[BaselineGeneratedGuardedPropertyLoadProbeMissRecord] {
        &self.guarded_probe_miss_records
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BaselineGeneratedPropertyLoadSidecarProbeAttempt<'code, 'instruction, 'site> {
    pub(crate) owner: CodeBlockId,
    pub(crate) frame: CallFrameId,
    pub(crate) window: RegisterWindow,
    pub(crate) code_block: &'code CodeBlock,
    pub(crate) instruction: DecodedInstruction<'instruction>,
    pub(crate) site: &'site BaselineGeneratedPropertyHandoffSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyLoadSidecarProbeHit {
    pub(crate) next_bytecode_index: Option<BytecodeIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedPropertyLoadOrIncrementSidecarProbeResult {
    Hit(BaselineGeneratedPropertyLoadSidecarProbeHit),
    Property(BaselineGeneratedPropertyHandoff),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedPropertyIncrementNumericMode {
    AnyNumber,
    Int32NoOverflow,
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_property_load_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: BaselineGeneratedPropertyLoadSidecarProbeAttempt<'_, '_, '_>,
) -> Result<Option<BaselineGeneratedPropertyLoadSidecarProbeHit>, BaselineGeneratedExecutionError> {
    let fallback = fallback_site(attempt.owner, attempt.frame, attempt.instruction);
    let next_bytecode_index =
        next_baseline_generated_bytecode_index(attempt.code_block, attempt.instruction);
    match execute_property_load_sidecar_candidate(
        sidecars,
        execution,
        PropertyLoadSidecarAttempt {
            window: attempt.window,
            code_block: attempt.code_block,
            fallback,
            frame: attempt.frame,
            instruction: attempt.instruction,
            site: attempt.site,
        },
    ) {
        Ok(Some(BaselineInstructionOutcome::Continue)) => {
            Ok(Some(BaselineGeneratedPropertyLoadSidecarProbeHit {
                next_bytecode_index,
            }))
        }
        Ok(Some(BaselineInstructionOutcome::ContinueTo(target))) => {
            Ok(Some(BaselineGeneratedPropertyLoadSidecarProbeHit {
                next_bytecode_index: Some(target),
            }))
        }
        Ok(Some(_)) => Err(ExecutionError::BaselineGeneratedExecutionRejected.into()),
        Ok(None) | Err(BaselineInstructionAbort::Fallback(_)) => Ok(None),
        Err(BaselineInstructionAbort::Error(error)) => Err(error),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_property_load_or_increment_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: BaselineGeneratedPropertyLoadSidecarProbeAttempt<'_, '_, '_>,
    property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'_>>,
    increment_numeric_mode: BaselineGeneratedPropertyIncrementNumericMode,
) -> Result<
    Option<BaselineGeneratedPropertyLoadOrIncrementSidecarProbeResult>,
    BaselineGeneratedExecutionError,
> {
    let fallback = fallback_site(attempt.owner, attempt.frame, attempt.instruction);
    let next_bytecode_index =
        next_baseline_generated_bytecode_index(attempt.code_block, attempt.instruction);
    match execute_property_load_or_increment_sidecar_candidate(
        sidecars,
        execution,
        PropertyLoadSidecarAttempt {
            window: attempt.window,
            code_block: attempt.code_block,
            fallback,
            frame: attempt.frame,
            instruction: attempt.instruction,
            site: attempt.site,
        },
        attempt.owner,
        property_handoff_plan,
        increment_numeric_mode,
    ) {
        Ok(Some(BaselineInstructionOutcome::Continue)) => Ok(Some(
            BaselineGeneratedPropertyLoadOrIncrementSidecarProbeResult::Hit(
                BaselineGeneratedPropertyLoadSidecarProbeHit {
                    next_bytecode_index,
                },
            ),
        )),
        Ok(Some(BaselineInstructionOutcome::ContinueTo(target))) => Ok(Some(
            BaselineGeneratedPropertyLoadOrIncrementSidecarProbeResult::Hit(
                BaselineGeneratedPropertyLoadSidecarProbeHit {
                    next_bytecode_index: Some(target),
                },
            ),
        )),
        Ok(Some(BaselineInstructionOutcome::Property(handoff))) => Ok(Some(
            BaselineGeneratedPropertyLoadOrIncrementSidecarProbeResult::Property(handoff),
        )),
        Ok(Some(_)) => Err(ExecutionError::BaselineGeneratedExecutionRejected.into()),
        Ok(None) | Err(BaselineInstructionAbort::Fallback(_)) => Ok(None),
        Err(BaselineInstructionAbort::Error(error)) => Err(error),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BaselineGeneratedPropertyStoreSidecarProbeAttempt<'code, 'instruction, 'site> {
    pub(crate) owner: CodeBlockId,
    pub(crate) frame: CallFrameId,
    pub(crate) window: RegisterWindow,
    pub(crate) code_block: &'code CodeBlock,
    pub(crate) instruction: DecodedInstruction<'instruction>,
    pub(crate) site: &'site BaselineGeneratedPropertyHandoffSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyStoreSidecarProbeHit {
    pub(crate) next_bytecode_index: Option<BytecodeIndex>,
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_property_store_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: BaselineGeneratedPropertyStoreSidecarProbeAttempt<'_, '_, '_>,
) -> Result<Option<BaselineGeneratedPropertyStoreSidecarProbeHit>, BaselineGeneratedExecutionError>
{
    let fallback = fallback_site(attempt.owner, attempt.frame, attempt.instruction);
    let next_bytecode_index =
        next_baseline_generated_bytecode_index(attempt.code_block, attempt.instruction);
    match execute_property_store_sidecar_candidate(
        sidecars,
        execution,
        PropertyStoreSidecarAttempt {
            window: attempt.window,
            code_block: attempt.code_block,
            fallback,
            instruction: attempt.instruction,
            site: attempt.site,
        },
    ) {
        Ok(Some(BaselineInstructionOutcome::Continue)) => {
            Ok(Some(BaselineGeneratedPropertyStoreSidecarProbeHit {
                next_bytecode_index,
            }))
        }
        Ok(Some(BaselineInstructionOutcome::ContinueTo(target))) => {
            Ok(Some(BaselineGeneratedPropertyStoreSidecarProbeHit {
                next_bytecode_index: Some(target),
            }))
        }
        Ok(Some(_)) => Err(ExecutionError::BaselineGeneratedExecutionRejected.into()),
        Ok(None) | Err(BaselineInstructionAbort::Fallback(_)) => Ok(None),
        Err(BaselineInstructionAbort::Error(error)) => Err(error),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BaselineGeneratedPropertyHasSidecarProbeAttempt<'code, 'instruction, 'site> {
    pub(crate) owner: CodeBlockId,
    pub(crate) frame: CallFrameId,
    pub(crate) window: RegisterWindow,
    pub(crate) code_block: &'code CodeBlock,
    pub(crate) instruction: DecodedInstruction<'instruction>,
    pub(crate) site: &'site BaselineGeneratedPropertyHandoffSite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyHasSidecarProbeHit {
    pub(crate) next_bytecode_index: Option<BytecodeIndex>,
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_property_has_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: BaselineGeneratedPropertyHasSidecarProbeAttempt<'_, '_, '_>,
) -> Result<Option<BaselineGeneratedPropertyHasSidecarProbeHit>, BaselineGeneratedExecutionError> {
    let fallback = fallback_site(attempt.owner, attempt.frame, attempt.instruction);
    let next_bytecode_index =
        next_baseline_generated_bytecode_index(attempt.code_block, attempt.instruction);
    match execute_property_has_sidecar_candidate(
        sidecars,
        execution,
        PropertyHasSidecarAttempt {
            window: attempt.window,
            code_block: attempt.code_block,
            fallback,
            instruction: attempt.instruction,
            site: attempt.site,
        },
    ) {
        Ok(Some(BaselineInstructionOutcome::Continue)) => {
            Ok(Some(BaselineGeneratedPropertyHasSidecarProbeHit {
                next_bytecode_index,
            }))
        }
        Ok(Some(BaselineInstructionOutcome::ContinueTo(target))) => {
            Ok(Some(BaselineGeneratedPropertyHasSidecarProbeHit {
                next_bytecode_index: Some(target),
            }))
        }
        Ok(Some(_)) => Err(ExecutionError::BaselineGeneratedExecutionRejected.into()),
        Ok(None) | Err(BaselineInstructionAbort::Fallback(_)) => Ok(None),
        Err(BaselineInstructionAbort::Error(error)) => Err(error),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyStoreProbeMissRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) slot: InlineCacheSlotId,
    pub(crate) key: CacheKey,
    pub(crate) plan_kind: PropertyStoreAccessCasePlanKind,
    pub(crate) base_structure: Option<StructureId>,
    pub(crate) planned_new_structure: Option<StructureId>,
    pub(crate) offset: Option<PropertyOffset>,
    pub(crate) store_plan_ordinal: u64,
    pub(crate) readiness_ordinal: u64,
    pub(crate) stored_value_kind: ValueKind,
    pub(crate) reason: GeneratedPropertyStoreProbeMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyStoreMutationRejectionRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) slot: InlineCacheSlotId,
    pub(crate) key: CacheKey,
    pub(crate) plan_kind: PropertyStoreAccessCasePlanKind,
    pub(crate) base_structure: Option<StructureId>,
    pub(crate) planned_new_structure: Option<StructureId>,
    pub(crate) offset: Option<PropertyOffset>,
    pub(crate) store_plan_ordinal: u64,
    pub(crate) readiness_ordinal: u64,
    pub(crate) stored_value_kind: ValueKind,
    pub(crate) reason: GeneratedPropertyStoreMutationMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyStoreMutationCommitRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) slot: InlineCacheSlotId,
    pub(crate) key: CacheKey,
    pub(crate) plan_kind: PropertyStoreAccessCasePlanKind,
    pub(crate) store_plan_ordinal: u64,
    pub(crate) readiness_ordinal: u64,
    pub(crate) stored_value_kind: ValueKind,
    pub(crate) commit: GeneratedPropertyStoreMutationCommit,
}

pub(crate) struct BaselineGeneratedPropertyStoreExecutionSidecar<'plan, 'host> {
    candidate_table: &'plan PropertyStoreMutationCandidateTable,
    dispatch_host: &'host mut dyn DispatchHost,
    probe_miss_records: Vec<BaselineGeneratedPropertyStoreProbeMissRecord>,
    mutation_rejection_records: Vec<BaselineGeneratedPropertyStoreMutationRejectionRecord>,
    mutation_commit_records: Vec<BaselineGeneratedPropertyStoreMutationCommitRecord>,
}

impl<'plan, 'host> BaselineGeneratedPropertyStoreExecutionSidecar<'plan, 'host> {
    #[allow(dead_code)]
    pub(crate) fn new(
        candidate_table: &'plan PropertyStoreMutationCandidateTable,
        dispatch_host: &'host mut dyn DispatchHost,
    ) -> Self {
        Self {
            candidate_table,
            dispatch_host,
            probe_miss_records: Vec::new(),
            mutation_rejection_records: Vec::new(),
            mutation_commit_records: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn probe_miss_records(&self) -> &[BaselineGeneratedPropertyStoreProbeMissRecord] {
        &self.probe_miss_records
    }

    #[allow(dead_code)]
    pub(crate) fn mutation_rejection_records(
        &self,
    ) -> &[BaselineGeneratedPropertyStoreMutationRejectionRecord] {
        &self.mutation_rejection_records
    }

    #[allow(dead_code)]
    pub(crate) fn mutation_commit_records(
        &self,
    ) -> &[BaselineGeneratedPropertyStoreMutationCommitRecord] {
        &self.mutation_commit_records
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedCallLinkProbeMissRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) slot: Option<InlineCacheSlotId>,
    pub(crate) attachment_ordinal: Option<u64>,
    pub(crate) attachment_plan_ordinal: Option<u64>,
    pub(crate) install_recheck_ordinal: Option<u64>,
    pub(crate) boundary_validation_ordinal: Option<u64>,
    pub(crate) descriptor_ordinal: Option<u64>,
    pub(crate) observation_ordinal: Option<u64>,
    pub(crate) readiness_ordinal: Option<u64>,
    pub(crate) target_executable: Option<ExecutableId>,
    pub(crate) target_callee: Option<ObjectId>,
    pub(crate) target_code_block: Option<CodeBlockId>,
    pub(crate) target_boundary: Option<CallBoundaryId>,
    pub(crate) direct_call_status: Option<GeneratedCallLinkDirectCallStatus>,
    pub(crate) reason: GeneratedCallLinkProbeMissReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedCallLinkProbeBlockedRecord {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) slot: InlineCacheSlotId,
    pub(crate) attachment_ordinal: u64,
    pub(crate) attachment_plan_ordinal: u64,
    pub(crate) install_recheck_ordinal: u64,
    pub(crate) boundary_validation_ordinal: Option<u64>,
    pub(crate) descriptor_ordinal: Option<u64>,
    pub(crate) observation_ordinal: Option<u64>,
    pub(crate) readiness_ordinal: Option<u64>,
    pub(crate) target_executable: ExecutableId,
    pub(crate) target_callee: ObjectId,
    pub(crate) target_code_block: CodeBlockId,
    pub(crate) target_boundary: CallBoundaryId,
    pub(crate) direct_call_status: GeneratedCallLinkDirectCallStatus,
    pub(crate) reason: GeneratedCallLinkProbeMissReason,
}

pub(crate) struct BaselineGeneratedCallLinkExecutionSidecar<'plan, 'host> {
    candidate_table: &'plan GeneratedCallLinkCandidateTable,
    direct_call_hot_slots: &'plan [BaselineGeneratedJsDirectCallHotSlot],
    dispatch_host: &'host mut dyn DispatchHost,
    probe_miss_records: Vec<BaselineGeneratedCallLinkProbeMissRecord>,
    probe_blocked_records: Vec<BaselineGeneratedCallLinkProbeBlockedRecord>,
    direct_call_hot_slot_hits: usize,
}

impl<'plan, 'host> BaselineGeneratedCallLinkExecutionSidecar<'plan, 'host> {
    #[allow(dead_code)]
    pub(crate) fn new(
        candidate_table: &'plan GeneratedCallLinkCandidateTable,
        dispatch_host: &'host mut dyn DispatchHost,
    ) -> Self {
        Self::new_with_direct_call_hot_slots(candidate_table, &[], dispatch_host)
    }

    pub(crate) fn new_with_direct_call_hot_slots(
        candidate_table: &'plan GeneratedCallLinkCandidateTable,
        direct_call_hot_slots: &'plan [BaselineGeneratedJsDirectCallHotSlot],
        dispatch_host: &'host mut dyn DispatchHost,
    ) -> Self {
        Self {
            candidate_table,
            direct_call_hot_slots,
            dispatch_host,
            probe_miss_records: Vec::new(),
            probe_blocked_records: Vec::new(),
            direct_call_hot_slot_hits: 0,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn probe_miss_records(&self) -> &[BaselineGeneratedCallLinkProbeMissRecord] {
        &self.probe_miss_records
    }

    #[allow(dead_code)]
    pub(crate) fn probe_blocked_records(&self) -> &[BaselineGeneratedCallLinkProbeBlockedRecord] {
        &self.probe_blocked_records
    }

    pub(crate) fn direct_call_hot_slot_hits(&self) -> usize {
        self.direct_call_hot_slot_hits
    }
}

pub(crate) struct BaselineGeneratedPropertyExecutionSidecars<'plan, 'host> {
    property_load_plan_table: Option<&'plan PropertyLoadAccessCasePlanTable>,
    property_load_guarded_candidate_table: Option<&'plan PropertyLoadGuardedCandidateTable>,
    property_load_megamorphic_candidate_table:
        Option<&'plan GeneratedPropertyLoadMegamorphicCandidateTable>,
    property_store_candidate_table: Option<&'plan PropertyStoreMutationCandidateTable>,
    property_store_megamorphic_candidate_table:
        Option<&'plan GeneratedPropertyStoreMegamorphicCandidateTable>,
    property_has_megamorphic_candidate_table:
        Option<&'plan GeneratedPropertyHasMegamorphicCandidateTable>,
    generated_call_link_candidate_table: Option<&'plan GeneratedCallLinkCandidateTable>,
    generated_direct_call_hot_slots: &'plan [BaselineGeneratedJsDirectCallHotSlot],
    dispatch_host: &'host mut dyn DispatchHost,
    destination_root_sync_requests: Vec<BaselineGeneratedPropertyLoadDestinationRootSyncRequest>,
    property_load_probe_miss_records: Vec<BaselineGeneratedPropertyLoadProbeMissRecord>,
    guarded_property_load_probe_miss_records:
        Vec<BaselineGeneratedGuardedPropertyLoadProbeMissRecord>,
    property_store_probe_miss_records: Vec<BaselineGeneratedPropertyStoreProbeMissRecord>,
    property_store_mutation_rejection_records:
        Vec<BaselineGeneratedPropertyStoreMutationRejectionRecord>,
    property_store_mutation_commit_records: Vec<BaselineGeneratedPropertyStoreMutationCommitRecord>,
    generated_call_link_probe_miss_records: Vec<BaselineGeneratedCallLinkProbeMissRecord>,
    generated_call_link_probe_blocked_records: Vec<BaselineGeneratedCallLinkProbeBlockedRecord>,
    generated_direct_call_hot_slot_hits: usize,
}

impl<'plan, 'host> BaselineGeneratedPropertyExecutionSidecars<'plan, 'host> {
    #[allow(dead_code)]
    pub(crate) fn new(
        dispatch_host: &'host mut dyn DispatchHost,
        property_load_tables: Option<(
            &'plan PropertyLoadAccessCasePlanTable,
            &'plan PropertyLoadGuardedCandidateTable,
        )>,
        property_store_candidate_table: Option<&'plan PropertyStoreMutationCandidateTable>,
    ) -> Self {
        let (property_load_plan_table, property_load_guarded_candidate_table) =
            match property_load_tables {
                Some((plan_table, guarded_candidate_table)) => {
                    (Some(plan_table), Some(guarded_candidate_table))
                }
                None => (None, None),
            };

        Self {
            property_load_plan_table,
            property_load_guarded_candidate_table,
            property_load_megamorphic_candidate_table: None,
            property_store_candidate_table,
            property_store_megamorphic_candidate_table: None,
            property_has_megamorphic_candidate_table: None,
            generated_call_link_candidate_table: None,
            generated_direct_call_hot_slots: &[],
            dispatch_host,
            destination_root_sync_requests: Vec::new(),
            property_load_probe_miss_records: Vec::new(),
            guarded_property_load_probe_miss_records: Vec::new(),
            property_store_probe_miss_records: Vec::new(),
            property_store_mutation_rejection_records: Vec::new(),
            property_store_mutation_commit_records: Vec::new(),
            generated_call_link_probe_miss_records: Vec::new(),
            generated_call_link_probe_blocked_records: Vec::new(),
            generated_direct_call_hot_slot_hits: 0,
        }
    }

    pub(crate) fn new_with_generated_call_link(
        dispatch_host: &'host mut dyn DispatchHost,
        property_load_tables: Option<(
            &'plan PropertyLoadAccessCasePlanTable,
            &'plan PropertyLoadGuardedCandidateTable,
        )>,
        property_store_candidate_table: Option<&'plan PropertyStoreMutationCandidateTable>,
        generated_call_link_candidate_table: &'plan GeneratedCallLinkCandidateTable,
    ) -> Self {
        let mut sidecars = Self::new(
            dispatch_host,
            property_load_tables,
            property_store_candidate_table,
        );
        sidecars.generated_call_link_candidate_table = Some(generated_call_link_candidate_table);
        sidecars
    }

    pub(crate) fn new_with_generated_call_link_and_direct_call_hot_slots(
        dispatch_host: &'host mut dyn DispatchHost,
        property_load_tables: Option<(
            &'plan PropertyLoadAccessCasePlanTable,
            &'plan PropertyLoadGuardedCandidateTable,
        )>,
        property_store_candidate_table: Option<&'plan PropertyStoreMutationCandidateTable>,
        generated_call_link_candidate_table: &'plan GeneratedCallLinkCandidateTable,
        hot_slots: &'plan [BaselineGeneratedJsDirectCallHotSlot],
    ) -> Self {
        let mut sidecars = Self::new_with_generated_call_link(
            dispatch_host,
            property_load_tables,
            property_store_candidate_table,
            generated_call_link_candidate_table,
        );
        sidecars.generated_direct_call_hot_slots = hot_slots;
        sidecars
    }

    pub(crate) fn with_property_load_megamorphic_candidate_table(
        mut self,
        table: Option<&'plan GeneratedPropertyLoadMegamorphicCandidateTable>,
    ) -> Self {
        self.property_load_megamorphic_candidate_table = table;
        self
    }

    pub(crate) fn with_property_store_megamorphic_candidate_table(
        mut self,
        table: Option<&'plan GeneratedPropertyStoreMegamorphicCandidateTable>,
    ) -> Self {
        self.property_store_megamorphic_candidate_table = table;
        self
    }

    pub(crate) fn with_property_has_megamorphic_candidate_table(
        mut self,
        table: Option<&'plan GeneratedPropertyHasMegamorphicCandidateTable>,
    ) -> Self {
        self.property_has_megamorphic_candidate_table = table;
        self
    }

    #[allow(dead_code)]
    pub(crate) fn destination_root_sync_requests(
        &self,
    ) -> &[BaselineGeneratedPropertyLoadDestinationRootSyncRequest] {
        &self.destination_root_sync_requests
    }

    pub(crate) fn property_load_probe_miss_records(
        &self,
    ) -> &[BaselineGeneratedPropertyLoadProbeMissRecord] {
        &self.property_load_probe_miss_records
    }

    pub(crate) fn guarded_property_load_probe_miss_records(
        &self,
    ) -> &[BaselineGeneratedGuardedPropertyLoadProbeMissRecord] {
        &self.guarded_property_load_probe_miss_records
    }

    pub(crate) fn property_store_probe_miss_records(
        &self,
    ) -> &[BaselineGeneratedPropertyStoreProbeMissRecord] {
        &self.property_store_probe_miss_records
    }

    pub(crate) fn property_store_mutation_rejection_records(
        &self,
    ) -> &[BaselineGeneratedPropertyStoreMutationRejectionRecord] {
        &self.property_store_mutation_rejection_records
    }

    pub(crate) fn property_store_mutation_commit_records(
        &self,
    ) -> &[BaselineGeneratedPropertyStoreMutationCommitRecord] {
        &self.property_store_mutation_commit_records
    }

    pub(crate) fn generated_call_link_probe_miss_records(
        &self,
    ) -> &[BaselineGeneratedCallLinkProbeMissRecord] {
        &self.generated_call_link_probe_miss_records
    }

    pub(crate) fn generated_call_link_probe_blocked_records(
        &self,
    ) -> &[BaselineGeneratedCallLinkProbeBlockedRecord] {
        &self.generated_call_link_probe_blocked_records
    }

    pub(crate) fn generated_direct_call_hot_slot_hits(&self) -> usize {
        self.generated_direct_call_hot_slot_hits
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedFallback {
    pub(crate) request: BaselineFallbackRequest,
    pub(crate) reason: BaselineGeneratedFallbackReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedFallbackReason {
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: BaselineGeneratedFallbackOpcode,
    pub(crate) cause: BaselineGeneratedFallbackCause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedFallbackOpcode {
    Core(CoreOpcode),
    NonCore(Opcode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedFallbackCause {
    UnsupportedOpcode,
    NonInt32Operand {
        operand_index: u32,
        register: VirtualRegister,
    },
    NonNumberOperand {
        operand_index: u32,
        register: VirtualRegister,
        value_kind: ValueKind,
    },
    UnsupportedPrimitiveNumericCoercionOperand {
        operand_index: u32,
        register: VirtualRegister,
        value_kind: ValueKind,
    },
    UnsupportedTruthinessOperand {
        operand_index: u32,
        register: VirtualRegister,
        value_kind: ValueKind,
    },
    UnsupportedStrictEqualityOperand {
        operand_index: u32,
        register: VirtualRegister,
        value_kind: ValueKind,
    },
    Int32Overflow,
    OperandAccess {
        error: OperandAccessError,
    },
    BadImmediate {
        operand_index: u32,
        error: OperandAccessError,
    },
    RegisterRead {
        register: VirtualRegister,
        error: BaselineGeneratedRegisterFallbackCause,
    },
    RegisterWrite {
        register: VirtualRegister,
        error: BaselineGeneratedRegisterFallbackCause,
    },
    BadReturnRegister {
        register: VirtualRegister,
        error: BaselineGeneratedRegisterFallbackCause,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedRegisterFallbackCause {
    InvalidRegister,
    CannotWriteConstant,
    CannotAddressHeaderAsValue,
    MissingConstantPool,
    DeferredConstant,
    RegisterOutOfBounds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BaselineGeneratedFallbackSite {
    request: BaselineFallbackRequest,
    bytecode_index: BytecodeIndex,
    opcode: BaselineGeneratedFallbackOpcode,
}

impl BaselineGeneratedFallbackSite {
    const fn with_cause(self, cause: BaselineGeneratedFallbackCause) -> BaselineGeneratedFallback {
        BaselineGeneratedFallback {
            request: self.request,
            reason: BaselineGeneratedFallbackReason {
                bytecode_index: self.bytecode_index,
                opcode: self.opcode,
                cause,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedRuntimeHelperResume {
    pub(crate) owner: CodeBlockId,
    pub(crate) frame: CallFrameId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedRuntimeHelperHandoff {
    pub(crate) resume: BaselineGeneratedRuntimeHelperResume,
    pub(crate) safepoint: CompilerSafepointId,
    pub(crate) root_map: BytecodeRootMapId,
    pub(crate) root_count: usize,
    pub(crate) requires_no_gc_exit_reentry: bool,
    pub(crate) may_throw: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsCallResume {
    pub(crate) owner: CodeBlockId,
    pub(crate) frame: CallFrameId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsCallHandoff {
    pub(crate) resume: BaselineGeneratedJsCallResume,
    pub(crate) continuation: BaselineGeneratedJsCallHandoffContinuation,
    pub(crate) owner_continuation: Option<BaselineGeneratedOwnerContinuationSite>,
    pub(crate) direct_call: Option<Box<BaselineGeneratedJsDirectCall>>,
    pub(crate) requires_no_gc_exit_reentry: bool,
    pub(crate) may_throw: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedJsCallHandoffContinuation {
    Call(CallReturnContinuation),
    Construct(BaselineGeneratedJsConstructContinuation),
}

impl BaselineGeneratedJsCallHandoffContinuation {
    pub(crate) const fn as_call(&self) -> Option<&CallReturnContinuation> {
        match self {
            Self::Call(continuation) => Some(continuation),
            Self::Construct(_) => None,
        }
    }

    pub(crate) const fn as_construct(&self) -> Option<&BaselineGeneratedJsConstructContinuation> {
        match self {
            Self::Call(_) => None,
            Self::Construct(continuation) => Some(continuation),
        }
    }

    pub(crate) const fn destination(&self) -> VirtualRegister {
        match self {
            Self::Call(continuation) => continuation.destination,
            Self::Construct(continuation) => continuation.destination,
        }
    }

    pub(crate) const fn resume_bytecode_index(&self) -> Option<BytecodeIndex> {
        match self {
            Self::Call(continuation) => continuation.resume_bytecode_index,
            Self::Construct(continuation) => continuation.resume_bytecode_index,
        }
    }

    pub(crate) const fn argument_count_including_this(&self) -> u32 {
        match self {
            Self::Call(continuation) => continuation.argument_count_including_this,
            Self::Construct(continuation) => continuation.argument_count_including_this,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsConstructContinuation {
    pub(crate) caller_frame: CallFrameId,
    pub(crate) caller_window: RegisterWindow,
    pub(crate) owner: CodeBlockId,
    pub(crate) construct_bytecode_index: BytecodeIndex,
    pub(crate) resume_bytecode_index: Option<BytecodeIndex>,
    pub(crate) destination: VirtualRegister,
    pub(crate) argument_count_including_this: u32,
    pub(crate) callee_value: RuntimeValue,
    pub(crate) callee_object: Option<ObjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsDirectCall {
    pub(crate) candidate: GeneratedCallLinkCandidate,
    pub(crate) authorization: GeneratedCallLinkDirectCall,
    pub(crate) callee_value: RuntimeValue,
    pub(crate) callee_object: ObjectId,
    pub(crate) this_value: RuntimeValue,
    pub(crate) this_object: Option<ObjectId>,
    pub(crate) argument_count_including_this: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsDirectCallHotSlot {
    pub(crate) candidate: GeneratedCallLinkCandidate,
    pub(crate) authorization: GeneratedCallLinkDirectCall,
    pub(crate) callee_object: ObjectId,
    pub(crate) argument_count_including_this: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedJsCallHandoffError {
    NonCoreOpcode { opcode: Opcode },
    InvalidBytecodeIndex { bytecode_index: BytecodeIndex },
    UnsupportedOpcode { opcode: CoreOpcode },
    OperandAccess { error: OperandAccessError },
    RegisterRead { error: ExecutionError },
    Continuation { error: ExecutionError },
}

pub(crate) fn baseline_generated_js_call_handoff(
    owner: CodeBlockId,
    frame: CallFrameId,
    window: RegisterWindow,
    code_block: &CodeBlock,
    execution: &InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
) -> Result<BaselineGeneratedJsCallHandoff, BaselineGeneratedJsCallHandoffError> {
    let opcode = CoreOpcode::from_opcode(instruction.opcode).ok_or(
        BaselineGeneratedJsCallHandoffError::NonCoreOpcode {
            opcode: instruction.opcode,
        },
    )?;
    let bytecode_index = instruction.bytecode_index;
    if !bytecode_index.is_valid() {
        return Err(BaselineGeneratedJsCallHandoffError::InvalidBytecodeIndex { bytecode_index });
    }
    if !matches!(
        opcode,
        CoreOpcode::Call | CoreOpcode::CallWithThis | CoreOpcode::Construct
    ) {
        return Err(BaselineGeneratedJsCallHandoffError::UnsupportedOpcode { opcode });
    }
    let destination = instruction
        .register_operand(0)
        .map_err(|error| BaselineGeneratedJsCallHandoffError::OperandAccess { error })?;
    let callee_register = instruction
        .register_operand(1)
        .map_err(|error| BaselineGeneratedJsCallHandoffError::OperandAccess { error })?;
    let callee_value =
        baseline_read_register(execution.registers, code_block, window, callee_register)
            .map_err(|error| BaselineGeneratedJsCallHandoffError::RegisterRead { error })?;
    let provided_argument_count = match opcode {
        CoreOpcode::Call | CoreOpcode::Construct => instruction.unsigned_immediate_operand(2),
        CoreOpcode::CallWithThis => instruction.unsigned_immediate_operand(3),
        _ => unreachable!(),
    }
    .map_err(|error| BaselineGeneratedJsCallHandoffError::OperandAccess { error })?;
    let resume_bytecode_index = next_decoded_bytecode_index(code_block, bytecode_index)
        .map_err(|error| BaselineGeneratedJsCallHandoffError::Continuation { error })?;
    let argument_count_including_this = provided_argument_count.saturating_add(1);
    let continuation = match opcode {
        CoreOpcode::Call | CoreOpcode::CallWithThis => {
            let kind = CallReturnKind::from_opcode(opcode)
                .ok_or(BaselineGeneratedJsCallHandoffError::UnsupportedOpcode { opcode })?;
            BaselineGeneratedJsCallHandoffContinuation::Call(
                create_call_return_continuation(
                    execution.stack,
                    execution.registers,
                    execution.heap,
                    CallReturnContinuationRequest {
                        caller_frame: frame,
                        caller_window: window,
                        owner,
                        call_bytecode_index: bytecode_index,
                        resume_bytecode_index,
                        destination,
                        argument_count_including_this,
                        callee_value: Some(callee_value),
                        kind,
                    },
                )
                .map_err(|error| BaselineGeneratedJsCallHandoffError::Continuation { error })?,
            )
        }
        CoreOpcode::Construct => BaselineGeneratedJsCallHandoffContinuation::Construct(
            create_generated_js_construct_continuation(
                owner,
                frame,
                window,
                bytecode_index,
                resume_bytecode_index,
                destination,
                argument_count_including_this,
                callee_value,
                execution,
            )
            .map_err(|error| BaselineGeneratedJsCallHandoffError::Continuation { error })?,
        ),
        _ => unreachable!(),
    };

    Ok(BaselineGeneratedJsCallHandoff {
        resume: BaselineGeneratedJsCallResume {
            owner,
            frame,
            bytecode_index,
            opcode,
        },
        continuation,
        owner_continuation: None,
        direct_call: None,
        requires_no_gc_exit_reentry: true,
        may_throw: true,
    })
}

#[allow(clippy::too_many_arguments)]
fn create_generated_js_construct_continuation(
    owner: CodeBlockId,
    frame: CallFrameId,
    window: RegisterWindow,
    construct_bytecode_index: BytecodeIndex,
    resume_bytecode_index: Option<BytecodeIndex>,
    destination: VirtualRegister,
    argument_count_including_this: u32,
    callee_value: RuntimeValue,
    execution: &InterpreterExecutionState<'_>,
) -> Result<BaselineGeneratedJsConstructContinuation, ExecutionError> {
    let Some(active_frame) = execution.stack.top_frame() else {
        return Err(ExecutionError::NoActiveFrame);
    };
    if active_frame.id != frame {
        return Err(ExecutionError::FrameMismatch {
            expected: frame,
            actual: Some(active_frame.id),
        });
    }
    if active_frame.code_block != Some(owner) {
        return Err(ExecutionError::CodeBlockMismatch {
            expected: owner,
            actual: active_frame.code_block,
        });
    }
    if active_frame.register_window != window {
        return Err(ExecutionError::RegisterWindowMismatch);
    }
    if argument_count_including_this == 0 {
        return Err(ExecutionError::InvalidArgumentCount);
    }
    let _ = execution.registers.active_window_authority(frame, window)?;
    execution
        .registers
        .validate_writable_register(window, destination)?;
    let callee_object = object_id_for_runtime_value(execution, callee_value);

    Ok(BaselineGeneratedJsConstructContinuation {
        caller_frame: frame,
        caller_window: window,
        owner,
        construct_bytecode_index,
        resume_bytecode_index,
        destination,
        argument_count_including_this,
        callee_value,
        callee_object,
    })
}

fn next_decoded_bytecode_index(
    code_block: &CodeBlock,
    bytecode_index: BytecodeIndex,
) -> Result<Option<BytecodeIndex>, ExecutionError> {
    let next = bytecode_index.offset().saturating_add(1);
    match code_block.decoded_instruction_at(BytecodeIndex::from_offset(next)) {
        Ok(instruction) => Ok(Some(instruction.bytecode_index)),
        Err(InstructionDecodeError::MissingInstruction { .. }) => Ok(None),
        Err(_) => Err(ExecutionError::InvalidBytecodeIndex(
            BytecodeIndex::from_offset(next),
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyResume {
    pub(crate) owner: CodeBlockId,
    pub(crate) frame: CallFrameId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyHandoff {
    pub(crate) resume: BaselineGeneratedPropertyResume,
    pub(crate) site: BaselineGeneratedPropertyHandoffSite,
    pub(crate) requires_no_gc_exit_reentry: bool,
    pub(crate) may_throw: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedPropertyHandoffError {
    NonCoreOpcode {
        opcode: Opcode,
    },
    InvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    UnsupportedOpcode {
        opcode: CoreOpcode,
    },
    MissingSiteMetadata {
        bytecode_index: BytecodeIndex,
    },
    SiteMetadataAmbiguous {
        bytecode_index: BytecodeIndex,
    },
    SiteMetadata(JitPlanValidationError),
    SiteOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    SiteBytecodeIndexMismatch {
        instruction: BytecodeIndex,
        site: BytecodeIndex,
    },
    SiteOpcodeMismatch {
        instruction: CoreOpcode,
        site: CoreOpcode,
    },
    SiteCacheKindMismatch {
        opcode: CoreOpcode,
        cache_kind: InlineCacheKind,
    },
    SiteFallbackMismatch {
        opcode: CoreOpcode,
        fallback: InlineCacheFallbackSemantics,
    },
}

pub(crate) fn baseline_generated_property_handoff(
    owner: CodeBlockId,
    frame: CallFrameId,
    instruction: DecodedInstruction<'_>,
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Result<BaselineGeneratedPropertyHandoff, BaselineGeneratedPropertyHandoffError> {
    let opcode = CoreOpcode::from_opcode(instruction.opcode).ok_or(
        BaselineGeneratedPropertyHandoffError::NonCoreOpcode {
            opcode: instruction.opcode,
        },
    )?;
    let bytecode_index = instruction.bytecode_index;
    if !bytecode_index.is_valid() {
        return Err(BaselineGeneratedPropertyHandoffError::InvalidBytecodeIndex { bytecode_index });
    }
    if !matches!(
        opcode,
        CoreOpcode::GetByName
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::GetLength
            | CoreOpcode::PutByName
            | CoreOpcode::PutGlobalObjectProperty
            | CoreOpcode::GetByValue
            | CoreOpcode::PutByValue
            | CoreOpcode::InById
            | CoreOpcode::InByVal
    ) {
        return Err(BaselineGeneratedPropertyHandoffError::UnsupportedOpcode { opcode });
    }
    if site.owner != owner {
        return Err(BaselineGeneratedPropertyHandoffError::SiteOwnerMismatch {
            expected: owner,
            actual: site.owner,
        });
    }
    if site.bytecode_index != bytecode_index {
        return Err(
            BaselineGeneratedPropertyHandoffError::SiteBytecodeIndexMismatch {
                instruction: bytecode_index,
                site: site.bytecode_index,
            },
        );
    }
    if site.opcode != opcode {
        return Err(BaselineGeneratedPropertyHandoffError::SiteOpcodeMismatch {
            instruction: opcode,
            site: site.opcode,
        });
    }
    let expected_cache_kind = match opcode {
        CoreOpcode::GetByName | CoreOpcode::GetGlobalObjectProperty => {
            InlineCacheKind::PropertyLoad
        }
        CoreOpcode::GetLength => InlineCacheKind::PropertyLoad,
        CoreOpcode::PutByName | CoreOpcode::PutGlobalObjectProperty => {
            InlineCacheKind::PropertyStore
        }
        CoreOpcode::GetByValue => InlineCacheKind::ElementLoad,
        CoreOpcode::PutByValue => InlineCacheKind::ElementStore,
        CoreOpcode::InById | CoreOpcode::InByVal => InlineCacheKind::HasProperty,
        _ => unreachable!(),
    };
    if site.cache_kind != expected_cache_kind {
        return Err(
            BaselineGeneratedPropertyHandoffError::SiteCacheKindMismatch {
                opcode,
                cache_kind: site.cache_kind,
            },
        );
    }
    if site.fallback != InlineCacheFallbackSemantics::SlowPathLookup {
        return Err(
            BaselineGeneratedPropertyHandoffError::SiteFallbackMismatch {
                opcode,
                fallback: site.fallback,
            },
        );
    }
    validate_baseline_generated_property_handoff_site_metadata(site)
        .map_err(BaselineGeneratedPropertyHandoffError::SiteMetadata)?;

    Ok(BaselineGeneratedPropertyHandoff {
        resume: BaselineGeneratedPropertyResume {
            owner,
            frame,
            bytecode_index,
            opcode,
        },
        site: *site,
        requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
        may_throw: site.may_throw,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedRuntimeHelperHandoffError {
    NonCoreOpcode {
        opcode: Opcode,
    },
    InvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    OpcodeMismatch {
        instruction: CoreOpcode,
        proof: CoreOpcode,
    },
    MissingNoGcExitReentry {
        opcode: CoreOpcode,
    },
    MissingCompleteSafepointRootMap {
        opcode: CoreOpcode,
    },
    MissingRootMap {
        opcode: CoreOpcode,
        safepoint: CompilerSafepointId,
    },
    ContractDoesNotCallRuntimeHelper {
        opcode: CoreOpcode,
    },
    ContractDoesNotTouchGcRoots {
        opcode: CoreOpcode,
    },
    MayThrowMismatch {
        opcode: CoreOpcode,
        proof_may_throw: bool,
        contract_may_throw: bool,
    },
}

pub(crate) fn baseline_generated_runtime_helper_handoff(
    owner: CodeBlockId,
    frame: CallFrameId,
    instruction: DecodedInstruction<'_>,
    proof: &BaselineGeneratedRuntimeBoundaryProof,
) -> Result<BaselineGeneratedRuntimeHelperHandoff, BaselineGeneratedRuntimeHelperHandoffError> {
    let opcode = CoreOpcode::from_opcode(instruction.opcode).ok_or(
        BaselineGeneratedRuntimeHelperHandoffError::NonCoreOpcode {
            opcode: instruction.opcode,
        },
    )?;
    let bytecode_index = instruction.bytecode_index;
    if !bytecode_index.is_valid() {
        return Err(
            BaselineGeneratedRuntimeHelperHandoffError::InvalidBytecodeIndex { bytecode_index },
        );
    }
    if proof.contract.opcode != opcode {
        return Err(BaselineGeneratedRuntimeHelperHandoffError::OpcodeMismatch {
            instruction: opcode,
            proof: proof.contract.opcode,
        });
    }
    if !proof.contract.effects.calls_runtime_helper {
        return Err(
            BaselineGeneratedRuntimeHelperHandoffError::ContractDoesNotCallRuntimeHelper { opcode },
        );
    }
    if !proof.contract.effects.touches_gc_roots {
        return Err(
            BaselineGeneratedRuntimeHelperHandoffError::ContractDoesNotTouchGcRoots { opcode },
        );
    }
    if proof.may_throw != proof.contract.effects.may_throw {
        return Err(
            BaselineGeneratedRuntimeHelperHandoffError::MayThrowMismatch {
                opcode,
                proof_may_throw: proof.may_throw,
                contract_may_throw: proof.contract.effects.may_throw,
            },
        );
    }
    if !proof.contract.requirements.no_gc_exit_reentry || !proof.no_gc_exit_reentry {
        return Err(BaselineGeneratedRuntimeHelperHandoffError::MissingNoGcExitReentry { opcode });
    }
    if !proof.contract.requirements.complete_safepoint_root_map {
        return Err(
            BaselineGeneratedRuntimeHelperHandoffError::MissingCompleteSafepointRootMap { opcode },
        );
    }
    let root_map =
        proof
            .root_map
            .ok_or(BaselineGeneratedRuntimeHelperHandoffError::MissingRootMap {
                opcode,
                safepoint: proof.safepoint,
            })?;

    Ok(BaselineGeneratedRuntimeHelperHandoff {
        resume: BaselineGeneratedRuntimeHelperResume {
            owner,
            frame,
            bytecode_index,
            opcode,
        },
        safepoint: proof.safepoint,
        root_map,
        root_count: proof.root_count,
        requires_no_gc_exit_reentry: proof.no_gc_exit_reentry,
        may_throw: proof.may_throw,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedExecutionError {
    ArtifactValidation(JitCodeValidationError),
    OwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    UnsupportedOpcodeSubset {
        expected: BaselineSupportedOpcodeSubset,
        actual: BaselineSupportedOpcodeSubset,
    },
    CodeBlockSnapshotValidation(JitPlanValidationError),
    CodeBlockSnapshotMismatch {
        expected: BaselineBytecodeSnapshotFingerprint,
        actual: BaselineBytecodeSnapshotFingerprint,
    },
    InstructionDecode {
        bytecode_index: BytecodeIndex,
        error: InstructionDecodeError,
    },
    RuntimeHelperHandoff {
        bytecode_index: BytecodeIndex,
        opcode: BaselineGeneratedFallbackOpcode,
        error: BaselineGeneratedRuntimeHelperHandoffError,
    },
    JsCallHandoff {
        bytecode_index: BytecodeIndex,
        opcode: BaselineGeneratedFallbackOpcode,
        error: BaselineGeneratedJsCallHandoffError,
    },
    PropertyHandoff {
        bytecode_index: BytecodeIndex,
        opcode: BaselineGeneratedFallbackOpcode,
        error: BaselineGeneratedPropertyHandoffError,
    },
    RuntimeHelperProofAmbiguous {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    UnexpectedRuntimeHelper(BaselineGeneratedRuntimeHelperHandoff),
    Execution(ExecutionError),
}

impl From<ExecutionError> for BaselineGeneratedExecutionError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(error)
    }
}

pub(crate) struct BaselineGeneratedExecutionRequest<'code, 'exec> {
    pub(crate) artifact: &'code BaselineGeneratedCodeArtifact,
    pub(crate) owner: CodeBlockId,
    pub(crate) code_block: &'code CodeBlock,
    pub(crate) expected_frame: CallFrameId,
    pub(crate) execution: InterpreterExecutionState<'exec>,
}

pub(crate) struct BaselineNativeEntryShimExecutionRequest<'code, 'exec> {
    pub(crate) callable: BaselineNativeEntryCallableAuthority,
    pub(crate) owner: CodeBlockId,
    pub(crate) code_block: &'code CodeBlock,
    pub(crate) expected_frame: CallFrameId,
    pub(crate) execution: InterpreterExecutionState<'exec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineNativeEntryShimExecutionResult {
    Completed(ExecutionCompletion),
    Fallback(BaselineGeneratedFallback),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineNativeEntryShimExecutionError {
    CallableAuthority(Box<BaselineNativeEntryCallableValidationError>),
    InstructionDecode {
        bytecode_index: BytecodeIndex,
        error: InstructionDecodeError,
    },
    Execution(ExecutionError),
    Generated(Box<BaselineGeneratedExecutionError>),
}

impl From<ExecutionError> for BaselineNativeEntryShimExecutionError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(error)
    }
}

pub(crate) fn execute_baseline_p6_native_entry_shim(
    request: BaselineNativeEntryShimExecutionRequest<'_, '_>,
) -> Result<BaselineNativeEntryShimExecutionResult, BaselineNativeEntryShimExecutionError> {
    let BaselineNativeEntryShimExecutionRequest {
        callable,
        owner,
        code_block,
        expected_frame,
        mut execution,
    } = request;
    let descriptor = callable.descriptor();
    callable
        .validate_for_descriptor(&descriptor)
        .map_err(|error| {
            BaselineNativeEntryShimExecutionError::CallableAuthority(Box::new(error))
        })?;
    if descriptor.owner != owner {
        return Err(BaselineNativeEntryShimExecutionError::CallableAuthority(
            Box::new(BaselineNativeEntryCallableValidationError::OwnerMismatch {
                expected: owner,
                actual: descriptor.owner,
            }),
        ));
    }

    let opcode_subset = callable.kind().supported_opcode_subset();
    let (frame, window) = baseline_active_frame(execution.stack, expected_frame, owner)?;
    let instruction_count = code_block.unlinked().instructions().instruction_count();
    if instruction_count == 0 {
        return Ok(BaselineNativeEntryShimExecutionResult::Completed(
            ExecutionCompletion::Returned(RuntimeValue::undefined()),
        ));
    }

    let mut pc = frame
        .bytecode_index
        .unwrap_or_else(|| BytecodeIndex::from_offset(0));
    loop {
        let ordinal = pc.offset() as usize;
        if ordinal >= instruction_count {
            return Err(ExecutionError::InvalidBytecodeIndex(pc).into());
        }

        let instruction = code_block.decoded_instruction_at(pc).map_err(|error| {
            BaselineNativeEntryShimExecutionError::InstructionDecode {
                bytecode_index: pc,
                error,
            }
        })?;
        let bytecode_index = instruction.bytecode_index;
        if !bytecode_index.is_valid() {
            return Err(ExecutionError::InvalidBytecodeIndex(bytecode_index).into());
        }
        execution.stack.mark_top_bytecode_index(bytecode_index);

        let outcome = match execute_native_entry_shim_instruction(
            BaselineInstructionContext::new(opcode_subset, owner, expected_frame, code_block, None),
            window,
            &mut execution,
            instruction,
        ) {
            Ok(outcome) => outcome,
            Err(BaselineInstructionAbort::Fallback(request)) => {
                BaselineInstructionOutcome::Fallback(request)
            }
            Err(BaselineInstructionAbort::Error(error)) => {
                return Err(BaselineNativeEntryShimExecutionError::Generated(Box::new(
                    error,
                )));
            }
        };

        match outcome {
            BaselineInstructionOutcome::Continue => {
                let next = ordinal.saturating_add(1);
                if next >= instruction_count {
                    return Ok(BaselineNativeEntryShimExecutionResult::Completed(
                        ExecutionCompletion::Returned(RuntimeValue::undefined()),
                    ));
                }
                pc = BytecodeIndex::from_offset(next as u32);
            }
            BaselineInstructionOutcome::ContinueTo(target) => {
                let target_ordinal = target.offset() as usize;
                if target_ordinal >= instruction_count {
                    return Err(ExecutionError::InvalidBytecodeIndex(target).into());
                }
                pc = target;
            }
            BaselineInstructionOutcome::Jump(target) => {
                if target.offset() as usize >= instruction_count {
                    return Err(ExecutionError::InvalidBytecodeIndex(target).into());
                }
                pc = target;
            }
            BaselineInstructionOutcome::Return(value) => {
                return Ok(BaselineNativeEntryShimExecutionResult::Completed(
                    ExecutionCompletion::Returned(value),
                ));
            }
            BaselineInstructionOutcome::Fallback(fallback) => {
                return Ok(BaselineNativeEntryShimExecutionResult::Fallback(fallback));
            }
            BaselineInstructionOutcome::JsCall(_) | BaselineInstructionOutcome::Property(_) => {
                let fallback = fallback_site(owner, expected_frame, instruction)
                    .with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode);
                return Ok(BaselineNativeEntryShimExecutionResult::Fallback(fallback));
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        None,
        None,
        None,
        BaselineGeneratedExecutionValidation::Full,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_metrics(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_with_metrics_and_validation(
        request,
        metrics,
        BaselineGeneratedExecutionValidation::Full,
    )
}

pub(crate) fn execute_baseline_generated_code_with_metrics_and_validation(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        None,
        None,
        Some(metrics),
        validation,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_property_load_sidecar(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    sidecar: &mut BaselineGeneratedPropertyLoadExecutionSidecar<'_, '_>,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    let mut property_sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
        &mut *sidecar.dispatch_host,
        Some((sidecar.plan_table, sidecar.guarded_candidate_table)),
        None,
    );
    let result =
        execute_baseline_generated_code_with_property_sidecars(request, &mut property_sidecars);
    sidecar
        .destination_root_sync_requests
        .extend_from_slice(property_sidecars.destination_root_sync_requests());
    sidecar
        .probe_miss_records
        .extend_from_slice(property_sidecars.property_load_probe_miss_records());
    sidecar
        .guarded_probe_miss_records
        .extend_from_slice(property_sidecars.guarded_property_load_probe_miss_records());
    result
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_generated_call_link_sidecar(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        None,
        Some(sidecar),
        None,
        BaselineGeneratedExecutionValidation::Full,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_generated_call_link_sidecar_and_metrics(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_with_generated_call_link_sidecar_and_metrics_and_validation(
        request,
        sidecar,
        metrics,
        BaselineGeneratedExecutionValidation::Full,
    )
}

pub(crate) fn execute_baseline_generated_code_with_generated_call_link_sidecar_and_metrics_and_validation(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        None,
        Some(sidecar),
        Some(metrics),
        validation,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_property_store_sidecar(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    sidecar: &mut BaselineGeneratedPropertyStoreExecutionSidecar<'_, '_>,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    let mut property_sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
        &mut *sidecar.dispatch_host,
        None,
        Some(sidecar.candidate_table),
    );
    let result =
        execute_baseline_generated_code_with_property_sidecars(request, &mut property_sidecars);
    sidecar
        .probe_miss_records
        .extend_from_slice(property_sidecars.property_store_probe_miss_records());
    sidecar
        .mutation_rejection_records
        .extend_from_slice(property_sidecars.property_store_mutation_rejection_records());
    sidecar
        .mutation_commit_records
        .extend_from_slice(property_sidecars.property_store_mutation_commit_records());
    result
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_property_sidecars(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    property_sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        Some(property_sidecars),
        None,
        None,
        BaselineGeneratedExecutionValidation::Full,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_property_sidecars_and_metrics(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    property_sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_with_property_sidecars_and_metrics_and_validation(
        request,
        property_sidecars,
        metrics,
        BaselineGeneratedExecutionValidation::Full,
    )
}

pub(crate) fn execute_baseline_generated_code_with_property_sidecars_and_metrics_and_validation(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    property_sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        Some(property_sidecars),
        None,
        Some(metrics),
        validation,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_property_and_call_link_sidecars(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    property_sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    generated_call_link_sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
) -> Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError> {
    match execute_baseline_generated_code_internal(
        request,
        None,
        Some(property_sidecars),
        Some(generated_call_link_sidecar),
        None,
        BaselineGeneratedExecutionValidation::Full,
    )? {
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(completion) => {
            Ok(BaselineGeneratedExecutionResult::Completed(completion))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback) => {
            Ok(BaselineGeneratedExecutionResult::Fallback(fallback))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(handoff) => {
            Ok(BaselineGeneratedExecutionResult::JsCall(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff) => {
            Ok(BaselineGeneratedExecutionResult::Property(handoff))
        }
        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff) => Err(
            BaselineGeneratedExecutionError::UnexpectedRuntimeHelper(handoff),
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_runtime_helpers<'proof>(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_internal(
        request,
        Some(runtime_helper_plan),
        None,
        None,
        None,
        BaselineGeneratedExecutionValidation::Full,
    )
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_runtime_helpers_and_metrics<'proof>(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_with_runtime_helpers_and_metrics_and_validation(
        request,
        runtime_helper_plan,
        metrics,
        BaselineGeneratedExecutionValidation::Full,
    )
}

pub(crate) fn execute_baseline_generated_code_with_runtime_helpers_and_metrics_and_validation<
    'proof,
>(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_internal(
        request,
        Some(runtime_helper_plan),
        None,
        None,
        Some(metrics),
        validation,
    )
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_runtime_helpers_and_sidecars<'proof>(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
    property_sidecars: Option<&mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>>,
    generated_call_link_sidecar: Option<&mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>>,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_internal(
        request,
        Some(runtime_helper_plan),
        property_sidecars,
        generated_call_link_sidecar,
        None,
        BaselineGeneratedExecutionValidation::Full,
    )
}

#[allow(dead_code)]
pub(crate) fn execute_baseline_generated_code_with_runtime_helpers_and_sidecars_and_metrics<
    'proof,
>(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
    property_sidecars: Option<&mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>>,
    generated_call_link_sidecar: Option<&mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_with_runtime_helpers_and_sidecars_and_metrics_and_validation(
        request,
        runtime_helper_plan,
        property_sidecars,
        generated_call_link_sidecar,
        metrics,
        BaselineGeneratedExecutionValidation::Full,
    )
}

pub(crate) fn execute_baseline_generated_code_with_runtime_helpers_and_sidecars_and_metrics_and_validation<
    'proof,
>(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
    property_sidecars: Option<&mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>>,
    generated_call_link_sidecar: Option<&mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>>,
    metrics: &mut BaselineGeneratedExecutionMetrics,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    execute_baseline_generated_code_internal(
        request,
        Some(runtime_helper_plan),
        property_sidecars,
        generated_call_link_sidecar,
        Some(metrics),
        validation,
    )
}

fn execute_baseline_generated_code_internal(
    request: BaselineGeneratedExecutionRequest<'_, '_>,
    runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlan<'_>>,
    mut property_sidecars: Option<&mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>>,
    mut generated_call_link_sidecar: Option<&mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>>,
    mut metrics: Option<&mut BaselineGeneratedExecutionMetrics>,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError> {
    let BaselineGeneratedExecutionRequest {
        artifact,
        owner,
        code_block,
        expected_frame,
        mut execution,
    } = request;

    validate_generated_artifact_for_execution(
        artifact,
        owner,
        code_block,
        runtime_helper_plan,
        validation,
    )?;
    let property_handoff_plan = artifact.property_handoff_plan();
    if let Some(plan) = property_handoff_plan {
        match validation {
            BaselineGeneratedExecutionValidation::Full => {
                validate_property_handoff_plan_snapshot(code_block, plan)?;
            }
            BaselineGeneratedExecutionValidation::Prevalidated { bytecode_snapshot } => {
                if plan.bytecode_snapshot != bytecode_snapshot {
                    return Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch {
                        expected: bytecode_snapshot,
                        actual: plan.bytecode_snapshot,
                    });
                }
            }
        }
    }
    let opcode_subset = artifact.eligibility_proof.opcode_subset();
    let (frame, window) = baseline_active_frame(execution.stack, expected_frame, owner)?;
    let instruction_count = code_block.unlinked().instructions().instruction_count();
    if instruction_count == 0 {
        return Ok(
            BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::undefined()),
            ),
        );
    }

    let mut pc = frame
        .bytecode_index
        .unwrap_or_else(|| BytecodeIndex::from_offset(0));
    loop {
        let ordinal = pc.offset() as usize;
        if ordinal >= instruction_count {
            return Err(ExecutionError::InvalidBytecodeIndex(pc).into());
        }

        let instruction = code_block.decoded_instruction_at(pc).map_err(|error| {
            BaselineGeneratedExecutionError::InstructionDecode {
                bytecode_index: pc,
                error,
            }
        })?;
        let bytecode_index = instruction.bytecode_index;
        if !bytecode_index.is_valid() {
            return Err(ExecutionError::InvalidBytecodeIndex(bytecode_index).into());
        }
        execution.stack.mark_top_bytecode_index(bytecode_index);
        if let Some(metrics) = metrics.as_deref_mut() {
            metrics.executed_bytecode_count = metrics.executed_bytecode_count.saturating_add(1);
            if CoreOpcode::from_opcode(instruction.opcode) == Some(CoreOpcode::LoopHint) {
                metrics.record_loop_hint(bytecode_index);
            }
        }

        let outcome = match execute_instruction(
            BaselineInstructionContext::new(
                opcode_subset,
                owner,
                expected_frame,
                code_block,
                property_handoff_plan,
            )
            .with_owner_continuation_map(artifact.owner_continuation_map()),
            window,
            &mut execution,
            instruction,
            property_sidecars.as_deref_mut(),
            generated_call_link_sidecar.as_deref_mut(),
        ) {
            Ok(outcome) => outcome,
            Err(BaselineInstructionAbort::Fallback(request)) => {
                BaselineInstructionOutcome::Fallback(request)
            }
            Err(BaselineInstructionAbort::Error(error)) => return Err(error),
        };
        match outcome {
            BaselineInstructionOutcome::Continue => {
                let next = ordinal.saturating_add(1);
                if next >= instruction_count {
                    return Ok(
                        BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(
                            ExecutionCompletion::Returned(RuntimeValue::undefined()),
                        ),
                    );
                }
                pc = BytecodeIndex::from_offset(next as u32);
            }
            BaselineInstructionOutcome::ContinueTo(target) => {
                let target_ordinal = target.offset() as usize;
                if target_ordinal >= instruction_count {
                    return Err(ExecutionError::InvalidBytecodeIndex(target).into());
                }
                if let Some(metrics) = metrics.as_deref_mut() {
                    let skipped = target_ordinal.saturating_sub(ordinal.saturating_add(1));
                    metrics.executed_bytecode_count = metrics
                        .executed_bytecode_count
                        .saturating_add(skipped as u64);
                }
                pc = target;
            }
            BaselineInstructionOutcome::Jump(target) => {
                if target.offset() as usize >= instruction_count {
                    return Err(ExecutionError::InvalidBytecodeIndex(target).into());
                }
                pc = target;
            }
            BaselineInstructionOutcome::Return(value) => {
                return Ok(
                    BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(
                        ExecutionCompletion::Returned(value),
                    ),
                );
            }
            BaselineInstructionOutcome::JsCall(handoff) => {
                return Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::JsCall(
                    handoff,
                ));
            }
            BaselineInstructionOutcome::Property(handoff) => {
                return Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Property(handoff));
            }
            BaselineInstructionOutcome::Fallback(fallback) => {
                if let Some(handoff) = runtime_helper_handoff_for_fallback(
                    owner,
                    expected_frame,
                    instruction,
                    fallback,
                    runtime_helper_plan,
                )? {
                    return Ok(
                        BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(handoff),
                    );
                }
                return Ok(BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(fallback));
            }
        }
    }
}

fn validate_generated_artifact_for_execution(
    artifact: &BaselineGeneratedCodeArtifact,
    owner: CodeBlockId,
    code_block: &CodeBlock,
    runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlan<'_>>,
    validation: BaselineGeneratedExecutionValidation,
) -> Result<(), BaselineGeneratedExecutionError> {
    match validation {
        BaselineGeneratedExecutionValidation::Full => {
            if let Some(plan) = runtime_helper_plan {
                validate_generated_artifact_with_runtime_helpers(artifact, owner, code_block, plan)
            } else {
                validate_generated_artifact(artifact, owner, code_block)
            }
        }
        BaselineGeneratedExecutionValidation::Prevalidated { bytecode_snapshot } => {
            validate_generated_artifact_header(artifact, owner)?;
            let expected = artifact.eligibility_proof.bytecode_snapshot_fingerprint();
            if expected != bytecode_snapshot {
                return Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch {
                    expected,
                    actual: bytecode_snapshot,
                });
            }
            if let Some(plan) = runtime_helper_plan {
                if plan.bytecode_snapshot != bytecode_snapshot {
                    return Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch {
                        expected: bytecode_snapshot,
                        actual: plan.bytecode_snapshot,
                    });
                }
            }
            Ok(())
        }
    }
}

fn validate_generated_artifact(
    artifact: &BaselineGeneratedCodeArtifact,
    owner: CodeBlockId,
    code_block: &CodeBlock,
) -> Result<(), BaselineGeneratedExecutionError> {
    validate_generated_artifact_header(artifact, owner)?;
    validate_generated_artifact_snapshot(artifact, code_block)
}

fn validate_generated_artifact_with_runtime_helpers(
    artifact: &BaselineGeneratedCodeArtifact,
    owner: CodeBlockId,
    code_block: &CodeBlock,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'_>,
) -> Result<(), BaselineGeneratedExecutionError> {
    validate_generated_artifact_header(artifact, owner)?;
    validate_runtime_helper_plan_snapshot(code_block, runtime_helper_plan)?;
    validate_runtime_helper_code_block_coverage(
        code_block,
        artifact.eligibility_proof.opcode_subset(),
    )
}

fn validate_runtime_helper_plan_snapshot(
    code_block: &CodeBlock,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'_>,
) -> Result<(), BaselineGeneratedExecutionError> {
    let actual = BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block)
        .map_err(BaselineGeneratedExecutionError::CodeBlockSnapshotValidation)?;
    if actual != runtime_helper_plan.bytecode_snapshot {
        return Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch {
            expected: runtime_helper_plan.bytecode_snapshot,
            actual,
        });
    }
    Ok(())
}

fn validate_property_handoff_plan_snapshot(
    code_block: &CodeBlock,
    property_handoff_plan: BaselineGeneratedPropertyHandoffPlan<'_>,
) -> Result<(), BaselineGeneratedExecutionError> {
    let actual = BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block)
        .map_err(BaselineGeneratedExecutionError::CodeBlockSnapshotValidation)?;
    if actual != property_handoff_plan.bytecode_snapshot {
        return Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch {
            expected: property_handoff_plan.bytecode_snapshot,
            actual,
        });
    }
    Ok(())
}

fn validate_generated_artifact_header(
    artifact: &BaselineGeneratedCodeArtifact,
    owner: CodeBlockId,
) -> Result<(), BaselineGeneratedExecutionError> {
    artifact
        .validate()
        .map_err(BaselineGeneratedExecutionError::ArtifactValidation)?;
    if artifact.owner != owner {
        return Err(BaselineGeneratedExecutionError::OwnerMismatch {
            expected: artifact.owner,
            actual: owner,
        });
    }

    let actual = artifact.eligibility_proof.opcode_subset();
    if !baseline_generated_executor_supports_subset(actual) {
        return Err(BaselineGeneratedExecutionError::UnsupportedOpcodeSubset {
            expected:
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
            actual,
        });
    }
    Ok(())
}

fn validate_generated_artifact_snapshot(
    artifact: &BaselineGeneratedCodeArtifact,
    code_block: &CodeBlock,
) -> Result<(), BaselineGeneratedExecutionError> {
    let expected = artifact.eligibility_proof.bytecode_snapshot_fingerprint();
    let actual = BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block)
        .map_err(BaselineGeneratedExecutionError::CodeBlockSnapshotValidation)?;
    if actual != expected {
        return Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch {
            expected,
            actual,
        });
    }

    Ok(())
}

fn validate_runtime_helper_code_block_coverage(
    code_block: &CodeBlock,
    _opcode_subset: BaselineSupportedOpcodeSubset,
) -> Result<(), BaselineGeneratedExecutionError> {
    for (ordinal, instruction) in code_block
        .unlinked()
        .instructions()
        .decoded_instructions()
        .enumerate()
    {
        let instruction =
            instruction.map_err(|error| BaselineGeneratedExecutionError::InstructionDecode {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            })?;
        let bytecode_index = instruction.bytecode_index;
        if !bytecode_index.is_valid() {
            return Err(ExecutionError::InvalidBytecodeIndex(bytecode_index).into());
        }
        match CoreOpcode::from_opcode(instruction.opcode) {
            Some(_) => {}
            None => {
                return Err(ExecutionError::UnsupportedOpcode(instruction.opcode).into());
            }
        }
    }
    Ok(())
}

fn runtime_helper_handoff_for_fallback(
    owner: CodeBlockId,
    frame: CallFrameId,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallback,
    runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlan<'_>>,
) -> Result<Option<BaselineGeneratedRuntimeHelperHandoff>, BaselineGeneratedExecutionError> {
    if fallback.reason.cause != BaselineGeneratedFallbackCause::UnsupportedOpcode {
        return Ok(None);
    }
    let Some(runtime_helper_plan) = runtime_helper_plan else {
        return Ok(None);
    };
    let Some(proof) = runtime_helper_proof_for_instruction(instruction, runtime_helper_plan)?
    else {
        return Ok(None);
    };

    baseline_generated_runtime_helper_handoff(owner, frame, instruction, proof)
        .map(Some)
        .map_err(
            |error| BaselineGeneratedExecutionError::RuntimeHelperHandoff {
                bytecode_index: instruction.bytecode_index,
                opcode: fallback.reason.opcode,
                error,
            },
        )
}

fn runtime_helper_proof_for_instruction<'proof>(
    instruction: DecodedInstruction<'_>,
    runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'proof>,
) -> Result<Option<&'proof BaselineGeneratedRuntimeBoundaryProof>, BaselineGeneratedExecutionError>
{
    let Some(opcode) = CoreOpcode::from_opcode(instruction.opcode) else {
        return Ok(None);
    };
    runtime_helper_plan
        .proof_for_bytecode_index(instruction.bytecode_index)
        .map_err(
            |()| BaselineGeneratedExecutionError::RuntimeHelperProofAmbiguous {
                bytecode_index: instruction.bytecode_index,
                opcode,
            },
        )
}

fn property_handoff_site_for_instruction(
    owner: CodeBlockId,
    code_block: &CodeBlock,
    instruction: DecodedInstruction<'_>,
    property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'_>>,
) -> Result<BaselineGeneratedPropertyHandoffSite, BaselineGeneratedPropertyHandoffError> {
    let bytecode_index = instruction.bytecode_index;
    if let Some(plan) = property_handoff_plan {
        let site = plan
            .site_for_bytecode_index(bytecode_index)
            .map_err(
                |_| BaselineGeneratedPropertyHandoffError::SiteMetadataAmbiguous { bytecode_index },
            )?
            .copied()
            .ok_or(BaselineGeneratedPropertyHandoffError::MissingSiteMetadata { bytecode_index })?;
        validate_baseline_generated_property_handoff_site_against_current_code_block(
            code_block, owner, &site,
        )
        .map_err(BaselineGeneratedPropertyHandoffError::SiteMetadata)?;
        return Ok(site);
    }

    let derivation =
        derive_baseline_generated_property_handoff_plan_from_code_block(code_block, owner)
            .map_err(BaselineGeneratedPropertyHandoffError::SiteMetadata)?;
    let metadata = derivation
        .metadata
        .ok_or(BaselineGeneratedPropertyHandoffError::MissingSiteMetadata { bytecode_index })?;
    metadata
        .site_for_bytecode_index(bytecode_index)
        .copied()
        .ok_or(BaselineGeneratedPropertyHandoffError::MissingSiteMetadata { bytecode_index })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BaselineInstructionOutcome {
    Continue,
    ContinueTo(BytecodeIndex),
    Jump(BytecodeIndex),
    Return(RuntimeValue),
    JsCall(BaselineGeneratedJsCallHandoff),
    Property(BaselineGeneratedPropertyHandoff),
    Fallback(BaselineGeneratedFallback),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BaselineInstructionAbort {
    Fallback(BaselineGeneratedFallback),
    Error(BaselineGeneratedExecutionError),
}

#[derive(Clone, Copy, Debug)]
struct BaselineInstructionContext<'code, 'plan> {
    opcode_subset: BaselineSupportedOpcodeSubset,
    owner: CodeBlockId,
    frame: CallFrameId,
    code_block: &'code CodeBlock,
    property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'plan>>,
    owner_continuation_map: Option<&'plan BaselineGeneratedOwnerContinuationMapMetadata>,
}

impl<'code, 'plan> BaselineInstructionContext<'code, 'plan> {
    const fn new(
        opcode_subset: BaselineSupportedOpcodeSubset,
        owner: CodeBlockId,
        frame: CallFrameId,
        code_block: &'code CodeBlock,
        property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'plan>>,
    ) -> Self {
        Self {
            opcode_subset,
            owner,
            frame,
            code_block,
            property_handoff_plan,
            owner_continuation_map: None,
        }
    }

    const fn with_owner_continuation_map(
        mut self,
        owner_continuation_map: Option<&'plan BaselineGeneratedOwnerContinuationMapMetadata>,
    ) -> Self {
        self.owner_continuation_map = owner_continuation_map;
        self
    }
}

fn owner_continuation_site_for_handoff(
    owner_continuation_map: Option<&BaselineGeneratedOwnerContinuationMapMetadata>,
    handoff: &BaselineGeneratedJsCallHandoff,
) -> Option<BaselineGeneratedOwnerContinuationSite> {
    let site = owner_continuation_map?
        .call_site_for_bytecode_index(handoff.resume.bytecode_index)
        .copied()?;
    if site.owner == handoff.resume.owner
        && site.call_bytecode_index == handoff.resume.bytecode_index
        && site.opcode == handoff.resume.opcode
        && site.destination == handoff.continuation.destination()
        && site.argument_count_including_this
            == handoff.continuation.argument_count_including_this()
        && site.resume_bytecode_index == handoff.continuation.resume_bytecode_index()
    {
        Some(site)
    } else {
        None
    }
}

fn execute_instruction(
    context: BaselineInstructionContext<'_, '_>,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    mut property_sidecars: Option<&mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>>,
    mut generated_call_link_sidecar: Option<&mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>>,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let BaselineInstructionContext {
        opcode_subset,
        owner,
        frame,
        code_block,
        property_handoff_plan,
        owner_continuation_map,
    } = context;
    let fallback = fallback_site(owner, frame, instruction);
    let Some(opcode) = CoreOpcode::from_opcode(instruction.opcode) else {
        return Ok(BaselineInstructionOutcome::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
        ));
    };
    if !opcode_subset.supports(opcode) {
        if matches!(
            opcode,
            CoreOpcode::Call | CoreOpcode::CallWithThis | CoreOpcode::Construct
        ) {
            if matches!(opcode, CoreOpcode::Call | CoreOpcode::CallWithThis) {
                let native_intrinsic = if let Some(sidecar) = generated_call_link_sidecar.as_mut() {
                    execute_generated_native_intrinsic_call_sidecar_probe(
                        *sidecar,
                        execution,
                        GeneratedCallLinkSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            owner,
                            opcode,
                            instruction,
                        },
                    )?
                } else if let Some(sidecars) = property_sidecars.as_mut() {
                    execute_generated_native_intrinsic_call_property_sidecar_probe(
                        *sidecars,
                        execution,
                        GeneratedCallLinkSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            owner,
                            opcode,
                            instruction,
                        },
                    )?
                } else {
                    None
                };
                if let Some(outcome) = native_intrinsic {
                    return Ok(outcome);
                }
            }
            let direct_call = if matches!(opcode, CoreOpcode::Call | CoreOpcode::CallWithThis) {
                if let Some(sidecar) = generated_call_link_sidecar {
                    execute_generated_call_link_sidecar_probe(
                        sidecar,
                        execution,
                        GeneratedCallLinkSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            owner,
                            opcode,
                            instruction,
                        },
                    )?
                } else if let Some(sidecars) = property_sidecars {
                    execute_generated_call_link_property_sidecar_probe(
                        sidecars,
                        execution,
                        GeneratedCallLinkSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            owner,
                            opcode,
                            instruction,
                        },
                    )?
                } else {
                    None
                }
            } else {
                None
            };
            let mut handoff = baseline_generated_js_call_handoff(
                owner,
                frame,
                window,
                code_block,
                execution,
                instruction,
            )
            .map_err(|error| {
                BaselineInstructionAbort::Error(BaselineGeneratedExecutionError::JsCallHandoff {
                    bytecode_index: instruction.bytecode_index,
                    opcode: fallback_opcode(instruction.opcode),
                    error,
                })
            })?;
            handoff.owner_continuation =
                owner_continuation_site_for_handoff(owner_continuation_map, &handoff);
            handoff.direct_call = direct_call.map(Box::new);
            return Ok(BaselineInstructionOutcome::JsCall(handoff));
        }
        if matches!(
            opcode,
            CoreOpcode::GetByName
                | CoreOpcode::GetGlobalObjectProperty
                | CoreOpcode::GetLength
                | CoreOpcode::PutByName
                | CoreOpcode::PutGlobalObjectProperty
                | CoreOpcode::GetByValue
                | CoreOpcode::PutByValue
                | CoreOpcode::InById
                | CoreOpcode::InByVal
        ) {
            let site = property_handoff_site_for_instruction(
                owner,
                code_block,
                instruction,
                property_handoff_plan,
            )
            .map_err(|error| {
                BaselineInstructionAbort::Error(BaselineGeneratedExecutionError::PropertyHandoff {
                    bytecode_index: instruction.bytecode_index,
                    opcode: fallback_opcode(instruction.opcode),
                    error,
                })
            })?;
            if let Some(sidecars) = property_sidecars {
                if matches!(
                    opcode,
                    CoreOpcode::GetByName
                        | CoreOpcode::GetGlobalObjectProperty
                        | CoreOpcode::GetLength
                        | CoreOpcode::GetByValue
                ) {
                    if let Some(outcome) = execute_property_load_or_increment_sidecar_candidate(
                        sidecars,
                        execution,
                        PropertyLoadSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            frame,
                            instruction,
                            site: &site,
                        },
                        owner,
                        property_handoff_plan,
                        BaselineGeneratedPropertyIncrementNumericMode::AnyNumber,
                    )? {
                        return Ok(outcome);
                    }
                }
                if matches!(
                    opcode,
                    CoreOpcode::PutByName
                        | CoreOpcode::PutGlobalObjectProperty
                        | CoreOpcode::PutByValue
                ) {
                    if let Some(outcome) = execute_property_store_sidecar_candidate(
                        sidecars,
                        execution,
                        PropertyStoreSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            instruction,
                            site: &site,
                        },
                    )? {
                        return Ok(outcome);
                    }
                }
                if matches!(opcode, CoreOpcode::InById | CoreOpcode::InByVal) {
                    if let Some(outcome) = execute_property_has_sidecar_candidate(
                        sidecars,
                        execution,
                        PropertyHasSidecarAttempt {
                            window,
                            code_block,
                            fallback,
                            instruction,
                            site: &site,
                        },
                    )? {
                        return Ok(outcome);
                    }
                }
            }
            return baseline_generated_property_handoff(owner, frame, instruction, &site)
                .map(BaselineInstructionOutcome::Property)
                .map_err(|error| {
                    BaselineInstructionAbort::Error(
                        BaselineGeneratedExecutionError::PropertyHandoff {
                            bytecode_index: instruction.bytecode_index,
                            opcode: fallback_opcode(instruction.opcode),
                            error,
                        },
                    )
                });
        }
        return Ok(BaselineInstructionOutcome::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
        ));
    }

    match opcode {
        CoreOpcode::LoadUndefined => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            write_register_or_outcome(
                execution,
                window,
                destination,
                RuntimeValue::undefined(),
                fallback,
            )
        }
        CoreOpcode::LoadNull => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            write_register_or_outcome(
                execution,
                window,
                destination,
                RuntimeValue::null(),
                fallback,
            )
        }
        CoreOpcode::LoadBool => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            let value = match instruction.unsigned_immediate_operand(1) {
                Ok(value) => value != 0,
                Err(error) => {
                    return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
                        BaselineGeneratedFallbackCause::BadImmediate {
                            operand_index: 1,
                            error,
                        },
                    )));
                }
            };
            write_register_or_outcome(
                execution,
                window,
                destination,
                RuntimeValue::from_bool(value),
                fallback,
            )
        }
        CoreOpcode::LoadInt32 => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            let value = match instruction.signed_immediate_operand(1) {
                Ok(value) => value,
                Err(error) => {
                    return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
                        BaselineGeneratedFallbackCause::BadImmediate {
                            operand_index: 1,
                            error,
                        },
                    )));
                }
            };
            write_register_or_outcome(
                execution,
                window,
                destination,
                RuntimeValue::from_i32(value),
                fallback,
            )
        }
        CoreOpcode::LoadDouble => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            let low = match instruction.unsigned_immediate_operand(1) {
                Ok(value) => value,
                Err(error) => {
                    return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
                        BaselineGeneratedFallbackCause::BadImmediate {
                            operand_index: 1,
                            error,
                        },
                    )));
                }
            };
            let high = match instruction.unsigned_immediate_operand(2) {
                Ok(value) => value,
                Err(error) => {
                    return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
                        BaselineGeneratedFallbackCause::BadImmediate {
                            operand_index: 2,
                            error,
                        },
                    )));
                }
            };
            let bits = u64::from(low) | (u64::from(high) << 32);
            write_register_or_outcome(
                execution,
                window,
                destination,
                RuntimeValue::from_double(f64::from_bits(bits)),
                fallback,
            )
        }
        CoreOpcode::Move => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            let source = register_operand_or_fallback(instruction, 1, fallback)?;
            let value = read_register_or_outcome(execution, code_block, window, source, fallback)?;
            write_register_or_outcome(execution, window, destination, value, fallback)
        }
        CoreOpcode::LoadCallee => {
            let destination = register_operand_or_fallback(instruction, 0, fallback)?;
            let Some(callee) = execution
                .stack
                .top_frame()
                .and_then(|frame| frame.callee_value)
            else {
                return Ok(BaselineInstructionOutcome::Fallback(
                    fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
                ));
            };
            write_register_or_outcome(execution, window, destination, callee, fallback)
        }
        CoreOpcode::ToNumber => {
            execute_to_number(code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::Void => execute_void(code_block, window, execution, instruction, fallback),
        CoreOpcode::Return => {
            let source = register_operand_or_fallback(instruction, 0, fallback)?;
            let value =
                read_return_register_or_fallback(execution, code_block, window, source, fallback)?;
            Ok(BaselineInstructionOutcome::Return(value))
        }
        CoreOpcode::AddInt32 | CoreOpcode::SubInt32 | CoreOpcode::MulInt32 => {
            if baseline_subset_uses_pure_number_binary(opcode_subset) {
                execute_pure_number_arithmetic(
                    opcode,
                    code_block,
                    window,
                    execution,
                    instruction,
                    fallback,
                )
            } else {
                execute_int32_arithmetic(
                    opcode,
                    code_block,
                    window,
                    execution,
                    instruction,
                    fallback,
                )
            }
        }
        CoreOpcode::NegateNumber => {
            execute_negate_number(code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::DivNumber | CoreOpcode::ModNumber => {
            execute_number_arithmetic(opcode, code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::BitNotInt32 => {
            execute_int32_bit_not(code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::BitOrInt32
        | CoreOpcode::BitXorInt32
        | CoreOpcode::BitAndInt32
        | CoreOpcode::LeftShiftInt32
        | CoreOpcode::RightShiftInt32
        | CoreOpcode::UnsignedRightShiftInt32 => {
            if baseline_subset_uses_pure_number_binary(opcode_subset) {
                execute_pure_number_bitwise(
                    opcode,
                    code_block,
                    window,
                    execution,
                    instruction,
                    fallback,
                )
            } else {
                execute_int32_bitwise(opcode, code_block, window, execution, instruction, fallback)
            }
        }
        CoreOpcode::LessThanInt32
        | CoreOpcode::LessEqualInt32
        | CoreOpcode::GreaterThanInt32
        | CoreOpcode::GreaterEqualInt32 => {
            if baseline_subset_uses_pure_number_binary(opcode_subset) {
                execute_pure_number_relational(
                    opcode,
                    code_block,
                    window,
                    execution,
                    instruction,
                    fallback,
                )
            } else {
                execute_int32_relational(
                    opcode,
                    code_block,
                    window,
                    execution,
                    instruction,
                    fallback,
                )
            }
        }
        CoreOpcode::Jump => {
            let target = bytecode_index_operand_or_fallback(instruction, 0, fallback)?;
            Ok(BaselineInstructionOutcome::Jump(target))
        }
        CoreOpcode::JumpIfFalse => {
            execute_jump_if_false(code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::JumpIfNotNullish => {
            execute_jump_if_not_nullish(code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::LoopHint => {
            // C++ emit_op_loop_hint increments the baseline execute counter at
            // this bytecode index. The Rust generated-body executor records
            // that in BaselineGeneratedExecutionMetrics before dispatching the
            // instruction, so the bytecode operation itself is a no-op here.
            Ok(BaselineInstructionOutcome::Continue)
        }
        CoreOpcode::LogicalNot => {
            execute_logical_not(code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::StrictEqual | CoreOpcode::StrictNotEqual => {
            execute_strict_equality(opcode, code_block, window, execution, instruction, fallback)
        }
        CoreOpcode::Equal | CoreOpcode::NotEqual => execute_local_loose_equality(
            opcode,
            code_block,
            window,
            execution,
            instruction,
            fallback,
        ),
        _ => Ok(BaselineInstructionOutcome::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
        )),
    }
}

fn execute_native_entry_shim_instruction(
    context: BaselineInstructionContext<'_, '_>,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let fallback = fallback_site(context.owner, context.frame, instruction);
    let Some(opcode) = CoreOpcode::from_opcode(instruction.opcode) else {
        return Ok(BaselineInstructionOutcome::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
        ));
    };
    if !context.opcode_subset.supports(opcode) {
        return Ok(BaselineInstructionOutcome::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
        ));
    }
    execute_instruction(context, window, execution, instruction, None, None)
}

struct GeneratedCallLinkSidecarAttempt<'code, 'instruction> {
    window: RegisterWindow,
    code_block: &'code CodeBlock,
    fallback: BaselineGeneratedFallbackSite,
    owner: CodeBlockId,
    opcode: CoreOpcode,
    instruction: DecodedInstruction<'instruction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GeneratedCallLinkSidecarOperands {
    argument_count_including_this: u32,
    callee_value: RuntimeValue,
    callee_value_kind: ValueKind,
    callee_object: Option<ObjectId>,
    this_value: RuntimeValue,
    this_value_kind: ValueKind,
    this_object: Option<ObjectId>,
}

fn execute_generated_native_intrinsic_call_sidecar_probe(
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let BaselineGeneratedCallLinkExecutionSidecar { dispatch_host, .. } = sidecar;
    execute_generated_native_intrinsic_call_probe_with_host(
        &mut **dispatch_host,
        execution,
        attempt,
    )
}

fn execute_generated_native_intrinsic_call_property_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let BaselineGeneratedPropertyExecutionSidecars { dispatch_host, .. } = sidecars;
    execute_generated_native_intrinsic_call_probe_with_host(
        &mut **dispatch_host,
        execution,
        attempt,
    )
}

fn execute_generated_native_intrinsic_call_probe_with_host(
    dispatch_host: &mut dyn DispatchHost,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let GeneratedCallLinkSidecarAttempt {
        window,
        code_block,
        fallback,
        owner,
        opcode,
        instruction,
    } = attempt;
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let callee_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let callee_value =
        read_register_or_outcome(execution, code_block, window, callee_register, fallback)?;
    let (this_value, provided_argument_count, first_argument_operand) = match opcode {
        CoreOpcode::CallWithThis => {
            let this_register = register_operand_or_fallback(instruction, 2, fallback)?;
            let this_value =
                read_register_or_outcome(execution, code_block, window, this_register, fallback)?;
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 3, fallback)?;
            (this_value, argument_count, 4)
        }
        CoreOpcode::Call => {
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 2, fallback)?;
            (RuntimeValue::undefined(), argument_count, 3)
        }
        _ => return Ok(None),
    };
    let first_argument = if provided_argument_count == 0 {
        None
    } else {
        let argument_register =
            register_operand_or_fallback(instruction, first_argument_operand, fallback)?;
        Some(read_register_or_outcome(
            execution,
            code_block,
            window,
            argument_register,
            fallback,
        )?)
    };
    let second_argument = if provided_argument_count <= 1 {
        None
    } else {
        let second_argument_operand = first_argument_operand.saturating_add(1);
        let argument_register =
            register_operand_or_fallback(instruction, second_argument_operand, fallback)?;
        Some(read_register_or_outcome(
            execution,
            code_block,
            window,
            argument_register,
            fallback,
        )?)
    };
    let request = GeneratedNativeIntrinsicCallRequest {
        owner,
        bytecode_index: instruction.bytecode_index,
        opcode,
        callee_value,
        this_value,
        provided_argument_count,
        first_argument,
        second_argument,
    };
    match DispatchHost::dispatch_generated_native_intrinsic_call(
        dispatch_host,
        execution.heap,
        request,
    ) {
        GeneratedNativeIntrinsicCallResult::Hit(hit) => {
            write_register_or_outcome(execution, window, destination, hit.value, fallback).map(Some)
        }
        GeneratedNativeIntrinsicCallResult::Miss(_) => Ok(None),
    }
}

fn execute_generated_call_link_sidecar_probe(
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineGeneratedJsDirectCall>, BaselineInstructionAbort> {
    let BaselineGeneratedCallLinkExecutionSidecar {
        candidate_table,
        direct_call_hot_slots,
        dispatch_host,
        probe_miss_records,
        probe_blocked_records,
        direct_call_hot_slot_hits,
    } = sidecar;
    execute_generated_call_link_sidecar_probe_with_host(
        candidate_table,
        direct_call_hot_slots,
        &mut **dispatch_host,
        probe_miss_records,
        probe_blocked_records,
        direct_call_hot_slot_hits,
        execution,
        attempt,
    )
}

fn execute_generated_call_link_property_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineGeneratedJsDirectCall>, BaselineInstructionAbort> {
    let BaselineGeneratedPropertyExecutionSidecars {
        generated_call_link_candidate_table,
        generated_direct_call_hot_slots,
        dispatch_host,
        generated_call_link_probe_miss_records,
        generated_call_link_probe_blocked_records,
        generated_direct_call_hot_slot_hits,
        ..
    } = sidecars;
    let Some(candidate_table) = *generated_call_link_candidate_table else {
        return Ok(None);
    };
    execute_generated_call_link_sidecar_probe_with_host(
        candidate_table,
        generated_direct_call_hot_slots,
        &mut **dispatch_host,
        generated_call_link_probe_miss_records,
        generated_call_link_probe_blocked_records,
        generated_direct_call_hot_slot_hits,
        execution,
        attempt,
    )
}

fn execute_generated_call_link_sidecar_probe_with_host(
    candidate_table: &GeneratedCallLinkCandidateTable,
    direct_call_hot_slots: &[BaselineGeneratedJsDirectCallHotSlot],
    dispatch_host: &mut dyn DispatchHost,
    probe_miss_records: &mut Vec<BaselineGeneratedCallLinkProbeMissRecord>,
    probe_blocked_records: &mut Vec<BaselineGeneratedCallLinkProbeBlockedRecord>,
    direct_call_hot_slot_hits: &mut usize,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineGeneratedJsDirectCall>, BaselineInstructionAbort> {
    let GeneratedCallLinkSidecarAttempt {
        window,
        code_block,
        fallback,
        owner,
        opcode,
        instruction,
    } = attempt;
    let bytecode_index = instruction.bytecode_index;
    if candidate_table.owner() != owner {
        return Ok(None);
    }

    let candidates = candidate_table
        .candidates_for_bytecode_index(bytecode_index.offset())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        probe_miss_records.push(call_link_probe_miss_record(
            owner,
            bytecode_index,
            None,
            GeneratedCallLinkProbeMissReason::CandidateNotFound,
        ));
        return Ok(None);
    }

    let operands =
        generated_call_link_sidecar_operands(execution, code_block, window, instruction, fallback)?;
    if let Some(direct_call) = generated_call_link_hot_slot_direct_call(
        direct_call_hot_slots,
        &candidates,
        owner,
        opcode,
        bytecode_index,
        operands,
    ) {
        *direct_call_hot_slot_hits = direct_call_hot_slot_hits.saturating_add(1);
        return Ok(Some(direct_call));
    }

    let candidates_to_probe = match operands.callee_object {
        Some(callee_object) => {
            let matching = candidates
                .iter()
                .copied()
                .filter(|candidate| candidate.target.callee == callee_object)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                probe_miss_records.push(call_link_probe_miss_record(
                    owner,
                    bytecode_index,
                    candidates.first().copied(),
                    GeneratedCallLinkProbeMissReason::CalleeMismatch,
                ));
                return Ok(None);
            }
            matching
        }
        None => candidates,
    };
    for candidate in candidates_to_probe {
        let request = GeneratedCallLinkProbeRequest::new(
            candidate,
            owner,
            opcode,
            bytecode_index.offset(),
            operands.argument_count_including_this,
            operands.callee_value,
            operands.callee_value_kind,
            operands.callee_object,
            operands.this_value,
            operands.this_value_kind,
            operands.this_object,
        );
        match DispatchHost::probe_generated_call_link(dispatch_host, execution.heap, request) {
            GeneratedCallLinkProbeResult::DirectCall(authorization) => {
                let Some(callee_object) = operands.callee_object else {
                    probe_miss_records.push(call_link_probe_miss_record(
                        owner,
                        bytecode_index,
                        Some(candidate),
                        GeneratedCallLinkProbeMissReason::MissingCalleeIdentity,
                    ));
                    continue;
                };
                return Ok(Some(BaselineGeneratedJsDirectCall {
                    candidate: candidate.clone(),
                    authorization,
                    callee_value: operands.callee_value,
                    callee_object,
                    this_value: operands.this_value,
                    this_object: operands.this_object,
                    argument_count_including_this: operands.argument_count_including_this,
                }));
            }
            GeneratedCallLinkProbeResult::Blocked(blocked) => {
                probe_blocked_records
                    .push(call_link_probe_blocked_record(candidate, blocked.reason));
            }
            GeneratedCallLinkProbeResult::Miss(miss) => {
                probe_miss_records.push(call_link_probe_miss_record(
                    owner,
                    bytecode_index,
                    Some(candidate),
                    miss.reason,
                ));
            }
        }
    }

    Ok(None)
}

fn generated_call_link_hot_slot_direct_call(
    hot_slots: &[BaselineGeneratedJsDirectCallHotSlot],
    current_candidates: &[&GeneratedCallLinkCandidate],
    owner: CodeBlockId,
    opcode: CoreOpcode,
    bytecode_index: BytecodeIndex,
    operands: GeneratedCallLinkSidecarOperands,
) -> Option<BaselineGeneratedJsDirectCall> {
    let callee_object = operands.callee_object?;
    hot_slots
        .iter()
        .find(|slot| {
            slot.candidate.owner == owner
                && slot.candidate.opcode == opcode
                && slot.candidate.bytecode_index == bytecode_index.offset()
                && slot.candidate.direct_call_status
                    == GeneratedCallLinkDirectCallStatus::Authorized
                && current_candidates
                    .iter()
                    .any(|candidate| *candidate == &slot.candidate)
                && slot.candidate.target.callee == callee_object
                && slot.callee_object == callee_object
                && slot.argument_count_including_this == operands.argument_count_including_this
        })
        .map(|slot| BaselineGeneratedJsDirectCall {
            candidate: slot.candidate.clone(),
            authorization: slot.authorization,
            callee_value: operands.callee_value,
            callee_object,
            this_value: operands.this_value,
            this_object: operands.this_object,
            argument_count_including_this: operands.argument_count_including_this,
        })
}

fn generated_call_link_sidecar_operands(
    execution: &mut InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<GeneratedCallLinkSidecarOperands, BaselineInstructionAbort> {
    let opcode = CoreOpcode::from_opcode(instruction.opcode);
    let _destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let callee_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let callee_value =
        read_register_or_outcome(execution, code_block, window, callee_register, fallback)?;
    let callee_value_kind = callee_value.kind();
    let callee_object = object_id_for_runtime_value(execution, callee_value);

    let (this_value, provided_argument_count) = match opcode {
        Some(CoreOpcode::CallWithThis) => {
            let this_register = register_operand_or_fallback(instruction, 2, fallback)?;
            let this_value =
                read_register_or_outcome(execution, code_block, window, this_register, fallback)?;
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 3, fallback)?;
            (this_value, argument_count)
        }
        _ => {
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 2, fallback)?;
            (RuntimeValue::undefined(), argument_count)
        }
    };
    let this_value_kind = this_value.kind();
    let this_object = object_id_for_runtime_value(execution, this_value);

    Ok(GeneratedCallLinkSidecarOperands {
        argument_count_including_this: provided_argument_count.saturating_add(1),
        callee_value,
        callee_value_kind,
        callee_object,
        this_value,
        this_value_kind,
        this_object,
    })
}

fn object_id_for_runtime_value(
    execution: &InterpreterExecutionState<'_>,
    value: RuntimeValue,
) -> Option<ObjectId> {
    let payload = value.as_cell()?.pointer_payload_bits();
    execution.heap.cell_for_payload(payload).map(ObjectId)
}

fn call_link_probe_miss_record(
    owner: CodeBlockId,
    bytecode_index: BytecodeIndex,
    candidate: Option<&GeneratedCallLinkCandidate>,
    reason: GeneratedCallLinkProbeMissReason,
) -> BaselineGeneratedCallLinkProbeMissRecord {
    let (
        slot,
        attachment_ordinal,
        attachment_plan_ordinal,
        install_recheck_ordinal,
        boundary_validation_ordinal,
        descriptor_ordinal,
        observation_ordinal,
        readiness_ordinal,
        target_executable,
        target_callee,
        target_code_block,
        target_boundary,
        direct_call_status,
    ) = match candidate {
        Some(candidate) => (
            Some(candidate.slot),
            Some(candidate.attachment_ordinal),
            Some(candidate.attachment_plan_ordinal),
            Some(candidate.install_recheck_ordinal),
            candidate.boundary_validation_ordinal,
            candidate.descriptor_ordinal,
            candidate.observation_ordinal,
            candidate.readiness_ordinal,
            Some(candidate.target.executable),
            Some(candidate.target.callee),
            Some(candidate.target.target_code_block),
            Some(candidate.boundary.id),
            Some(candidate.direct_call_status),
        ),
        None => (
            None, None, None, None, None, None, None, None, None, None, None, None, None,
        ),
    };

    BaselineGeneratedCallLinkProbeMissRecord {
        owner,
        bytecode_index,
        slot,
        attachment_ordinal,
        attachment_plan_ordinal,
        install_recheck_ordinal,
        boundary_validation_ordinal,
        descriptor_ordinal,
        observation_ordinal,
        readiness_ordinal,
        target_executable,
        target_callee,
        target_code_block,
        target_boundary,
        direct_call_status,
        reason,
    }
}

fn call_link_probe_blocked_record(
    candidate: &GeneratedCallLinkCandidate,
    reason: GeneratedCallLinkProbeMissReason,
) -> BaselineGeneratedCallLinkProbeBlockedRecord {
    BaselineGeneratedCallLinkProbeBlockedRecord {
        owner: candidate.owner,
        bytecode_index: BytecodeIndex::from_offset(candidate.bytecode_index),
        slot: candidate.slot,
        attachment_ordinal: candidate.attachment_ordinal,
        attachment_plan_ordinal: candidate.attachment_plan_ordinal,
        install_recheck_ordinal: candidate.install_recheck_ordinal,
        boundary_validation_ordinal: candidate.boundary_validation_ordinal,
        descriptor_ordinal: candidate.descriptor_ordinal,
        observation_ordinal: candidate.observation_ordinal,
        readiness_ordinal: candidate.readiness_ordinal,
        target_executable: candidate.target.executable,
        target_callee: candidate.target.callee,
        target_code_block: candidate.target.target_code_block,
        target_boundary: candidate.boundary.id,
        direct_call_status: candidate.direct_call_status,
        reason,
    }
}

struct PropertyLoadSidecarAttempt<'code, 'instruction, 'site> {
    window: RegisterWindow,
    code_block: &'code CodeBlock,
    fallback: BaselineGeneratedFallbackSite,
    frame: CallFrameId,
    instruction: DecodedInstruction<'instruction>,
    site: &'site BaselineGeneratedPropertyHandoffSite,
}

fn named_property_sidecar_cache_key(
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Option<CacheKey> {
    match site.property_key {
        PropertyCacheKey::Key(property_key) => Some(CacheKey::Property(property_key)),
        PropertyCacheKey::None | PropertyCacheKey::RuntimeValue(_) => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PropertyIncrementSidecarShape<'code> {
    to_number_destination: VirtualRegister,
    post_update_result_destination: Option<VirtualRegister>,
    one_destination: VirtualRegister,
    add_destination: VirtualRegister,
    put_instruction: DecodedInstruction<'code>,
    put_site: BaselineGeneratedPropertyHandoffSite,
    resume_after_put: Option<BytecodeIndex>,
}

fn execute_property_load_or_increment_sidecar_candidate(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: PropertyLoadSidecarAttempt<'_, '_, '_>,
    owner: CodeBlockId,
    property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'_>>,
    increment_numeric_mode: BaselineGeneratedPropertyIncrementNumericMode,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let shape = property_increment_sidecar_shape_after_get_by_name(
        owner,
        attempt.code_block,
        attempt.instruction,
        attempt.site,
        property_handoff_plan,
    );
    let window = attempt.window;
    let code_block = attempt.code_block;
    let fallback = attempt.fallback;
    let load_destination = register_operand_or_fallback(attempt.instruction, 0, fallback)?;
    let Some(load_outcome) = execute_property_load_sidecar_candidate(sidecars, execution, attempt)?
    else {
        return Ok(None);
    };
    if !matches!(load_outcome, BaselineInstructionOutcome::Continue) {
        return Ok(Some(load_outcome));
    }

    let Some(shape) = shape else {
        return Ok(Some(BaselineInstructionOutcome::Continue));
    };
    let old_value =
        read_register_or_outcome(execution, code_block, window, load_destination, fallback)?;
    if old_value.as_number().is_none() {
        return Ok(Some(BaselineInstructionOutcome::Continue));
    }

    let to_number_fallback = fallback_site(owner, fallback.request.frame, shape.put_instruction);
    write_register_or_outcome(
        execution,
        window,
        shape.to_number_destination,
        old_value,
        to_number_fallback,
    )?;
    if let Some(destination) = shape.post_update_result_destination {
        write_register_or_outcome(
            execution,
            window,
            destination,
            old_value,
            to_number_fallback,
        )?;
    }
    write_register_or_outcome(
        execution,
        window,
        shape.one_destination,
        RuntimeValue::from_i32(1),
        to_number_fallback,
    )?;
    let left_value = read_register_or_outcome(
        execution,
        code_block,
        window,
        shape.to_number_destination,
        to_number_fallback,
    )?;
    let right_value = read_register_or_outcome(
        execution,
        code_block,
        window,
        shape.one_destination,
        to_number_fallback,
    )?;
    let (Some(left_number), Some(right_number)) = (left_value.as_number(), right_value.as_number())
    else {
        return Ok(Some(BaselineInstructionOutcome::Continue));
    };
    let updated_value = match increment_numeric_mode {
        BaselineGeneratedPropertyIncrementNumericMode::AnyNumber => {
            let Some(updated_value) =
                pure_number_arithmetic_result(CoreOpcode::AddInt32, left_number, right_number)
            else {
                return Ok(Some(BaselineInstructionOutcome::Continue));
            };
            updated_value
        }
        BaselineGeneratedPropertyIncrementNumericMode::Int32NoOverflow => {
            let (NumberValue::Int32(left), NumberValue::Int32(right)) = (left_number, right_number)
            else {
                return Ok(Some(BaselineInstructionOutcome::Continue));
            };
            let Some(updated_value) = left.checked_add(right) else {
                return Ok(Some(BaselineInstructionOutcome::Continue));
            };
            RuntimeValue::from_i32(updated_value)
        }
    };
    write_register_or_outcome(
        execution,
        window,
        shape.add_destination,
        updated_value,
        to_number_fallback,
    )?;

    let put_fallback = fallback_site(owner, fallback.request.frame, shape.put_instruction);
    if let Some(resume_after_put) = shape.resume_after_put {
        match execute_property_store_sidecar_candidate(
            sidecars,
            execution,
            PropertyStoreSidecarAttempt {
                window,
                code_block,
                fallback: put_fallback,
                instruction: shape.put_instruction,
                site: &shape.put_site,
            },
        )? {
            Some(BaselineInstructionOutcome::Continue) => {
                return Ok(Some(BaselineInstructionOutcome::ContinueTo(
                    resume_after_put,
                )));
            }
            Some(other) => return Ok(Some(other)),
            None => {}
        }
    }

    baseline_generated_property_handoff(
        owner,
        fallback.request.frame,
        shape.put_instruction,
        &shape.put_site,
    )
    .map(BaselineInstructionOutcome::Property)
    .map(Some)
    .map_err(|error| {
        BaselineInstructionAbort::Error(BaselineGeneratedExecutionError::PropertyHandoff {
            bytecode_index: shape.put_instruction.bytecode_index,
            opcode: fallback_opcode(shape.put_instruction.opcode),
            error,
        })
    })
}

fn property_increment_sidecar_shape_after_get_by_name<'code>(
    owner: CodeBlockId,
    code_block: &'code CodeBlock,
    get_instruction: DecodedInstruction<'code>,
    get_site: &BaselineGeneratedPropertyHandoffSite,
    property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'_>>,
) -> Option<PropertyIncrementSidecarShape<'code>> {
    if get_site.opcode != CoreOpcode::GetByName
        || CoreOpcode::from_opcode(get_instruction.opcode) != Some(CoreOpcode::GetByName)
    {
        return None;
    }
    let get_destination = get_instruction.register_operand(0).ok()?;
    let get_base = get_instruction.register_operand(1).ok()?;
    let get_identifier = identifier_index_operand(get_instruction, 2)?;
    let get_key = named_property_sidecar_cache_key(get_site)?;

    let mut cursor = get_instruction.bytecode_index.offset().checked_add(1)?;
    let to_number = decoded_core_instruction_at(code_block, cursor, CoreOpcode::ToNumber)?;
    let to_number_destination = to_number.register_operand(0).ok()?;
    if to_number.register_operand(1).ok()? != get_destination {
        return None;
    }
    cursor = cursor.checked_add(1)?;

    let mut post_update_result_destination = None;
    if let Some(move_instruction) =
        decoded_core_instruction_at(code_block, cursor, CoreOpcode::Move)
    {
        if move_instruction.register_operand(1).ok()? != to_number_destination {
            return None;
        }
        post_update_result_destination = Some(move_instruction.register_operand(0).ok()?);
        cursor = cursor.checked_add(1)?;
    }

    let load_one = decoded_core_instruction_at(code_block, cursor, CoreOpcode::LoadInt32)?;
    let one_destination = load_one.register_operand(0).ok()?;
    if load_one.signed_immediate_operand(1).ok()? != 1 {
        return None;
    }
    cursor = cursor.checked_add(1)?;

    let add = decoded_core_instruction_at(code_block, cursor, CoreOpcode::AddInt32)?;
    let add_destination = add.register_operand(0).ok()?;
    if add.register_operand(1).ok()? != to_number_destination
        || add.register_operand(2).ok()? != one_destination
    {
        return None;
    }
    cursor = cursor.checked_add(1)?;

    let put = decoded_core_instruction_at(code_block, cursor, CoreOpcode::PutByName)?;
    if put.register_operand(0).ok()? != get_base
        || identifier_index_operand(put, 1)? != get_identifier
        || put.register_operand(2).ok()? != add_destination
    {
        return None;
    }
    let put_site =
        property_handoff_site_for_instruction(owner, code_block, put, property_handoff_plan)
            .ok()?;
    if put_site.opcode != CoreOpcode::PutByName
        || named_property_sidecar_cache_key(&put_site)? != get_key
    {
        return None;
    }

    Some(PropertyIncrementSidecarShape {
        to_number_destination,
        post_update_result_destination,
        one_destination,
        add_destination,
        put_instruction: put,
        put_site,
        resume_after_put: next_baseline_generated_bytecode_index(code_block, put),
    })
}

fn decoded_core_instruction_at(
    code_block: &CodeBlock,
    offset: u32,
    opcode: CoreOpcode,
) -> Option<DecodedInstruction<'_>> {
    let instruction = code_block
        .decoded_instruction_at(BytecodeIndex::from_offset(offset))
        .ok()?;
    (CoreOpcode::from_opcode(instruction.opcode) == Some(opcode)).then_some(instruction)
}

fn identifier_index_operand(instruction: DecodedInstruction<'_>, index: usize) -> Option<u32> {
    match instruction.operand(index).ok()? {
        Operand::IdentifierIndex(identifier_index) => Some(identifier_index),
        _ => None,
    }
}

fn element_sidecar_cache_key_from_runtime_value(value: RuntimeValue) -> Option<CacheKey> {
    let NumberValue::Int32(index) = value.as_number()? else {
        return None;
    };
    let index = u32::try_from(index).ok()?;
    Some(CacheKey::Property(PropertyKey::from_index(
        PropertyIndex::from_canonical_index(index),
    )))
}

fn execute_property_load_sidecar_candidate(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: PropertyLoadSidecarAttempt<'_, '_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let PropertyLoadSidecarAttempt {
        window,
        code_block,
        fallback,
        frame,
        instruction,
        site,
    } = attempt;

    let mut operands = None;
    let BaselineGeneratedPropertyExecutionSidecars {
        property_load_plan_table,
        property_load_guarded_candidate_table,
        property_load_megamorphic_candidate_table,
        dispatch_host,
        destination_root_sync_requests,
        property_load_probe_miss_records,
        guarded_property_load_probe_miss_records,
        ..
    } = sidecars;

    let mut current_base_structure = None;
    if let Some(megamorphic_table) = *property_load_megamorphic_candidate_table {
        if megamorphic_table.owner() == site.owner && site.opcode == CoreOpcode::GetByName {
            let Some(site_key) = named_property_sidecar_cache_key(site) else {
                return Ok(None);
            };
            if megamorphic_table.contains_site(site.slot, site.bytecode_index.offset(), site_key) {
                if dispatch_host.has_pending_structure_chain_invalidation_events() {
                    return Ok(None);
                }
                let (destination, base, _) = property_load_sidecar_operands(
                    &mut operands,
                    execution,
                    code_block,
                    window,
                    instruction,
                    fallback,
                )?;
                let actual_structure = *current_base_structure.get_or_insert_with(|| {
                    dispatch_host.generated_property_sidecar_base_structure(base)
                });
                let lookup = match actual_structure {
                    Some(actual_structure) => megamorphic_table.lookup(
                        site.slot,
                        site.bytecode_index.offset(),
                        site_key,
                        actual_structure,
                    ),
                    None => return Ok(None),
                };
                match lookup {
                    GeneratedPropertyLoadMegamorphicLookup::NoSite => {}
                    GeneratedPropertyLoadMegamorphicLookup::Miss => return Ok(None),
                    GeneratedPropertyLoadMegamorphicLookup::Missing => {
                        let outcome = write_register_or_outcome(
                            execution,
                            window,
                            destination,
                            RuntimeValue::undefined(),
                            fallback,
                        )?;
                        return Ok(Some(outcome));
                    }
                    GeneratedPropertyLoadMegamorphicLookup::PrototypeData {
                        key,
                        base_structure,
                        holder,
                        offset,
                    } => {
                        let result = dispatch_host
                            .probe_generated_property_load_megamorphic_holder(
                                GeneratedPropertyLoadMegamorphicHolderProbeRequest {
                                    key,
                                    base_structure,
                                    holder,
                                    offset,
                                },
                            );
                        let hit = match result {
                            GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
                            GeneratedPropertyLoadProbeResult::Miss(miss) => {
                                property_load_probe_miss_records.push(
                                    BaselineGeneratedPropertyLoadProbeMissRecord {
                                        owner: site.owner,
                                        bytecode_index: site.bytecode_index,
                                        key,
                                        base_structure: Some(base_structure),
                                        offset: Some(offset),
                                        reason: miss.reason,
                                    },
                                );
                                return Ok(None);
                            }
                        };

                        let outcome = write_register_or_outcome(
                            execution,
                            window,
                            destination,
                            hit.value,
                            fallback,
                        )?;
                        if hit.destination_root_sync.requires_targeted_register_sync() {
                            destination_root_sync_requests.push(
                                BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                                    frame,
                                    bytecode_index: site.bytecode_index,
                                    destination,
                                },
                            );
                        }
                        return Ok(Some(outcome));
                    }
                    GeneratedPropertyLoadMegamorphicLookup::Hit(plan) => {
                        let result = dispatch_host.probe_generated_property_load(
                            GeneratedPropertyLoadProbeRequest { plan: &plan, base },
                        );
                        let hit = match result {
                            GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
                            GeneratedPropertyLoadProbeResult::Miss(miss) => {
                                property_load_probe_miss_records.push(
                                    BaselineGeneratedPropertyLoadProbeMissRecord {
                                        owner: plan.owner,
                                        bytecode_index: BytecodeIndex::from_offset(
                                            plan.bytecode_index,
                                        ),
                                        key: plan.key,
                                        base_structure: plan.access_case.base_structure,
                                        offset: plan.access_case.offset,
                                        reason: miss.reason,
                                    },
                                );
                                return Ok(None);
                            }
                        };

                        let outcome = write_register_or_outcome(
                            execution,
                            window,
                            destination,
                            hit.value,
                            fallback,
                        )?;
                        if hit.destination_root_sync.requires_targeted_register_sync() {
                            destination_root_sync_requests.push(
                                BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                                    frame,
                                    bytecode_index: site.bytecode_index,
                                    destination,
                                },
                            );
                        }
                        return Ok(Some(outcome));
                    }
                }
            }
        }
    }

    if let Some(plan_table) = *property_load_plan_table {
        if plan_table.owner() == site.owner {
            for plan in
                plan_table.candidates_for_bytecode_index_newest_first(site.bytecode_index.offset())
            {
                let (destination, base, runtime_key) = property_load_sidecar_operands(
                    &mut operands,
                    execution,
                    code_block,
                    window,
                    instruction,
                    fallback,
                )?;
                let probe_plan;
                let plan = match site.opcode {
                    CoreOpcode::GetByName
                    | CoreOpcode::GetGlobalObjectProperty
                    | CoreOpcode::GetLength => {
                        let Some(site_key) = named_property_sidecar_cache_key(site) else {
                            return Ok(None);
                        };
                        if plan.plan_kind != PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad
                            || plan.key != site_key
                        {
                            continue;
                        }
                        plan
                    }
                    CoreOpcode::GetByValue => {
                        if plan.plan_kind != PropertyLoadAccessCasePlanKind::DataOnlyIndexedLoad {
                            continue;
                        }
                        let Some(runtime_key) = runtime_key else {
                            return Ok(None);
                        };
                        probe_plan = property_load_plan_with_runtime_key(plan, runtime_key);
                        &probe_plan
                    }
                    _ => return Ok(None),
                };

                if property_load_sidecar_structure_guard_misses(
                    &mut **dispatch_host,
                    &mut current_base_structure,
                    base,
                    plan,
                ) {
                    continue;
                }

                let result = dispatch_host.probe_generated_property_load(
                    GeneratedPropertyLoadProbeRequest { plan, base },
                );
                let hit = match result {
                    GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
                    GeneratedPropertyLoadProbeResult::Miss(miss) => {
                        property_load_probe_miss_records.push(
                            BaselineGeneratedPropertyLoadProbeMissRecord {
                                owner: plan.owner,
                                bytecode_index: BytecodeIndex::from_offset(plan.bytecode_index),
                                key: plan.key,
                                base_structure: plan.access_case.base_structure,
                                offset: plan.access_case.offset,
                                reason: miss.reason,
                            },
                        );
                        continue;
                    }
                };

                let outcome =
                    write_register_or_outcome(execution, window, destination, hit.value, fallback)?;
                if hit.destination_root_sync.requires_targeted_register_sync() {
                    destination_root_sync_requests.push(
                        BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                            frame,
                            bytecode_index: site.bytecode_index,
                            destination,
                        },
                    );
                }
                return Ok(Some(outcome));
            }
        }
    }

    if let Some(guarded_candidate_table) = *property_load_guarded_candidate_table {
        if guarded_candidate_table.owner() == site.owner {
            for candidate in
                guarded_candidate_table.candidates_for_bytecode_index(site.bytecode_index.offset())
            {
                if !matches!(site.opcode, CoreOpcode::GetByName | CoreOpcode::GetLength) {
                    continue;
                }
                let Some(site_key) = named_property_sidecar_cache_key(site) else {
                    return Ok(None);
                };
                let plan = &candidate.plan;
                if plan.descriptor.key != site_key {
                    continue;
                }

                let (destination, base, _) = property_load_sidecar_operands(
                    &mut operands,
                    execution,
                    code_block,
                    window,
                    instruction,
                    fallback,
                )?;

                let result = dispatch_host.probe_generated_guarded_property_load(
                    GeneratedGuardedPropertyLoadProbeRequest::new(plan, base),
                );
                let hit = match result {
                    GeneratedGuardedPropertyLoadProbeResult::Hit(hit) => hit,
                    GeneratedGuardedPropertyLoadProbeResult::Miss(miss) => {
                        guarded_property_load_probe_miss_records.push(
                            BaselineGeneratedGuardedPropertyLoadProbeMissRecord {
                                owner: plan.owner,
                                bytecode_index: BytecodeIndex::from_offset(plan.bytecode_index),
                                slot: plan.slot,
                                guard_plan_ordinal: candidate.guard_plan_ordinal,
                                materialization_ordinal: candidate.materialization_ordinal,
                                dependency_ordinals: candidate.dependency_ordinals.clone(),
                                binding_set_ids: candidate.binding_set_ids.clone(),
                                candidate_kind: candidate.candidate_kind,
                                base_structure: plan.descriptor.base_structure,
                                reason: miss.reason,
                                requirement: miss.requirement,
                                key: miss.key,
                                prototype_depth: miss.prototype_depth,
                                chain_index: miss.chain_index,
                                outcome: miss.outcome,
                            },
                        );
                        continue;
                    }
                };

                let outcome =
                    write_register_or_outcome(execution, window, destination, hit.value, fallback)?;
                if hit.destination_root_sync.requires_targeted_register_sync() {
                    destination_root_sync_requests.push(
                        BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                            frame,
                            bytecode_index: site.bytecode_index,
                            destination,
                        },
                    );
                }
                return Ok(Some(outcome));
            }
        }
    }

    Ok(None)
}

fn property_load_sidecar_structure_guard_misses(
    dispatch_host: &mut dyn DispatchHost,
    current_base_structure: &mut Option<Option<StructureId>>,
    base: RuntimeValue,
    plan: &PropertyLoadAccessCasePlan,
) -> bool {
    if plan.plan_kind != PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad {
        return false;
    }
    let Some(expected_structure) = plan.access_case.base_structure else {
        return false;
    };
    if expected_structure == StructureId::INVALID {
        return false;
    }
    let actual_structure = *current_base_structure
        .get_or_insert_with(|| dispatch_host.generated_property_sidecar_base_structure(base));
    matches!(actual_structure, Some(actual_structure) if actual_structure != expected_structure)
}

fn property_load_plan_with_runtime_key(
    plan: &crate::jit::PropertyLoadAccessCasePlan,
    runtime_key: CacheKey,
) -> crate::jit::PropertyLoadAccessCasePlan {
    let mut plan = plan.clone();
    plan.key = runtime_key;
    plan.access_case.key = runtime_key;
    plan
}

fn property_load_sidecar_operands(
    operands: &mut Option<(VirtualRegister, RuntimeValue, Option<CacheKey>)>,
    execution: &mut InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<(VirtualRegister, RuntimeValue, Option<CacheKey>), BaselineInstructionAbort> {
    if let Some(operands) = *operands {
        return Ok(operands);
    }

    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let opcode = CoreOpcode::from_opcode(instruction.opcode);
    let (base, runtime_key) = match opcode {
        Some(CoreOpcode::GetGlobalObjectProperty) => (
            execution
                .stack
                .active_global_this_value()
                .map_err(execution_error_abort)?,
            None,
        ),
        Some(CoreOpcode::GetByValue) => {
            let base_register = register_operand_or_fallback(instruction, 1, fallback)?;
            let base =
                read_register_or_outcome(execution, code_block, window, base_register, fallback)?;
            let key_register = register_operand_or_fallback(instruction, 2, fallback)?;
            let key_value =
                read_register_or_outcome(execution, code_block, window, key_register, fallback)?;
            (
                base,
                element_sidecar_cache_key_from_runtime_value(key_value),
            )
        }
        _ => {
            let base_register = register_operand_or_fallback(instruction, 1, fallback)?;
            (
                read_register_or_outcome(execution, code_block, window, base_register, fallback)?,
                None,
            )
        }
    };
    let decoded_operands = (destination, base, runtime_key);
    *operands = Some(decoded_operands);
    Ok(decoded_operands)
}

fn next_baseline_generated_bytecode_index(
    code_block: &CodeBlock,
    instruction: DecodedInstruction<'_>,
) -> Option<BytecodeIndex> {
    let next = (instruction.bytecode_index.offset() as usize).saturating_add(1);
    (next < code_block.unlinked().instructions().instruction_count())
        .then(|| BytecodeIndex::from_offset(next as u32))
}

struct PropertyHasSidecarAttempt<'code, 'instruction, 'site> {
    window: RegisterWindow,
    code_block: &'code CodeBlock,
    fallback: BaselineGeneratedFallbackSite,
    instruction: DecodedInstruction<'instruction>,
    site: &'site BaselineGeneratedPropertyHandoffSite,
}

fn execute_property_has_sidecar_candidate(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: PropertyHasSidecarAttempt<'_, '_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let PropertyHasSidecarAttempt {
        window,
        code_block,
        fallback,
        instruction,
        site,
    } = attempt;

    let BaselineGeneratedPropertyExecutionSidecars {
        property_has_megamorphic_candidate_table,
        dispatch_host,
        ..
    } = sidecars;
    let Some(megamorphic_table) = *property_has_megamorphic_candidate_table else {
        return Ok(None);
    };
    if megamorphic_table.owner() != site.owner {
        return Ok(None);
    }
    let site_key = match site.opcode {
        CoreOpcode::InById => {
            let Some(site_key) = named_property_sidecar_cache_key(site) else {
                return Ok(None);
            };
            site_key
        }
        CoreOpcode::InByVal => {
            let property_register = register_operand_or_fallback(instruction, 2, fallback)?;
            if !matches!(
                site.property_key,
                PropertyCacheKey::RuntimeValue(expected) if expected == property_register
            ) {
                return Ok(None);
            }
            let property = read_register_or_outcome(
                execution,
                code_block,
                window,
                property_register,
                fallback,
            )?;
            let Some(runtime_key) =
                dispatch_host.generated_property_has_sidecar_runtime_key(property)
            else {
                return Ok(None);
            };
            runtime_key
        }
        _ => return Ok(None),
    };
    if !megamorphic_table.contains_site(site.slot, site.bytecode_index.offset(), site_key) {
        return Ok(None);
    }
    if dispatch_host.has_pending_structure_chain_invalidation_events() {
        return Ok(None);
    }

    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let base_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let base = read_register_or_outcome(execution, code_block, window, base_register, fallback)?;
    let Some(actual_structure) = dispatch_host.generated_property_has_sidecar_base_structure(base)
    else {
        return Ok(None);
    };
    let result = match megamorphic_table.lookup(
        site.slot,
        site.bytecode_index.offset(),
        site_key,
        actual_structure,
    ) {
        GeneratedPropertyHasMegamorphicLookup::Hit(result) => result,
        GeneratedPropertyHasMegamorphicLookup::NoSite
        | GeneratedPropertyHasMegamorphicLookup::Miss => return Ok(None),
    };

    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_bool(result),
        fallback,
    )
    .map(Some)
}

struct PropertyStoreSidecarAttempt<'code, 'instruction, 'site> {
    window: RegisterWindow,
    code_block: &'code CodeBlock,
    fallback: BaselineGeneratedFallbackSite,
    instruction: DecodedInstruction<'instruction>,
    site: &'site BaselineGeneratedPropertyHandoffSite,
}

fn execute_property_store_sidecar_candidate(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: PropertyStoreSidecarAttempt<'_, '_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let PropertyStoreSidecarAttempt {
        window,
        code_block,
        fallback,
        instruction,
        site,
    } = attempt;

    let mut operands = None;
    let BaselineGeneratedPropertyExecutionSidecars {
        property_store_candidate_table,
        property_store_megamorphic_candidate_table,
        dispatch_host,
        property_store_probe_miss_records,
        property_store_mutation_rejection_records,
        property_store_mutation_commit_records,
        ..
    } = sidecars;
    let mut current_base_structure = None;
    if let Some(megamorphic_table) = *property_store_megamorphic_candidate_table {
        if megamorphic_table.owner() == site.owner {
            match site.opcode {
                CoreOpcode::PutByName | CoreOpcode::PutGlobalObjectProperty => {
                    let Some(site_key) = named_property_sidecar_cache_key(site) else {
                        return Ok(None);
                    };
                    let (base, stored_value, _) = property_store_sidecar_operands(
                        &mut operands,
                        execution,
                        code_block,
                        window,
                        instruction,
                        fallback,
                    )?;
                    let Some(base_structure) = *current_base_structure.get_or_insert_with(|| {
                        dispatch_host.generated_property_sidecar_base_structure(base)
                    }) else {
                        return Ok(None);
                    };
                    match megamorphic_table.lookup(
                        site.slot,
                        site.bytecode_index.offset(),
                        site_key,
                        base_structure,
                    ) {
                        GeneratedPropertyStoreMegamorphicLookup::NoSite => {}
                        GeneratedPropertyStoreMegamorphicLookup::Miss => return Ok(None),
                        GeneratedPropertyStoreMegamorphicLookup::Hit(plan) => {
                            if site.opcode == CoreOpcode::PutGlobalObjectProperty
                                && plan.plan_kind
                                    != PropertyStoreAccessCasePlanKind::DataOnlyReplace
                            {
                                return Ok(None);
                            }
                            let result = dispatch_host.probe_generated_property_store(
                                GeneratedPropertyStoreProbeRequest::new(&plan, base, stored_value),
                            );
                            let hit = match result {
                                GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
                                GeneratedPropertyStoreProbeResult::Miss(_) => return Ok(None),
                            };
                            let request = GeneratedPropertyStoreMutationRequest::new(base, hit);
                            return match dispatch_host
                                .commit_generated_property_store(execution.heap, request)
                            {
                                GeneratedPropertyStoreMutationResult::Committed(_) => {
                                    Ok(Some(BaselineInstructionOutcome::Continue))
                                }
                                GeneratedPropertyStoreMutationResult::Rejected(_) => Ok(None),
                            };
                        }
                    }
                }
                CoreOpcode::PutByValue => {}
                _ => return Ok(None),
            }
        }
    }

    let Some(candidate_table) = *property_store_candidate_table else {
        return Ok(None);
    };

    if candidate_table.owner() != site.owner {
        return Ok(None);
    }

    for candidate in
        candidate_table.candidates_for_bytecode_index_newest_first(site.bytecode_index.offset())
    {
        let plan = &candidate.plan;
        let (base, stored_value, runtime_key) = property_store_sidecar_operands(
            &mut operands,
            execution,
            code_block,
            window,
            instruction,
            fallback,
        )?;
        let probe_plan;
        let plan = match site.opcode {
            CoreOpcode::PutByName | CoreOpcode::PutGlobalObjectProperty => {
                let Some(site_key) = named_property_sidecar_cache_key(site) else {
                    return Ok(None);
                };
                if plan.plan_kind == PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore
                    || plan.key != site_key
                    || (site.opcode == CoreOpcode::PutGlobalObjectProperty
                        && plan.plan_kind != PropertyStoreAccessCasePlanKind::DataOnlyReplace)
                {
                    continue;
                }
                plan
            }
            CoreOpcode::PutByValue => {
                if plan.plan_kind != PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore {
                    continue;
                }
                let Some(runtime_key) = runtime_key else {
                    return Ok(None);
                };
                probe_plan = property_store_plan_with_runtime_key(plan, runtime_key);
                &probe_plan
            }
            _ => return Ok(None),
        };

        if property_store_sidecar_structure_guard_misses(
            &mut **dispatch_host,
            &mut current_base_structure,
            base,
            plan,
        ) {
            continue;
        }

        let result = dispatch_host.probe_generated_property_store(
            GeneratedPropertyStoreProbeRequest::new(plan, base, stored_value),
        );
        let hit = match result {
            GeneratedPropertyStoreProbeResult::Hit(hit) => hit,
            GeneratedPropertyStoreProbeResult::Miss(miss) => {
                property_store_probe_miss_records
                    .push(property_store_probe_miss_record(candidate, miss.reason));
                continue;
            }
        };

        let request = GeneratedPropertyStoreMutationRequest::new(base, hit);
        match dispatch_host.commit_generated_property_store(execution.heap, request) {
            GeneratedPropertyStoreMutationResult::Committed(commit) => {
                property_store_mutation_commit_records
                    .push(property_store_mutation_commit_record(candidate, commit));
                return Ok(Some(BaselineInstructionOutcome::Continue));
            }
            GeneratedPropertyStoreMutationResult::Rejected(rejection) => {
                property_store_mutation_rejection_records.push(
                    property_store_mutation_rejection_record(candidate, rejection.reason),
                );
            }
        }
    }

    Ok(None)
}

fn property_store_sidecar_structure_guard_misses(
    dispatch_host: &mut dyn DispatchHost,
    current_base_structure: &mut Option<Option<StructureId>>,
    base: RuntimeValue,
    plan: &PropertyStoreAccessCasePlan,
) -> bool {
    if !matches!(
        plan.plan_kind,
        PropertyStoreAccessCasePlanKind::DataOnlyReplace
            | PropertyStoreAccessCasePlanKind::DataOnlyTransition
    ) {
        return false;
    }
    let Some(expected_structure) = plan.access_case.base_structure else {
        return false;
    };
    if expected_structure == StructureId::INVALID {
        return false;
    }
    let actual_structure = *current_base_structure
        .get_or_insert_with(|| dispatch_host.generated_property_sidecar_base_structure(base));
    matches!(actual_structure, Some(actual_structure) if actual_structure != expected_structure)
}

fn property_store_plan_with_runtime_key(
    plan: &crate::jit::PropertyStoreAccessCasePlan,
    runtime_key: CacheKey,
) -> crate::jit::PropertyStoreAccessCasePlan {
    let mut plan = plan.clone();
    plan.key = runtime_key;
    plan.access_case.key = runtime_key;
    plan
}

fn property_store_sidecar_operands(
    operands: &mut Option<(RuntimeValue, RuntimeValue, Option<CacheKey>)>,
    execution: &mut InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<(RuntimeValue, RuntimeValue, Option<CacheKey>), BaselineInstructionAbort> {
    if let Some(operands) = *operands {
        return Ok(operands);
    }

    let opcode = CoreOpcode::from_opcode(instruction.opcode);
    let (base, stored_value_register) = match opcode {
        Some(CoreOpcode::PutGlobalObjectProperty) => (
            execution
                .stack
                .active_global_this_value()
                .map_err(execution_error_abort)?,
            register_operand_or_fallback(instruction, 1, fallback)?,
        ),
        _ => {
            let base_register = register_operand_or_fallback(instruction, 0, fallback)?;
            (
                read_register_or_outcome(execution, code_block, window, base_register, fallback)?,
                register_operand_or_fallback(instruction, 2, fallback)?,
            )
        }
    };
    let stored_value = read_register_or_outcome(
        execution,
        code_block,
        window,
        stored_value_register,
        fallback,
    )?;
    let runtime_key = if opcode == Some(CoreOpcode::PutByValue) {
        let key_register = register_operand_or_fallback(instruction, 1, fallback)?;
        let key_value =
            read_register_or_outcome(execution, code_block, window, key_register, fallback)?;
        element_sidecar_cache_key_from_runtime_value(key_value)
    } else {
        None
    };
    let decoded_operands = (base, stored_value, runtime_key);
    *operands = Some(decoded_operands);
    Ok(decoded_operands)
}

fn property_store_probe_miss_record(
    candidate: &PropertyStoreMutationCandidate,
    reason: GeneratedPropertyStoreProbeMissReason,
) -> BaselineGeneratedPropertyStoreProbeMissRecord {
    let plan = &candidate.plan;
    BaselineGeneratedPropertyStoreProbeMissRecord {
        owner: plan.owner,
        bytecode_index: BytecodeIndex::from_offset(plan.bytecode_index),
        slot: plan.slot,
        key: plan.key,
        plan_kind: plan.plan_kind,
        base_structure: plan.access_case.base_structure,
        planned_new_structure: plan.access_case.new_structure,
        offset: plan.access_case.offset,
        store_plan_ordinal: candidate.store_plan_ordinal,
        readiness_ordinal: candidate.readiness_ordinal,
        stored_value_kind: candidate.stored_value_kind,
        reason,
    }
}

fn property_store_mutation_rejection_record(
    candidate: &PropertyStoreMutationCandidate,
    reason: GeneratedPropertyStoreMutationMissReason,
) -> BaselineGeneratedPropertyStoreMutationRejectionRecord {
    let plan = &candidate.plan;
    BaselineGeneratedPropertyStoreMutationRejectionRecord {
        owner: plan.owner,
        bytecode_index: BytecodeIndex::from_offset(plan.bytecode_index),
        slot: plan.slot,
        key: plan.key,
        plan_kind: plan.plan_kind,
        base_structure: plan.access_case.base_structure,
        planned_new_structure: plan.access_case.new_structure,
        offset: plan.access_case.offset,
        store_plan_ordinal: candidate.store_plan_ordinal,
        readiness_ordinal: candidate.readiness_ordinal,
        stored_value_kind: candidate.stored_value_kind,
        reason,
    }
}

fn property_store_mutation_commit_record(
    candidate: &PropertyStoreMutationCandidate,
    commit: GeneratedPropertyStoreMutationCommit,
) -> BaselineGeneratedPropertyStoreMutationCommitRecord {
    BaselineGeneratedPropertyStoreMutationCommitRecord {
        owner: candidate.plan.owner,
        bytecode_index: BytecodeIndex::from_offset(candidate.plan.bytecode_index),
        slot: candidate.plan.slot,
        key: candidate.plan.key,
        plan_kind: candidate.plan.plan_kind,
        store_plan_ordinal: candidate.store_plan_ordinal,
        readiness_ordinal: candidate.readiness_ordinal,
        stored_value_kind: candidate.stored_value_kind,
        commit,
    }
}

const fn baseline_generated_executor_supports_subset(
    subset: BaselineSupportedOpcodeSubset,
) -> bool {
    matches!(
        subset,
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
            | BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish
            | BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOr
            | BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish
            | BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullishFalse
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary
    )
}

const fn baseline_subset_uses_pure_number_binary(subset: BaselineSupportedOpcodeSubset) -> bool {
    matches!(
        subset,
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrimitiveNumericOperand {
    register: VirtualRegister,
    value: RuntimeValue,
}

fn execute_to_number(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let result = primitive_numeric_operand_or_fallback(
        execution,
        code_block,
        window,
        instruction,
        1,
        fallback,
    )?;
    write_register_or_outcome(execution, window, destination, result.value, fallback)
}

fn execute_void(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let source = register_operand_or_fallback(instruction, 1, fallback)?;
    let _ = read_register_or_outcome(execution, code_block, window, source, fallback)?;
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::undefined(),
        fallback,
    )
}

fn execute_int32_arithmetic(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = int32_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right = int32_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;
    let Some(result) = (match opcode {
        CoreOpcode::AddInt32 => left.checked_add(right),
        CoreOpcode::SubInt32 => left.checked_sub(right),
        CoreOpcode::MulInt32 => left.checked_mul(right),
        _ => None,
    }) else {
        return Ok(BaselineInstructionOutcome::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::Int32Overflow),
        ));
    };

    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_i32(result),
        fallback,
    )
}

fn execute_pure_number_arithmetic(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = number_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right =
        number_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;
    let result = pure_number_arithmetic_result(opcode, left, right).ok_or_else(|| {
        BaselineInstructionAbort::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
        )
    })?;
    write_register_or_outcome(execution, window, destination, result, fallback)
}

fn pure_number_arithmetic_result(
    opcode: CoreOpcode,
    left: NumberValue,
    right: NumberValue,
) -> Option<RuntimeValue> {
    if let (NumberValue::Int32(left), NumberValue::Int32(right)) = (left, right) {
        let checked = match opcode {
            CoreOpcode::AddInt32 => left.checked_add(right),
            CoreOpcode::SubInt32 => left.checked_sub(right),
            CoreOpcode::MulInt32 => left.checked_mul(right),
            _ => None,
        };
        if let Some(result) = checked {
            return Some(RuntimeValue::from_i32(result));
        }
        return pure_number_arithmetic_f64_result(opcode, f64::from(left), f64::from(right))
            .map(RuntimeValue::from_double);
    }

    pure_number_arithmetic_f64_result(opcode, number_to_f64(left), number_to_f64(right))
        .map(RuntimeValue::from_double)
}

fn pure_number_arithmetic_f64_result(opcode: CoreOpcode, left: f64, right: f64) -> Option<f64> {
    match opcode {
        CoreOpcode::AddInt32 => Some(left + right),
        CoreOpcode::SubInt32 => Some(left - right),
        CoreOpcode::MulInt32 => Some(left * right),
        _ => None,
    }
}

fn execute_negate_number(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let source = primitive_numeric_operand_or_fallback(
        execution,
        code_block,
        window,
        instruction,
        1,
        fallback,
    )?;
    let Some(source_number) = source.value.as_number() else {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedPrimitiveNumericCoercionOperand {
                operand_index: 1,
                register: source.register,
                value_kind: source.value.kind(),
            },
        )));
    };
    let result = match source_number {
        NumberValue::Int32(0) => RuntimeValue::from_double(-0.0),
        NumberValue::Int32(value) => match value.checked_neg() {
            Some(value) => RuntimeValue::from_i32(value),
            None => RuntimeValue::from_double(-(value as f64)),
        },
        NumberValue::DoubleBits(bits) => RuntimeValue::from_double(-bits.to_f64()),
    };
    write_register_or_outcome(execution, window, destination, result, fallback)
}

fn execute_number_arithmetic(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = number_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right =
        number_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;

    if let (CoreOpcode::ModNumber, NumberValue::Int32(left), NumberValue::Int32(right)) =
        (opcode, left, right)
    {
        if let Some(result) = left.checked_rem(right) {
            return write_register_or_outcome(
                execution,
                window,
                destination,
                RuntimeValue::from_i32(result),
                fallback,
            );
        }
    }

    let left = number_to_f64(left);
    let right = number_to_f64(right);
    let result = match opcode {
        CoreOpcode::DivNumber => left / right,
        CoreOpcode::ModNumber => left % right,
        _ => {
            return Ok(BaselineInstructionOutcome::Fallback(
                fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
            ));
        }
    };
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_double(result),
        fallback,
    )
}

fn execute_int32_bit_not(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let source = primitive_numeric_operand_or_fallback(
        execution,
        code_block,
        window,
        instruction,
        1,
        fallback,
    )?;
    let Some(source_number) = source.value.as_number() else {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedPrimitiveNumericCoercionOperand {
                operand_index: 1,
                register: source.register,
                value_kind: source.value.kind(),
            },
        )));
    };
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_i32(!number_to_int32(source_number)),
        fallback,
    )
}

fn execute_int32_bitwise(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = int32_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right = int32_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;
    let result = match opcode {
        CoreOpcode::BitOrInt32 => RuntimeValue::from_i32(left | right),
        CoreOpcode::BitXorInt32 => RuntimeValue::from_i32(left ^ right),
        CoreOpcode::BitAndInt32 => RuntimeValue::from_i32(left & right),
        CoreOpcode::LeftShiftInt32 => {
            RuntimeValue::from_i32(left.wrapping_shl((right & 0x1f) as u32))
        }
        CoreOpcode::RightShiftInt32 => {
            RuntimeValue::from_i32(left.wrapping_shr((right & 0x1f) as u32))
        }
        CoreOpcode::UnsignedRightShiftInt32 => {
            let value = (left as u32).wrapping_shr((right & 0x1f) as u32);
            if value <= i32::MAX as u32 {
                RuntimeValue::from_i32(value as i32)
            } else {
                RuntimeValue::from_double(f64::from(value))
            }
        }
        _ => {
            return Ok(BaselineInstructionOutcome::Fallback(
                fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
            ));
        }
    };
    write_register_or_outcome(execution, window, destination, result, fallback)
}

fn execute_pure_number_bitwise(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = number_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right =
        number_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;
    let result = int32_bitwise_result(opcode, number_to_int32(left), number_to_int32(right))
        .ok_or_else(|| {
            BaselineInstructionAbort::Fallback(
                fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
            )
        })?;
    write_register_or_outcome(execution, window, destination, result, fallback)
}

fn int32_bitwise_result(opcode: CoreOpcode, left: i32, right: i32) -> Option<RuntimeValue> {
    Some(match opcode {
        CoreOpcode::BitOrInt32 => RuntimeValue::from_i32(left | right),
        CoreOpcode::BitXorInt32 => RuntimeValue::from_i32(left ^ right),
        CoreOpcode::BitAndInt32 => RuntimeValue::from_i32(left & right),
        CoreOpcode::LeftShiftInt32 => {
            RuntimeValue::from_i32(left.wrapping_shl((right & 0x1f) as u32))
        }
        CoreOpcode::RightShiftInt32 => {
            RuntimeValue::from_i32(left.wrapping_shr((right & 0x1f) as u32))
        }
        CoreOpcode::UnsignedRightShiftInt32 => {
            let value = (left as u32).wrapping_shr((right & 0x1f) as u32);
            if value <= i32::MAX as u32 {
                RuntimeValue::from_i32(value as i32)
            } else {
                RuntimeValue::from_double(f64::from(value))
            }
        }
        _ => return None,
    })
}

fn execute_int32_relational(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = int32_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right = int32_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;
    let result = match opcode {
        CoreOpcode::LessThanInt32 => left < right,
        CoreOpcode::LessEqualInt32 => left <= right,
        CoreOpcode::GreaterThanInt32 => left > right,
        CoreOpcode::GreaterEqualInt32 => left >= right,
        _ => {
            return Ok(BaselineInstructionOutcome::Fallback(
                fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
            ));
        }
    };
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_bool(result),
        fallback,
    )
}

fn execute_pure_number_relational(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left = number_operand_or_fallback(execution, code_block, window, instruction, 1, fallback)?;
    let right =
        number_operand_or_fallback(execution, code_block, window, instruction, 2, fallback)?;
    let left = number_to_f64(left);
    let right = number_to_f64(right);
    let result = match opcode {
        CoreOpcode::LessThanInt32 => left < right,
        CoreOpcode::LessEqualInt32 => left <= right,
        CoreOpcode::GreaterThanInt32 => left > right,
        CoreOpcode::GreaterEqualInt32 => left >= right,
        _ => {
            return Ok(BaselineInstructionOutcome::Fallback(
                fallback.with_cause(BaselineGeneratedFallbackCause::UnsupportedOpcode),
            ));
        }
    };
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_bool(result),
        fallback,
    )
}

fn int32_operand_or_fallback(
    execution: &InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    index: usize,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<i32, BaselineInstructionAbort> {
    let register = register_operand_or_fallback(instruction, index, fallback)?;
    let value = read_register_or_outcome(execution, code_block, window, register, fallback)?;
    match value.as_number() {
        Some(NumberValue::Int32(value)) => Ok(value),
        _ => Err(BaselineInstructionAbort::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::NonInt32Operand {
                operand_index: index as u32,
                register,
            },
        ))),
    }
}

fn number_operand_or_fallback(
    execution: &InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    index: usize,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<NumberValue, BaselineInstructionAbort> {
    let register = register_operand_or_fallback(instruction, index, fallback)?;
    let value = read_register_or_outcome(execution, code_block, window, register, fallback)?;
    match value.as_number() {
        Some(value) => Ok(value),
        None => Err(BaselineInstructionAbort::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::NonNumberOperand {
                operand_index: index as u32,
                register,
                value_kind: value.kind(),
            },
        ))),
    }
}

fn primitive_numeric_operand_or_fallback(
    execution: &InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    index: usize,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<PrimitiveNumericOperand, BaselineInstructionAbort> {
    let register = register_operand_or_fallback(instruction, index, fallback)?;
    let value = read_register_or_outcome(execution, code_block, window, register, fallback)?;
    let value = match value.as_number() {
        Some(_) => value,
        None => match value.kind() {
            ValueKind::Boolean => {
                RuntimeValue::from_i32(i32::from(value.as_bool().unwrap_or(false)))
            }
            ValueKind::Null => RuntimeValue::from_i32(0),
            ValueKind::Undefined => RuntimeValue::from_double(f64::NAN),
            ValueKind::Cell | ValueKind::Unknown => {
                return Err(BaselineInstructionAbort::Fallback(fallback.with_cause(
                    BaselineGeneratedFallbackCause::UnsupportedPrimitiveNumericCoercionOperand {
                        operand_index: index as u32,
                        register,
                        value_kind: value.kind(),
                    },
                )));
            }
            ValueKind::Int32 | ValueKind::Double => value,
        },
    };
    Ok(PrimitiveNumericOperand { register, value })
}

fn number_to_f64(value: NumberValue) -> f64 {
    match value {
        NumberValue::Int32(value) => f64::from(value),
        NumberValue::DoubleBits(bits) => bits.to_f64(),
    }
}

fn number_to_int32(value: NumberValue) -> i32 {
    match value {
        NumberValue::Int32(value) => value,
        NumberValue::DoubleBits(bits) => f64_to_int32(bits.to_f64()),
    }
}

fn f64_to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    const TWO_32: f64 = 4_294_967_296.0;
    const TWO_31: f64 = 2_147_483_648.0;
    let integer = value.signum() * value.abs().floor();
    let mut modulo = integer % TWO_32;
    if modulo < 0.0 {
        modulo += TWO_32;
    }
    if modulo >= TWO_31 {
        (modulo - TWO_32) as i32
    } else {
        modulo as i32
    }
}

fn execute_jump_if_false(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let register = register_operand_or_fallback(instruction, 0, fallback)?;
    let value = read_register_or_outcome(execution, code_block, window, register, fallback)?;
    let target = bytecode_index_operand_or_fallback(instruction, 1, fallback)?;
    let Some(truthy) = local_primitive_truthiness(value) else {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedTruthinessOperand {
                operand_index: 0,
                register,
                value_kind: value.kind(),
            },
        )));
    };
    if truthy {
        Ok(BaselineInstructionOutcome::Continue)
    } else {
        Ok(BaselineInstructionOutcome::Jump(target))
    }
}

fn local_primitive_truthiness(value: RuntimeValue) -> Option<bool> {
    match value.kind() {
        ValueKind::Undefined | ValueKind::Null => Some(false),
        ValueKind::Boolean => Some(value.as_bool().unwrap_or(false)),
        ValueKind::Int32 => Some(!matches!(value.as_number(), Some(NumberValue::Int32(0)))),
        ValueKind::Double => match value.as_number() {
            Some(NumberValue::DoubleBits(bits)) => {
                let value = bits.to_f64();
                Some(value != 0.0 && !value.is_nan())
            }
            _ => Some(false),
        },
        ValueKind::Cell | ValueKind::Unknown => None,
    }
}

fn execute_logical_not(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let source = register_operand_or_fallback(instruction, 1, fallback)?;
    let value = read_register_or_outcome(execution, code_block, window, source, fallback)?;
    let Some(truthy) = local_primitive_truthiness(value) else {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedTruthinessOperand {
                operand_index: 1,
                register: source,
                value_kind: value.kind(),
            },
        )));
    };
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_bool(!truthy),
        fallback,
    )
}

fn execute_strict_equality(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let left = read_register_or_outcome(execution, code_block, window, left_register, fallback)?;
    if unsupported_strict_equality_value(left) {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedStrictEqualityOperand {
                operand_index: 1,
                register: left_register,
                value_kind: left.kind(),
            },
        )));
    }
    let right_register = register_operand_or_fallback(instruction, 2, fallback)?;
    let right = read_register_or_outcome(execution, code_block, window, right_register, fallback)?;
    if unsupported_strict_equality_value(right) {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedStrictEqualityOperand {
                operand_index: 2,
                register: right_register,
                value_kind: right.kind(),
            },
        )));
    }
    let Some(equals) = local_primitive_strict_equals(left, right) else {
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::UnsupportedStrictEqualityOperand {
                operand_index: 1,
                register: left_register,
                value_kind: left.kind(),
            },
        )));
    };
    let result = matches!(opcode, CoreOpcode::StrictEqual) == equals;
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_bool(result),
        fallback,
    )
}

fn execute_local_loose_equality(
    opcode: CoreOpcode,
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let left_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let left = read_register_or_outcome(execution, code_block, window, left_register, fallback)?;
    let right_register = register_operand_or_fallback(instruction, 2, fallback)?;
    let right = read_register_or_outcome(execution, code_block, window, right_register, fallback)?;
    let Some(equals) = local_no_call_loose_equals(left, right) else {
        let (operand_index, register) = if !matches!(left.as_number(), Some(NumberValue::Int32(_)))
        {
            (1, left_register)
        } else {
            (2, right_register)
        };
        return Ok(BaselineInstructionOutcome::Fallback(fallback.with_cause(
            BaselineGeneratedFallbackCause::NonInt32Operand {
                operand_index,
                register,
            },
        )));
    };
    let result = matches!(opcode, CoreOpcode::Equal) == equals;
    write_register_or_outcome(
        execution,
        window,
        destination,
        RuntimeValue::from_bool(result),
        fallback,
    )
}

fn local_no_call_loose_equals(left: RuntimeValue, right: RuntimeValue) -> Option<bool> {
    match (left.as_number(), right.as_number()) {
        (Some(NumberValue::Int32(left)), Some(NumberValue::Int32(right))) => {
            return Some(left == right);
        }
        (Some(_), Some(_)) => return None,
        _ => {}
    }

    let left_nullish = matches!(left.kind(), ValueKind::Undefined | ValueKind::Null);
    let right_nullish = matches!(right.kind(), ValueKind::Undefined | ValueKind::Null);
    if left_nullish || right_nullish {
        return Some(left_nullish && right_nullish);
    }

    if left.kind() == ValueKind::Boolean && right.kind() == ValueKind::Boolean {
        return Some(left.as_bool() == right.as_bool());
    }

    match (left.as_cell(), right.as_cell()) {
        (Some(left), Some(right))
            if left.pointer_payload_bits() == right.pointer_payload_bits() =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn local_primitive_strict_equals(left: RuntimeValue, right: RuntimeValue) -> Option<bool> {
    if unsupported_strict_equality_value(left) || unsupported_strict_equality_value(right) {
        None
    } else {
        Some(left.strict_equals(right))
    }
}

const fn unsupported_strict_equality_kind(kind: ValueKind) -> bool {
    matches!(kind, ValueKind::Cell | ValueKind::Unknown)
}

fn unsupported_strict_equality_value(value: RuntimeValue) -> bool {
    unsupported_strict_equality_kind(value.kind())
}

fn execute_jump_if_not_nullish(
    code_block: &CodeBlock,
    window: RegisterWindow,
    execution: &mut InterpreterExecutionState<'_>,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    let register = register_operand_or_fallback(instruction, 0, fallback)?;
    let value = read_register_or_outcome(execution, code_block, window, register, fallback)?;
    let target = bytecode_index_operand_or_fallback(instruction, 1, fallback)?;
    if matches!(value.kind(), ValueKind::Undefined | ValueKind::Null) {
        Ok(BaselineInstructionOutcome::Continue)
    } else {
        Ok(BaselineInstructionOutcome::Jump(target))
    }
}

fn register_operand_or_fallback(
    instruction: DecodedInstruction<'_>,
    index: usize,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<VirtualRegister, BaselineInstructionAbort> {
    instruction.register_operand(index).map_err(|error| {
        BaselineInstructionAbort::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::OperandAccess { error }),
        )
    })
}

fn bytecode_index_operand_or_fallback(
    instruction: DecodedInstruction<'_>,
    index: usize,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BytecodeIndex, BaselineInstructionAbort> {
    instruction.bytecode_index_operand(index).map_err(|error| {
        BaselineInstructionAbort::Fallback(
            fallback.with_cause(BaselineGeneratedFallbackCause::OperandAccess { error }),
        )
    })
}

fn unsigned_immediate_operand_or_fallback(
    instruction: DecodedInstruction<'_>,
    index: usize,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<u32, BaselineInstructionAbort> {
    instruction
        .unsigned_immediate_operand(index)
        .map_err(|error| {
            BaselineInstructionAbort::Fallback(
                fallback.with_cause(BaselineGeneratedFallbackCause::OperandAccess { error }),
            )
        })
}

fn read_register_or_outcome(
    execution: &InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    register: VirtualRegister,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<RuntimeValue, BaselineInstructionAbort> {
    match baseline_read_register(execution.registers, code_block, window, register) {
        Ok(value) => Ok(value),
        Err(error) => Err(register_read_abort(error, fallback, register)),
    }
}

fn write_register_or_outcome(
    execution: &mut InterpreterExecutionState<'_>,
    window: RegisterWindow,
    register: VirtualRegister,
    value: RuntimeValue,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<BaselineInstructionOutcome, BaselineInstructionAbort> {
    match baseline_write_register(execution.registers, window, register, value) {
        Ok(()) => Ok(BaselineInstructionOutcome::Continue),
        Err(error) => Err(register_write_abort(error, fallback, register)),
    }
}

fn read_return_register_or_fallback(
    execution: &InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    register: VirtualRegister,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<RuntimeValue, BaselineInstructionAbort> {
    match baseline_return_register(execution.registers, code_block, window, register) {
        Ok(value) => Ok(value),
        Err(error) => Err(bad_return_register_abort(error, fallback, register)),
    }
}

fn register_read_abort(
    error: ExecutionError,
    fallback: BaselineGeneratedFallbackSite,
    register: VirtualRegister,
) -> BaselineInstructionAbort {
    let Some(error) = register_fallback_cause(&error) else {
        return execution_error_abort(error);
    };
    BaselineInstructionAbort::Fallback(
        fallback.with_cause(BaselineGeneratedFallbackCause::RegisterRead { register, error }),
    )
}

fn register_write_abort(
    error: ExecutionError,
    fallback: BaselineGeneratedFallbackSite,
    register: VirtualRegister,
) -> BaselineInstructionAbort {
    let Some(error) = register_fallback_cause(&error) else {
        return execution_error_abort(error);
    };
    BaselineInstructionAbort::Fallback(
        fallback.with_cause(BaselineGeneratedFallbackCause::RegisterWrite { register, error }),
    )
}

fn bad_return_register_abort(
    error: ExecutionError,
    fallback: BaselineGeneratedFallbackSite,
    register: VirtualRegister,
) -> BaselineInstructionAbort {
    let Some(error) = register_fallback_cause(&error) else {
        return execution_error_abort(error);
    };
    BaselineInstructionAbort::Fallback(
        fallback.with_cause(BaselineGeneratedFallbackCause::BadReturnRegister { register, error }),
    )
}

fn register_fallback_cause(
    error: &ExecutionError,
) -> Option<BaselineGeneratedRegisterFallbackCause> {
    match error {
        ExecutionError::InvalidRegister => {
            Some(BaselineGeneratedRegisterFallbackCause::InvalidRegister)
        }
        ExecutionError::CannotWriteConstant => {
            Some(BaselineGeneratedRegisterFallbackCause::CannotWriteConstant)
        }
        ExecutionError::CannotAddressHeaderAsValue => {
            Some(BaselineGeneratedRegisterFallbackCause::CannotAddressHeaderAsValue)
        }
        ExecutionError::MissingConstantPool => {
            Some(BaselineGeneratedRegisterFallbackCause::MissingConstantPool)
        }
        ExecutionError::DeferredConstant => {
            Some(BaselineGeneratedRegisterFallbackCause::DeferredConstant)
        }
        ExecutionError::RegisterOutOfBounds => {
            Some(BaselineGeneratedRegisterFallbackCause::RegisterOutOfBounds)
        }
        _ => None,
    }
}

fn execution_error_abort(error: ExecutionError) -> BaselineInstructionAbort {
    BaselineInstructionAbort::Error(BaselineGeneratedExecutionError::Execution(error))
}

const fn fallback_request(
    code_block: CodeBlockId,
    frame: CallFrameId,
    bytecode_index: BytecodeIndex,
) -> BaselineFallbackRequest {
    BaselineFallbackRequest::new(code_block, frame, bytecode_index)
}

fn fallback_site(
    code_block: CodeBlockId,
    frame: CallFrameId,
    instruction: DecodedInstruction<'_>,
) -> BaselineGeneratedFallbackSite {
    let bytecode_index = instruction.bytecode_index;
    BaselineGeneratedFallbackSite {
        request: fallback_request(code_block, frame, bytecode_index),
        bytecode_index,
        opcode: fallback_opcode(instruction.opcode),
    }
}

fn fallback_opcode(opcode: Opcode) -> BaselineGeneratedFallbackOpcode {
    match CoreOpcode::from_opcode(opcode) {
        Some(opcode) => BaselineGeneratedFallbackOpcode::Core(opcode),
        None => BaselineGeneratedFallbackOpcode::NonCore(opcode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::code_block::{CodeKind, LinkContext, UnlinkedCodeBlock};
    use crate::bytecode::instruction::{
        DecodedInstructionSource, Operand, PackedInstructionStream, TypedInstruction,
    };
    use crate::bytecode::opcode::{Opcode, OpcodeId, OperandWidth};
    use crate::bytecode::{
        BytecodeRootMap, BytecodeRootSlotDescriptor, BytecodeRootSlotKind, CodeSpecialization,
        OperandKind, RegisterFrameShape,
    };
    use crate::gc::{
        static_cell_metadata_registry, AllocationMode, BarrierAction, BarrierRequirementOutcome,
        CellId, CellType, Heap, HeapAllocationRequest, StructureId,
    };
    use crate::interpreter::{
        execute_code_block, CoreOpcodeDispatchHost, DispatchConfig, DispatchInstruction,
        DispatchOutcome, DispatchState, ExecutionContextStack, ExecutionEntryRecord,
        FramePushRequest, ProgramExecutionEntry, RegisterFile,
    };
    use crate::jit::ic::GeneratedCallLinkDirectCall;
    use crate::jit::plan::{
        BaselineGeneratedRuntimeBoundaryCandidate, BaselineGeneratedRuntimeBoundaryProof,
        BaselineGeneratedRuntimeHelperPlanMetadata, CompilerSafepointDescriptor,
        CompilerSafepointKind,
    };
    use crate::jit::{
        AbiValue, AccessCaseDescriptor, AccessCaseKind, BaselineBytecodeEligibilityRecord,
        BaselineGeneratedCodeBody, BaselineGeneratedCodeBodyId, CallBoundaryMetadata,
        CallLinkAttachmentTargetDescriptor, CallLinkInfoDescriptor, CallLinkMode,
        CallLinkReadinessBlocker, CallLinkReadinessBlockers, CodeFinalizationAuthority,
        CodeLiveness, EntryAbi, EntrypointKind, GeneratedCallLinkCandidate,
        GeneratedCallLinkCandidateTable, GeneratedCallLinkDirectCallStatus,
        GeneratedCallLinkProbeMissReason, GeneratedCallLinkProbeRequest,
        GeneratedCallLinkProbeResult, GeneratedGuardedPropertyLoadProbeMissReason,
        GeneratedGuardedPropertyLoadProbeRequest, GeneratedGuardedPropertyLoadProbeResult,
        GeneratedPropertyHasMegamorphicCacheEntry, GeneratedPropertyHasMegamorphicCandidateTable,
        GeneratedPropertyHasMegamorphicSite, GeneratedPropertyLoadMegamorphicHolderProbeRequest,
        GeneratedPropertyLoadProbeMissReason, GeneratedPropertyLoadProbeRequest,
        GeneratedPropertyLoadProbeResult, GeneratedPropertyStoreMegamorphicCandidateTable,
        GeneratedPropertyStoreProbeMissReason, GeneratedPropertyStoreProbeRequest,
        GeneratedPropertyStoreProbeResult, InlineCacheSlotId, InlineCacheStubKind, JitCodeId,
        JitType, LinkedCallKind, PropertyLoadAccessCasePlan, PropertyLoadAccessCasePlanContract,
        PropertyLoadAccessCasePlanKind, PropertyLoadGuardChainCertificate,
        PropertyLoadGuardChainEntry, PropertyLoadGuardChainEntryProof,
        PropertyLoadGuardChainOutcome, PropertyLoadGuardDescriptor, PropertyLoadGuardPlan,
        PropertyLoadGuardRequirement, PropertyLoadGuardedCandidate,
        PropertyLoadGuardedCandidateKind, PropertyLoadGuardedCandidateTable,
        PropertyStoreAccessCasePlan, PropertyStoreAccessCasePlanContract,
        PropertyStoreAccessCasePlanKind, PropertyStoreMutationBarrierEvidence,
        PropertyStoreMutationCandidate, PropertyStoreMutationCandidateTable, TieringSnapshot,
        TieringTrigger, WatchpointSetId,
    };
    use crate::object::PropertyOffset;
    use crate::runtime::{ExecutableId, GlobalObjectId, ObjectId};
    use crate::strings::{AtomId, Identifier, PropertyKey};
    use crate::value::{static_value_representation_layout, EncodedJsValue};
    use crate::vm::ExceptionState;

    fn owner() -> CodeBlockId {
        CodeBlockId(CellId(11))
    }

    fn other_owner() -> CodeBlockId {
        CodeBlockId(CellId(12))
    }

    fn local(index: u32) -> VirtualRegister {
        VirtualRegister::local(index)
    }

    fn code_block(instructions: Vec<TypedInstruction>) -> CodeBlock {
        code_block_with_string_literals(instructions, Vec::new())
    }

    fn code_block_with_string_literals(
        instructions: Vec<TypedInstruction>,
        string_literals: Vec<(u32, String)>,
    ) -> CodeBlock {
        let frame = RegisterFrameShape {
            num_parameters_including_this: 1,
            num_vars: 8,
            num_callee_locals: 0,
            num_temporaries: 0,
            special: Default::default(),
        };
        let stream = PackedInstructionStream::from_typed_placeholder(instructions);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, stream)
            .with_frame(frame)
            .with_string_literals(string_literals);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    fn core_typed(offset: u32, opcode: CoreOpcode, operands: Vec<Operand>) -> TypedInstruction {
        typed(offset, opcode.opcode(), operands)
    }

    fn load_double_instruction(
        offset: u32,
        destination: VirtualRegister,
        value: f64,
    ) -> TypedInstruction {
        let bits = value.to_bits();
        core_typed(
            offset,
            CoreOpcode::LoadDouble,
            vec![
                Operand::Register(destination),
                Operand::UnsignedImmediate((bits & u64::from(u32::MAX)) as u32),
                Operand::UnsignedImmediate((bits >> 32) as u32),
            ],
        )
    }

    fn typed(offset: u32, opcode: Opcode, operands: Vec<Operand>) -> TypedInstruction {
        TypedInstruction {
            opcode,
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn decoded_core_handoff_instruction(
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    ) -> DecodedInstruction<'static> {
        decoded_handoff_instruction(bytecode_index, opcode.opcode())
    }

    fn decoded_handoff_instruction(
        bytecode_index: BytecodeIndex,
        opcode: Opcode,
    ) -> DecodedInstruction<'static> {
        DecodedInstruction {
            opcode,
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &[],
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        }
    }

    fn runtime_helper_handoff_frame() -> CallFrameId {
        CallFrameId(33)
    }

    fn js_call_handoff_instruction(opcode: CoreOpcode) -> TypedInstruction {
        let operands = match opcode {
            CoreOpcode::Call | CoreOpcode::Construct => vec![
                Operand::Register(local(1)),
                Operand::Register(local(2)),
                Operand::UnsignedImmediate(0),
            ],
            CoreOpcode::CallWithThis => vec![
                Operand::Register(local(1)),
                Operand::Register(local(2)),
                Operand::Register(local(3)),
                Operand::UnsignedImmediate(0),
            ],
            _ => Vec::new(),
        };
        core_typed(0, opcode, operands)
    }

    #[allow(clippy::too_many_arguments)]
    fn expected_js_call_continuation(
        owner: CodeBlockId,
        frame: CallFrameId,
        opcode: CoreOpcode,
        call_bytecode_index: BytecodeIndex,
        resume_bytecode_index: Option<BytecodeIndex>,
        destination: VirtualRegister,
        argument_count_including_this: u32,
        callee_value: RuntimeValue,
    ) -> BaselineGeneratedJsCallHandoffContinuation {
        BaselineGeneratedJsCallHandoffContinuation::Call(CallReturnContinuation {
            caller_frame: frame,
            callee_frame: None,
            owner,
            call_bytecode_index,
            resume_bytecode_index,
            destination,
            argument_count_including_this,
            callee_value: Some(callee_value),
            callee_object: None,
            kind: CallReturnKind::from_opcode(opcode).unwrap(),
        })
    }

    fn expected_owner_continuation_site(
        artifact: &BaselineGeneratedCodeArtifact,
        bytecode_index: BytecodeIndex,
    ) -> Option<BaselineGeneratedOwnerContinuationSite> {
        artifact
            .owner_continuation_map()
            .and_then(|map| map.call_site_for_bytecode_index(bytecode_index))
            .copied()
    }

    fn baseline_generated_js_call_handoff_for_test(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        instruction: DecodedInstruction<'_>,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> Result<BaselineGeneratedJsCallHandoff, BaselineGeneratedJsCallHandoffError> {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }
        baseline_generated_js_call_handoff(
            owner,
            frame,
            window,
            code_block,
            &InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
            instruction,
        )
    }

    fn property_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        identifier_index: u32,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::get_by_name_property_load(
            owner,
            InlineCacheSlotId(0),
            index,
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                identifier_index,
            ))),
        )
    }

    fn property_store_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        identifier_index: u32,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::put_by_name_property_store(
            owner,
            InlineCacheSlotId(0),
            index,
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                identifier_index,
            ))),
        )
    }

    fn get_length_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        identifier_index: u32,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::get_length_property_load(
            owner,
            InlineCacheSlotId(0),
            index,
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                identifier_index,
            ))),
        )
    }

    fn in_by_id_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        identifier_index: u32,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::in_by_id_has(
            owner,
            InlineCacheSlotId(0),
            index,
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
                identifier_index,
            ))),
        )
    }

    fn in_by_value_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        property_register: VirtualRegister,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::in_by_value_has(
            owner,
            InlineCacheSlotId(0),
            index,
            property_register,
        )
    }

    fn element_load_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        property_register: VirtualRegister,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::get_by_value_element_load(
            owner,
            InlineCacheSlotId(0),
            index,
            property_register,
        )
    }

    fn element_store_handoff_site(
        owner: CodeBlockId,
        index: BytecodeIndex,
        property_register: VirtualRegister,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::put_by_value_element_store(
            owner,
            InlineCacheSlotId(0),
            index,
            property_register,
        )
    }

    fn property_cache_key(identifier_index: u32) -> CacheKey {
        CacheKey::Property(PropertyKey::from_identifier(Identifier::from_atom(
            AtomId::from_table_slot(identifier_index),
        )))
    }

    fn indexed_property_cache_key(index: u32) -> CacheKey {
        CacheKey::Property(PropertyKey::from_index(
            PropertyIndex::from_canonical_index(index),
        ))
    }

    fn property_has_megamorphic_candidate_table(
        owner: CodeBlockId,
        site_bytecode_index: BytecodeIndex,
        key: CacheKey,
        base_structure: StructureId,
        table_epoch: u16,
        entry_epoch: u16,
        result: bool,
    ) -> GeneratedPropertyHasMegamorphicCandidateTable {
        GeneratedPropertyHasMegamorphicCandidateTable::test_with_primary_entry(
            owner,
            table_epoch,
            GeneratedPropertyHasMegamorphicSite {
                owner,
                slot: InlineCacheSlotId(0),
                bytecode_index: site_bytecode_index.offset(),
                key,
            },
            GeneratedPropertyHasMegamorphicCacheEntry {
                key,
                base_structure,
                epoch: entry_epoch,
                result,
            },
        )
    }

    fn property_load_plan(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
        base_structure: StructureId,
        offset: PropertyOffset,
    ) -> PropertyLoadAccessCasePlan {
        let key = property_cache_key(identifier_index);
        PropertyLoadAccessCasePlan {
            plan_kind: PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad,
            owner,
            slot: InlineCacheSlotId(0),
            bytecode_index: bytecode_index.offset(),
            key,
            access_case: AccessCaseDescriptor {
                kind: AccessCaseKind::Load,
                key,
                base_structure: Some(base_structure),
                new_structure: None,
                holder: None,
                offset: Some(offset),
                via_global_proxy: false,
                may_call_js: false,
                dependencies: Vec::new(),
            },
            planned_stub_kind: InlineCacheStubKind::DataOnlyHandler,
            effect_contract: PropertyLoadAccessCasePlanContract::DATA_ONLY_OWN_LOAD,
        }
    }

    fn property_load_plan_table(
        owner: CodeBlockId,
        plans: Vec<PropertyLoadAccessCasePlan>,
    ) -> PropertyLoadAccessCasePlanTable {
        PropertyLoadAccessCasePlanTable::new(owner, plans).unwrap()
    }

    fn property_load_guarded_candidate_table(
        owner: CodeBlockId,
        candidates: Vec<PropertyLoadGuardedCandidate>,
    ) -> PropertyLoadGuardedCandidateTable {
        PropertyLoadGuardedCandidateTable::new(owner, candidates).unwrap()
    }

    fn empty_property_load_guarded_candidate_table(
        owner: CodeBlockId,
    ) -> PropertyLoadGuardedCandidateTable {
        property_load_guarded_candidate_table(owner, Vec::new())
    }

    fn property_store_plan(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
        base_structure: StructureId,
        offset: PropertyOffset,
    ) -> PropertyStoreAccessCasePlan {
        let key = property_cache_key(identifier_index);
        PropertyStoreAccessCasePlan {
            plan_kind: PropertyStoreAccessCasePlanKind::DataOnlyReplace,
            owner,
            slot: InlineCacheSlotId(0),
            bytecode_index: bytecode_index.offset(),
            key,
            access_case: AccessCaseDescriptor {
                kind: AccessCaseKind::Replace,
                key,
                base_structure: Some(base_structure),
                new_structure: None,
                holder: None,
                offset: Some(offset),
                via_global_proxy: false,
                may_call_js: false,
                dependencies: Vec::new(),
            },
            planned_stub_kind: InlineCacheStubKind::RepatchingStub,
            effect_contract: PropertyStoreAccessCasePlanContract::DATA_ONLY_REPLACE,
        }
    }

    fn property_store_indexed_plan(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        index: u32,
        base_structure: StructureId,
    ) -> PropertyStoreAccessCasePlan {
        let key = indexed_property_cache_key(index);
        PropertyStoreAccessCasePlan {
            plan_kind: PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore,
            owner,
            slot: InlineCacheSlotId(0),
            bytecode_index: bytecode_index.offset(),
            key,
            access_case: AccessCaseDescriptor {
                kind: AccessCaseKind::IndexedStore,
                key,
                base_structure: Some(base_structure),
                new_structure: None,
                holder: None,
                offset: None,
                via_global_proxy: false,
                may_call_js: false,
                dependencies: Vec::new(),
            },
            planned_stub_kind: InlineCacheStubKind::RepatchingStub,
            effect_contract: PropertyStoreAccessCasePlanContract::DATA_ONLY_INDEXED_STORE,
        }
    }

    fn property_store_mutation_candidate_table(
        owner: CodeBlockId,
        candidates: Vec<PropertyStoreMutationCandidate>,
    ) -> PropertyStoreMutationCandidateTable {
        PropertyStoreMutationCandidateTable::new(owner, candidates).unwrap()
    }

    fn property_store_mutation_candidate(
        plan: PropertyStoreAccessCasePlan,
        store_plan_ordinal: u64,
    ) -> PropertyStoreMutationCandidate {
        let barrier_evidence = PropertyStoreMutationBarrierEvidence {
            plan_kind: plan.plan_kind,
            effect_contract: plan.effect_contract,
            barrier_effect: plan.effect_contract.barrier,
            observed_write_barrier_count: 1,
            last_write_barrier: BarrierRequirementOutcome::Required(BarrierAction::MarkingBarrier),
        };
        PropertyStoreMutationCandidate {
            plan,
            store_plan_ordinal,
            install_recheck_ordinal: store_plan_ordinal + 100,
            readiness_ordinal: store_plan_ordinal + 300,
            observation_ordinal: store_plan_ordinal + 200,
            barrier_evidence,
            stored_value_kind: ValueKind::Int32,
        }
    }

    fn generated_call_link_candidate(
        opcode: CoreOpcode,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        executable_cell: u32,
        callee_cell: u32,
        target_code_block_cell: u32,
        attachment_ordinal: u64,
    ) -> GeneratedCallLinkCandidate {
        generated_call_link_candidate_for_target(
            opcode,
            slot,
            bytecode_index,
            ExecutableId(CellId(executable_cell.into())),
            ObjectId(CellId(callee_cell.into())),
            CodeBlockId(CellId(target_code_block_cell.into())),
            attachment_ordinal,
        )
    }

    fn generated_call_link_candidate_for_target(
        opcode: CoreOpcode,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        executable: ExecutableId,
        callee: ObjectId,
        target_code_block: CodeBlockId,
        attachment_ordinal: u64,
    ) -> GeneratedCallLinkCandidate {
        let owner = owner();
        let boundary = CallBoundaryId(10_000 + u64::from(slot.0));
        let max_argument_count_including_this = 1;
        let descriptor = CallLinkInfoDescriptor {
            mode: CallLinkMode::Monomorphic,
            call_kind: LinkedCallKind::Call,
            owner: Some(owner),
            executable: Some(executable),
            callee: Some(callee),
            target_code_block: Some(target_code_block),
            boundary: Some(boundary),
            slow_path_count: 0,
            max_argument_count_including_this,
        };

        GeneratedCallLinkCandidate {
            owner,
            opcode,
            slot,
            bytecode_index: bytecode_index.offset(),
            descriptor,
            target: CallLinkAttachmentTargetDescriptor {
                executable,
                target_code_block,
                callee,
                specialization: CodeSpecialization::Call,
            },
            boundary: CallBoundaryMetadata {
                id: boundary,
                owner: Some(owner),
                abi: EntryAbi::LlIntCompatible,
                entry_kind: EntrypointKind::InterpreterThunk,
                native_symbol: None,
                arguments: vec![AbiValue::JsValue; usize::from(max_argument_count_including_this)],
                returns: vec![AbiValue::JsValue],
                registers: Vec::new(),
                frame_slots: Vec::new(),
                requires_vm_entry_scope: true,
                may_call_js: true,
                may_throw: true,
            },
            attachment_ordinal,
            attachment_plan_ordinal: attachment_ordinal + 100,
            install_recheck_ordinal: attachment_ordinal + 200,
            boundary_validation_ordinal: Some(attachment_ordinal + 300),
            descriptor_ordinal: Some(attachment_ordinal + 400),
            observation_ordinal: Some(attachment_ordinal + 500),
            readiness_ordinal: Some(attachment_ordinal + 600),
            remaining_blockers: CallLinkReadinessBlockers::from_blocker(
                CallLinkReadinessBlocker::DirectCallDisallowed,
            ),
            direct_call_status: GeneratedCallLinkDirectCallStatus::Disallowed,
        }
    }

    fn generated_call_link_candidate_table(
        owner: CodeBlockId,
        candidates: Vec<GeneratedCallLinkCandidate>,
    ) -> GeneratedCallLinkCandidateTable {
        GeneratedCallLinkCandidateTable::new(owner, candidates).unwrap()
    }

    fn generated_call_link_cell_value(payload: usize) -> RuntimeValue {
        RuntimeValue::from_encoded(
            static_value_representation_layout()
                .encode_cell_payload(payload)
                .unwrap(),
        )
    }

    fn allocate_test_object_cell(heap: &mut Heap, payload: usize) -> CellId {
        let metadata = static_cell_metadata_registry()
            .metadata_for_type(CellType::Object)
            .expect("object metadata")
            .metadata;
        let allocation = heap
            .allocate_record(HeapAllocationRequest {
                heap: heap.id(),
                subspace: "object",
                metadata,
                byte_size: std::mem::size_of::<RuntimeValue>().max(1),
                mode: AllocationMode::Normal,
                may_trigger_collection: false,
            })
            .expect("object allocation");
        heap.bind_cell_payload(allocation.cell, payload)
            .expect("payload binding");
        allocation.cell
    }

    fn prototype_data_guarded_candidate(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
        guard_plan_ordinal: u64,
    ) -> PropertyLoadGuardedCandidate {
        let base = ObjectId(CellId(101));
        let holder = ObjectId(CellId(102));
        let offset = PropertyOffset::new(0);
        let key = property_cache_key(identifier_index);
        let plan = PropertyLoadGuardPlan {
            owner,
            slot: InlineCacheSlotId(0),
            bytecode_index: bytecode_index.offset(),
            descriptor: PropertyLoadGuardDescriptor {
                requirement: PropertyLoadGuardRequirement::PrototypeChain,
                key,
                base_object: base,
                holder_object: Some(holder),
                base_structure: StructureId::new(1),
                offset: Some(offset),
                prototype_depth: 1,
                chain: PropertyLoadGuardChainCertificate {
                    entries: vec![
                        PropertyLoadGuardChainEntry {
                            object: base,
                            structure: StructureId::new(1),
                            next_prototype: Some(holder),
                            proof: PropertyLoadGuardChainEntryProof::NoOwnProperty,
                        },
                        PropertyLoadGuardChainEntry {
                            object: holder,
                            structure: StructureId::new(2),
                            next_prototype: None,
                            proof: PropertyLoadGuardChainEntryProof::DataProperty { offset },
                        },
                    ],
                    outcome: PropertyLoadGuardChainOutcome::PrototypeData {
                        holder_index: 1,
                        offset,
                    },
                },
            },
        };
        guarded_candidate(
            plan,
            PropertyLoadGuardedCandidateKind::PrototypeData,
            guard_plan_ordinal,
        )
    }

    fn negative_lookup_guarded_candidate(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
        guard_plan_ordinal: u64,
    ) -> PropertyLoadGuardedCandidate {
        let base = ObjectId(CellId(201));
        let key = property_cache_key(identifier_index);
        let plan = PropertyLoadGuardPlan {
            owner,
            slot: InlineCacheSlotId(0),
            bytecode_index: bytecode_index.offset(),
            descriptor: PropertyLoadGuardDescriptor {
                requirement: PropertyLoadGuardRequirement::NegativeLookup,
                key,
                base_object: base,
                holder_object: None,
                base_structure: StructureId::new(3),
                offset: None,
                prototype_depth: 0,
                chain: PropertyLoadGuardChainCertificate {
                    entries: vec![PropertyLoadGuardChainEntry {
                        object: base,
                        structure: StructureId::new(3),
                        next_prototype: None,
                        proof: PropertyLoadGuardChainEntryProof::NoOwnProperty,
                    }],
                    outcome: PropertyLoadGuardChainOutcome::Missing {
                        terminal_null: true,
                    },
                },
            },
        };
        guarded_candidate(
            plan,
            PropertyLoadGuardedCandidateKind::NegativeLookup,
            guard_plan_ordinal,
        )
    }

    fn guarded_candidate(
        plan: PropertyLoadGuardPlan,
        candidate_kind: PropertyLoadGuardedCandidateKind,
        guard_plan_ordinal: u64,
    ) -> PropertyLoadGuardedCandidate {
        let chain_length = plan.descriptor.chain.entries.len();
        PropertyLoadGuardedCandidate {
            plan,
            guard_plan_ordinal,
            materialization_ordinal: guard_plan_ordinal + 100,
            dependency_ordinals: (0..chain_length)
                .map(|index| guard_plan_ordinal + 1_000 + index as u64)
                .collect(),
            binding_set_ids: (0..chain_length)
                .map(|index| WatchpointSetId(guard_plan_ordinal + 2_000 + index as u64))
                .collect(),
            candidate_kind,
        }
    }

    #[derive(Debug, Default)]
    struct SequencedPropertyLoadProbeHost {
        results: Vec<GeneratedPropertyLoadProbeResult>,
        holder_results: Vec<GeneratedPropertyLoadProbeResult>,
        guarded_results: Vec<GeneratedGuardedPropertyLoadProbeResult>,
        call_link_results: Vec<GeneratedCallLinkProbeResult>,
        probed_base_values: Vec<RuntimeValue>,
        probed_plan_keys: Vec<CacheKey>,
        probed_base_structures: Vec<Option<StructureId>>,
        holder_requests: Vec<GeneratedPropertyLoadMegamorphicHolderProbeRequest>,
        guarded_probed_base_values: Vec<RuntimeValue>,
        guarded_probed_plan_keys: Vec<CacheKey>,
        guarded_probed_base_structures: Vec<StructureId>,
        call_link_requests: Vec<GeneratedCallLinkProbeSnapshot>,
        sidecar_base_structure: Option<StructureId>,
        sidecar_base_structure_queries: Vec<RuntimeValue>,
        has_sidecar_runtime_key_results: Vec<Option<CacheKey>>,
        has_sidecar_runtime_key_queries: Vec<RuntimeValue>,
        pending_structure_chain_invalidations: bool,
    }

    impl SequencedPropertyLoadProbeHost {
        fn new(results: Vec<GeneratedPropertyLoadProbeResult>) -> Self {
            Self {
                results,
                holder_results: Vec::new(),
                guarded_results: Vec::new(),
                call_link_results: Vec::new(),
                probed_base_values: Vec::new(),
                probed_plan_keys: Vec::new(),
                probed_base_structures: Vec::new(),
                holder_requests: Vec::new(),
                guarded_probed_base_values: Vec::new(),
                guarded_probed_plan_keys: Vec::new(),
                guarded_probed_base_structures: Vec::new(),
                call_link_requests: Vec::new(),
                sidecar_base_structure: None,
                sidecar_base_structure_queries: Vec::new(),
                has_sidecar_runtime_key_results: Vec::new(),
                has_sidecar_runtime_key_queries: Vec::new(),
                pending_structure_chain_invalidations: false,
            }
        }

        fn new_guarded(results: Vec<GeneratedGuardedPropertyLoadProbeResult>) -> Self {
            Self {
                results: Vec::new(),
                holder_results: Vec::new(),
                guarded_results: results,
                call_link_results: Vec::new(),
                probed_base_values: Vec::new(),
                probed_plan_keys: Vec::new(),
                probed_base_structures: Vec::new(),
                holder_requests: Vec::new(),
                guarded_probed_base_values: Vec::new(),
                guarded_probed_plan_keys: Vec::new(),
                guarded_probed_base_structures: Vec::new(),
                call_link_requests: Vec::new(),
                sidecar_base_structure: None,
                sidecar_base_structure_queries: Vec::new(),
                has_sidecar_runtime_key_results: Vec::new(),
                has_sidecar_runtime_key_queries: Vec::new(),
                pending_structure_chain_invalidations: false,
            }
        }
    }

    impl DispatchHost for SequencedPropertyLoadProbeHost {
        fn generated_property_sidecar_base_structure(
            &mut self,
            base: RuntimeValue,
        ) -> Option<StructureId> {
            self.sidecar_base_structure_queries.push(base);
            self.sidecar_base_structure
        }

        fn generated_property_has_sidecar_base_structure(
            &mut self,
            base: RuntimeValue,
        ) -> Option<StructureId> {
            self.sidecar_base_structure_queries.push(base);
            base.as_cell().and(self.sidecar_base_structure)
        }

        fn generated_property_has_sidecar_runtime_key(
            &mut self,
            property: RuntimeValue,
        ) -> Option<CacheKey> {
            self.has_sidecar_runtime_key_queries.push(property);
            if self.has_sidecar_runtime_key_results.is_empty() {
                return None;
            }
            self.has_sidecar_runtime_key_results.remove(0)
        }

        fn has_pending_structure_chain_invalidation_events(&mut self) -> bool {
            self.pending_structure_chain_invalidations
        }

        fn probe_generated_property_load(
            &mut self,
            request: GeneratedPropertyLoadProbeRequest<'_>,
        ) -> GeneratedPropertyLoadProbeResult {
            self.probed_base_values.push(request.base);
            self.probed_plan_keys.push(request.plan.key);
            self.probed_base_structures
                .push(request.plan.access_case.base_structure);
            if self.results.is_empty() {
                return GeneratedPropertyLoadProbeResult::miss(
                    GeneratedPropertyLoadProbeMissReason::HostUnavailable,
                );
            }
            self.results.remove(0)
        }

        fn probe_generated_property_load_megamorphic_holder(
            &mut self,
            request: GeneratedPropertyLoadMegamorphicHolderProbeRequest,
        ) -> GeneratedPropertyLoadProbeResult {
            self.holder_requests.push(request);
            if self.holder_results.is_empty() {
                return GeneratedPropertyLoadProbeResult::miss(
                    GeneratedPropertyLoadProbeMissReason::HostUnavailable,
                );
            }
            self.holder_results.remove(0)
        }

        fn probe_generated_guarded_property_load(
            &mut self,
            request: GeneratedGuardedPropertyLoadProbeRequest<'_>,
        ) -> GeneratedGuardedPropertyLoadProbeResult {
            self.guarded_probed_base_values.push(request.base);
            self.guarded_probed_plan_keys
                .push(request.plan.descriptor.key);
            self.guarded_probed_base_structures
                .push(request.plan.descriptor.base_structure);
            if self.guarded_results.is_empty() {
                return GeneratedGuardedPropertyLoadProbeResult::miss_for_plan(
                    GeneratedGuardedPropertyLoadProbeMissReason::HostUnavailable,
                    request.plan,
                    None,
                );
            }
            self.guarded_results.remove(0)
        }

        fn probe_generated_call_link(
            &mut self,
            _heap: &mut Heap,
            request: GeneratedCallLinkProbeRequest<'_>,
        ) -> GeneratedCallLinkProbeResult {
            self.call_link_requests
                .push(GeneratedCallLinkProbeSnapshot {
                    owner: request.owner,
                    opcode: request.opcode,
                    bytecode_index: request.bytecode_index,
                    argument_count_including_this: request.argument_count_including_this,
                    candidate_slot: request.candidate.slot,
                    candidate_attachment_ordinal: request.candidate.attachment_ordinal,
                    callee_value: request.callee_value,
                    callee_value_kind: request.callee_value_kind,
                    callee_object: request.callee_object,
                    this_value: request.this_value,
                    this_value_kind: request.this_value_kind,
                    this_object: request.this_object,
                });
            if self.call_link_results.is_empty() {
                return GeneratedCallLinkProbeResult::miss(
                    GeneratedCallLinkProbeMissReason::HostUnavailable,
                );
            }
            self.call_link_results.remove(0)
        }

        fn dispatch_instruction(
            &mut self,
            _state: &mut DispatchState<'_>,
            _instruction: DispatchInstruction<'_>,
        ) -> DispatchOutcome {
            panic!("baseline property-load sidecar tests must not dispatch interpreter opcodes")
        }
    }

    #[derive(Debug, Default)]
    struct SequencedPropertyStoreMutationHost {
        load_results: Vec<GeneratedPropertyLoadProbeResult>,
        probe_results: Vec<GeneratedPropertyStoreProbeResult>,
        mutation_results: Vec<GeneratedPropertyStoreMutationResult>,
        load_probed_base_values: Vec<RuntimeValue>,
        load_probed_plan_keys: Vec<CacheKey>,
        probed_base_values: Vec<RuntimeValue>,
        probed_stored_values: Vec<RuntimeValue>,
        probed_plan_keys: Vec<CacheKey>,
        probed_base_structures: Vec<Option<StructureId>>,
        committed_base_values: Vec<RuntimeValue>,
        committed_keys: Vec<CacheKey>,
        committed_stored_values: Vec<RuntimeValue>,
        sidecar_base_structure: Option<StructureId>,
        sidecar_base_structure_queries: Vec<RuntimeValue>,
    }

    impl SequencedPropertyStoreMutationHost {
        fn new(
            probe_results: Vec<GeneratedPropertyStoreProbeResult>,
            mutation_results: Vec<GeneratedPropertyStoreMutationResult>,
        ) -> Self {
            Self {
                load_results: Vec::new(),
                probe_results,
                mutation_results,
                load_probed_base_values: Vec::new(),
                load_probed_plan_keys: Vec::new(),
                probed_base_values: Vec::new(),
                probed_stored_values: Vec::new(),
                probed_plan_keys: Vec::new(),
                probed_base_structures: Vec::new(),
                committed_base_values: Vec::new(),
                committed_keys: Vec::new(),
                committed_stored_values: Vec::new(),
                sidecar_base_structure: None,
                sidecar_base_structure_queries: Vec::new(),
            }
        }
    }

    impl DispatchHost for SequencedPropertyStoreMutationHost {
        fn generated_property_sidecar_base_structure(
            &mut self,
            base: RuntimeValue,
        ) -> Option<StructureId> {
            self.sidecar_base_structure_queries.push(base);
            self.sidecar_base_structure
        }

        fn probe_generated_property_load(
            &mut self,
            request: GeneratedPropertyLoadProbeRequest<'_>,
        ) -> GeneratedPropertyLoadProbeResult {
            self.load_probed_base_values.push(request.base);
            self.load_probed_plan_keys.push(request.plan.key);
            if self.load_results.is_empty() {
                return GeneratedPropertyLoadProbeResult::miss(
                    GeneratedPropertyLoadProbeMissReason::HostUnavailable,
                );
            }
            self.load_results.remove(0)
        }

        fn probe_generated_property_store(
            &mut self,
            request: GeneratedPropertyStoreProbeRequest<'_>,
        ) -> GeneratedPropertyStoreProbeResult {
            self.probed_base_values.push(request.base);
            self.probed_stored_values.push(request.stored_value);
            self.probed_plan_keys.push(request.plan.key);
            self.probed_base_structures
                .push(request.plan.access_case.base_structure);
            if self.probe_results.is_empty() {
                return GeneratedPropertyStoreProbeResult::hit_for_plan(
                    request.plan,
                    request.stored_value,
                );
            }
            self.probe_results.remove(0)
        }

        fn commit_generated_property_store(
            &mut self,
            _heap: &mut Heap,
            request: GeneratedPropertyStoreMutationRequest,
        ) -> GeneratedPropertyStoreMutationResult {
            self.committed_base_values.push(request.base);
            self.committed_keys.push(request.key());
            self.committed_stored_values
                .push(request.probe_hit.stored_value);
            if self.mutation_results.is_empty() {
                return GeneratedPropertyStoreMutationResult::committed(
                    GeneratedPropertyStoreMutationCommit::host_confirmed_for_request(&request),
                );
            }
            self.mutation_results.remove(0)
        }

        fn dispatch_instruction(
            &mut self,
            _state: &mut DispatchState<'_>,
            _instruction: DispatchInstruction<'_>,
        ) -> DispatchOutcome {
            panic!("baseline property-store sidecar tests must not dispatch interpreter opcodes")
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct GeneratedCallLinkProbeSnapshot {
        owner: CodeBlockId,
        opcode: CoreOpcode,
        bytecode_index: u32,
        argument_count_including_this: u32,
        candidate_slot: InlineCacheSlotId,
        candidate_attachment_ordinal: u64,
        callee_value: RuntimeValue,
        callee_value_kind: ValueKind,
        callee_object: Option<ObjectId>,
        this_value: RuntimeValue,
        this_value_kind: ValueKind,
        this_object: Option<ObjectId>,
    }

    #[derive(Debug, Default)]
    struct SequencedGeneratedCallLinkProbeHost {
        results: Vec<GeneratedCallLinkProbeResult>,
        requests: Vec<GeneratedCallLinkProbeSnapshot>,
    }

    impl SequencedGeneratedCallLinkProbeHost {
        fn new(results: Vec<GeneratedCallLinkProbeResult>) -> Self {
            Self {
                results,
                requests: Vec::new(),
            }
        }
    }

    impl DispatchHost for SequencedGeneratedCallLinkProbeHost {
        fn probe_generated_call_link(
            &mut self,
            _heap: &mut Heap,
            request: GeneratedCallLinkProbeRequest<'_>,
        ) -> GeneratedCallLinkProbeResult {
            self.requests.push(GeneratedCallLinkProbeSnapshot {
                owner: request.owner,
                opcode: request.opcode,
                bytecode_index: request.bytecode_index,
                argument_count_including_this: request.argument_count_including_this,
                candidate_slot: request.candidate.slot,
                candidate_attachment_ordinal: request.candidate.attachment_ordinal,
                callee_value: request.callee_value,
                callee_value_kind: request.callee_value_kind,
                callee_object: request.callee_object,
                this_value: request.this_value,
                this_value_kind: request.this_value_kind,
                this_object: request.this_object,
            });
            if self.results.is_empty() {
                return GeneratedCallLinkProbeResult::miss(
                    GeneratedCallLinkProbeMissReason::HostUnavailable,
                );
            }
            self.results.remove(0)
        }

        fn dispatch_instruction(
            &mut self,
            _state: &mut DispatchState<'_>,
            _instruction: DispatchInstruction<'_>,
        ) -> DispatchOutcome {
            panic!(
                "baseline generated call-link sidecar tests must not dispatch interpreter opcodes"
            )
        }
    }

    fn new_object_runtime_boundary_proof() -> BaselineGeneratedRuntimeBoundaryProof {
        runtime_boundary_proof(CoreOpcode::NewObject, BytecodeIndex::from_offset(20))
    }

    fn new_object_runtime_boundary_proof_at(
        index: BytecodeIndex,
    ) -> BaselineGeneratedRuntimeBoundaryProof {
        runtime_boundary_proof(CoreOpcode::NewObject, index)
    }

    fn new_array_runtime_boundary_proof_at(
        index: BytecodeIndex,
    ) -> BaselineGeneratedRuntimeBoundaryProof {
        runtime_boundary_proof(CoreOpcode::NewArray, index)
    }

    fn type_of_runtime_boundary_proof_at(
        index: BytecodeIndex,
    ) -> BaselineGeneratedRuntimeBoundaryProof {
        let root_map_id = BytecodeRootMapId(42);
        let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::TypeOf,
            safepoint: runtime_helper_safepoint(index, root_map_id),
            root_map: Some(runtime_helper_type_of_root_map(index, root_map_id)),
            no_gc_exit_reentry: true,
        };

        candidate.validate().unwrap()
    }

    fn runtime_boundary_proof(
        opcode: CoreOpcode,
        index: BytecodeIndex,
    ) -> BaselineGeneratedRuntimeBoundaryProof {
        let root_map_id = BytecodeRootMapId(42);
        let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode,
            safepoint: runtime_helper_safepoint(index, root_map_id),
            root_map: Some(runtime_helper_root_map(index, root_map_id)),
            no_gc_exit_reentry: true,
        };

        candidate.validate().unwrap()
    }

    fn runtime_helper_safepoint(
        index: BytecodeIndex,
        root_map: BytecodeRootMapId,
    ) -> CompilerSafepointDescriptor {
        CompilerSafepointDescriptor {
            id: CompilerSafepointId(7),
            owner: Some(owner()),
            code: None,
            tier: JitType::Baseline,
            kind: CompilerSafepointKind::Call,
            bytecode_index: Some(index),
            root_map: Some(root_map),
            roots: Vec::new(),
            may_call: true,
            may_allocate: true,
        }
    }

    fn runtime_helper_root_map(index: BytecodeIndex, id: BytecodeRootMapId) -> BytecodeRootMap {
        BytecodeRootMap {
            id,
            owner: Some(owner()),
            bytecode_range_start: index,
            bytecode_range_end: index,
            slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
            complete: true,
        }
    }

    fn runtime_helper_type_of_root_map(
        index: BytecodeIndex,
        id: BytecodeRootMapId,
    ) -> BytecodeRootMap {
        BytecodeRootMap {
            id,
            owner: Some(owner()),
            bytecode_range_start: index,
            bytecode_range_end: index,
            slots: vec![
                BytecodeRootSlotDescriptor::virtual_register(
                    index,
                    local(0),
                    BytecodeRootSlotKind::VirtualRegister,
                ),
                BytecodeRootSlotDescriptor::virtual_register(
                    index,
                    local(1),
                    BytecodeRootSlotKind::VirtualRegister,
                ),
            ],
            complete: true,
        }
    }

    #[test]
    fn runtime_helper_handoff_accepts_planner_new_object_boundary() {
        let owner = owner();
        let frame = runtime_helper_handoff_frame();
        let index = BytecodeIndex::from_offset(20);
        let proof = new_object_runtime_boundary_proof();
        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::NewObject);

        let handoff =
            baseline_generated_runtime_helper_handoff(owner, frame, instruction, &proof).unwrap();

        assert_eq!(
            handoff.resume,
            BaselineGeneratedRuntimeHelperResume {
                owner,
                frame,
                bytecode_index: index,
                opcode: CoreOpcode::NewObject,
            }
        );
        assert_eq!(handoff.safepoint, CompilerSafepointId(7));
        assert_eq!(handoff.root_map, BytecodeRootMapId(42));
        assert_eq!(handoff.root_count, 1);
        assert!(handoff.requires_no_gc_exit_reentry);
        assert!(handoff.may_throw);
    }

    #[test]
    fn runtime_helper_handoff_rejects_opcode_mismatch() {
        let proof = new_object_runtime_boundary_proof();
        let instruction =
            decoded_core_handoff_instruction(BytecodeIndex::from_offset(20), CoreOpcode::NewArray);

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(BaselineGeneratedRuntimeHelperHandoffError::OpcodeMismatch {
                instruction: CoreOpcode::NewArray,
                proof: CoreOpcode::NewObject,
            })
        );
    }

    #[test]
    fn runtime_helper_handoff_accepts_planner_typeof_boundary() {
        let owner = owner();
        let frame = runtime_helper_handoff_frame();
        let index = BytecodeIndex::from_offset(20);
        let proof = type_of_runtime_boundary_proof_at(index);
        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::TypeOf);

        let handoff =
            baseline_generated_runtime_helper_handoff(owner, frame, instruction, &proof).unwrap();

        assert_eq!(
            handoff.resume,
            BaselineGeneratedRuntimeHelperResume {
                owner,
                frame,
                bytecode_index: index,
                opcode: CoreOpcode::TypeOf,
            }
        );
        assert_eq!(handoff.safepoint, CompilerSafepointId(7));
        assert_eq!(handoff.root_map, BytecodeRootMapId(42));
        assert_eq!(handoff.root_count, 2);
        assert!(handoff.requires_no_gc_exit_reentry);
        assert!(handoff.may_throw);
    }

    #[test]
    fn runtime_helper_handoff_accepts_planner_load_string_boundary() {
        let owner = owner();
        let frame = runtime_helper_handoff_frame();
        let index = BytecodeIndex::from_offset(20);
        let proof = runtime_boundary_proof(CoreOpcode::LoadString, index);
        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::LoadString);

        let handoff =
            baseline_generated_runtime_helper_handoff(owner, frame, instruction, &proof).unwrap();

        assert_eq!(
            handoff.resume,
            BaselineGeneratedRuntimeHelperResume {
                owner,
                frame,
                bytecode_index: index,
                opcode: CoreOpcode::LoadString,
            }
        );
        assert_eq!(handoff.safepoint, CompilerSafepointId(7));
        assert_eq!(handoff.root_map, BytecodeRootMapId(42));
        assert_eq!(handoff.root_count, 1);
        assert!(handoff.requires_no_gc_exit_reentry);
        assert!(handoff.may_throw);
    }

    #[test]
    fn runtime_helper_handoff_accepts_planner_load_bigint_boundary() {
        let owner = owner();
        let frame = runtime_helper_handoff_frame();
        let index = BytecodeIndex::from_offset(20);
        let proof = runtime_boundary_proof(CoreOpcode::LoadBigInt, index);
        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::LoadBigInt);

        let handoff =
            baseline_generated_runtime_helper_handoff(owner, frame, instruction, &proof).unwrap();

        assert_eq!(
            handoff.resume,
            BaselineGeneratedRuntimeHelperResume {
                owner,
                frame,
                bytecode_index: index,
                opcode: CoreOpcode::LoadBigInt,
            }
        );
        assert_eq!(handoff.safepoint, CompilerSafepointId(7));
        assert_eq!(handoff.root_map, BytecodeRootMapId(42));
        assert_eq!(handoff.root_count, 1);
        assert!(handoff.requires_no_gc_exit_reentry);
        assert!(handoff.may_throw);
    }

    #[test]
    fn runtime_helper_handoff_rejects_missing_no_gc_exit_reentry() {
        let mut proof = new_object_runtime_boundary_proof();
        proof.no_gc_exit_reentry = false;
        let instruction =
            decoded_core_handoff_instruction(BytecodeIndex::from_offset(20), CoreOpcode::NewObject);

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(
                BaselineGeneratedRuntimeHelperHandoffError::MissingNoGcExitReentry {
                    opcode: CoreOpcode::NewObject,
                }
            )
        );
    }

    #[test]
    fn runtime_helper_handoff_rejects_stale_may_throw_metadata() {
        let mut proof = new_object_runtime_boundary_proof();
        proof.may_throw = false;
        let instruction =
            decoded_core_handoff_instruction(BytecodeIndex::from_offset(20), CoreOpcode::NewObject);

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(
                BaselineGeneratedRuntimeHelperHandoffError::MayThrowMismatch {
                    opcode: CoreOpcode::NewObject,
                    proof_may_throw: false,
                    contract_may_throw: true,
                }
            )
        );
    }

    #[test]
    fn runtime_helper_handoff_rejects_current_no_heap_contracts() {
        let mut proof = new_object_runtime_boundary_proof();
        proof.contract.opcode = CoreOpcode::LoadInt32;
        proof.contract.effects.calls_runtime_helper = false;
        let instruction =
            decoded_core_handoff_instruction(BytecodeIndex::from_offset(20), CoreOpcode::LoadInt32);

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(
                BaselineGeneratedRuntimeHelperHandoffError::ContractDoesNotCallRuntimeHelper {
                    opcode: CoreOpcode::LoadInt32,
                }
            )
        );

        proof.contract.effects.calls_runtime_helper = true;
        proof.contract.effects.touches_gc_roots = false;

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(
                BaselineGeneratedRuntimeHelperHandoffError::ContractDoesNotTouchGcRoots {
                    opcode: CoreOpcode::LoadInt32,
                }
            )
        );
    }

    #[test]
    fn runtime_helper_handoff_rejects_invalid_instruction_and_root_map_metadata() {
        let proof = new_object_runtime_boundary_proof();
        let invalid_instruction =
            decoded_core_handoff_instruction(BytecodeIndex::INVALID, CoreOpcode::NewObject);

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                invalid_instruction,
                &proof,
            ),
            Err(
                BaselineGeneratedRuntimeHelperHandoffError::InvalidBytecodeIndex {
                    bytecode_index: BytecodeIndex::INVALID,
                }
            )
        );

        let non_core_instruction = decoded_handoff_instruction(
            BytecodeIndex::from_offset(20),
            Opcode::Generated(OpcodeId::from_generated_index(4095)),
        );

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                non_core_instruction,
                &proof,
            ),
            Err(BaselineGeneratedRuntimeHelperHandoffError::NonCoreOpcode {
                opcode: Opcode::Generated(OpcodeId::from_generated_index(4095)),
            })
        );

        let mut proof = proof;
        proof.root_map = None;
        let instruction =
            decoded_core_handoff_instruction(BytecodeIndex::from_offset(20), CoreOpcode::NewObject);

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(BaselineGeneratedRuntimeHelperHandoffError::MissingRootMap {
                opcode: CoreOpcode::NewObject,
                safepoint: CompilerSafepointId(7),
            })
        );

        proof.root_map = Some(BytecodeRootMapId(42));
        proof.contract.requirements.complete_safepoint_root_map = false;

        assert_eq!(
            baseline_generated_runtime_helper_handoff(
                owner(),
                runtime_helper_handoff_frame(),
                instruction,
                &proof,
            ),
            Err(
                BaselineGeneratedRuntimeHelperHandoffError::MissingCompleteSafepointRootMap {
                    opcode: CoreOpcode::NewObject,
                }
            )
        );
    }

    #[test]
    fn js_call_handoff_accepts_core_call_call_with_this_and_construct() {
        let owner = owner();

        for opcode in [CoreOpcode::Call, CoreOpcode::CallWithThis] {
            let block = code_block(vec![js_call_handoff_instruction(opcode)]);
            let instruction = block
                .decoded_instruction_at(BytecodeIndex::from_offset(0))
                .unwrap();
            let handoff =
                baseline_generated_js_call_handoff_for_test(owner, &block, instruction, &[])
                    .unwrap();
            let frame = handoff.resume.frame;

            assert_eq!(
                handoff,
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner,
                        frame,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode,
                    },
                    continuation: expected_js_call_continuation(
                        owner,
                        frame,
                        opcode,
                        BytecodeIndex::from_offset(0),
                        None,
                        local(1),
                        1,
                        RuntimeValue::undefined(),
                    ),
                    owner_continuation: None,
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            );
        }
        let construct_block = code_block(vec![js_call_handoff_instruction(CoreOpcode::Construct)]);
        let construct_instruction = construct_block
            .decoded_instruction_at(BytecodeIndex::from_offset(0))
            .unwrap();
        let construct_handoff = baseline_generated_js_call_handoff_for_test(
            owner,
            &construct_block,
            construct_instruction,
            &[],
        )
        .unwrap();
        let construct_frame = construct_handoff.resume.frame;
        let construct_window = construct_handoff
            .continuation
            .as_construct()
            .expect("construct continuation")
            .caller_window;

        assert_eq!(
            construct_handoff,
            BaselineGeneratedJsCallHandoff {
                resume: BaselineGeneratedJsCallResume {
                    owner,
                    frame: construct_frame,
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::Construct,
                },
                continuation: BaselineGeneratedJsCallHandoffContinuation::Construct(
                    BaselineGeneratedJsConstructContinuation {
                        caller_frame: construct_frame,
                        caller_window: construct_window,
                        owner,
                        construct_bytecode_index: BytecodeIndex::from_offset(0),
                        resume_bytecode_index: None,
                        destination: local(1),
                        argument_count_including_this: 1,
                        callee_value: RuntimeValue::undefined(),
                        callee_object: None,
                    },
                ),
                owner_continuation: None,
                direct_call: None,
                requires_no_gc_exit_reentry: true,
                may_throw: true,
            }
        );
        let block = code_block(vec![js_call_handoff_instruction(CoreOpcode::Call)]);
        let index = BytecodeIndex::from_offset(20);
        for opcode in [CoreOpcode::CallDirect, CoreOpcode::ConstructSuper] {
            assert_eq!(
                baseline_generated_js_call_handoff_for_test(
                    owner,
                    &block,
                    decoded_core_handoff_instruction(index, opcode),
                    &[],
                ),
                Err(BaselineGeneratedJsCallHandoffError::UnsupportedOpcode { opcode })
            );
        }
        assert_eq!(
            baseline_generated_js_call_handoff_for_test(
                owner,
                &block,
                decoded_core_handoff_instruction(BytecodeIndex::INVALID, CoreOpcode::Call),
                &[],
            ),
            Err(BaselineGeneratedJsCallHandoffError::InvalidBytecodeIndex {
                bytecode_index: BytecodeIndex::INVALID,
            })
        );
        assert_eq!(
            baseline_generated_js_call_handoff_for_test(
                owner,
                &block,
                decoded_handoff_instruction(
                    index,
                    Opcode::Generated(OpcodeId::from_generated_index(4095)),
                ),
                &[],
            ),
            Err(BaselineGeneratedJsCallHandoffError::NonCoreOpcode {
                opcode: Opcode::Generated(OpcodeId::from_generated_index(4095)),
            })
        );
    }

    #[test]
    fn property_handoff_accepts_named_property_load_and_store() {
        let owner = owner();
        let frame = runtime_helper_handoff_frame();
        let index = BytecodeIndex::from_offset(20);
        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::GetByName);
        let site = property_handoff_site(owner, index, 11);

        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::GetByName,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::PutByName);
        let site = property_store_handoff_site(owner, index, 11);
        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::PutByName,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::GetByValue);
        let site = element_load_handoff_site(owner, index, VirtualRegister::local(3));
        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::GetByValue,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::PutByValue);
        let site = element_store_handoff_site(owner, index, VirtualRegister::local(3));
        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::PutByValue,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::GetLength);
        let site = get_length_handoff_site(owner, index, 11);
        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::GetLength,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::InById);
        let site = in_by_id_handoff_site(owner, index, 11);
        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::InById,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        let instruction = decoded_core_handoff_instruction(index, CoreOpcode::InByVal);
        let site = in_by_value_handoff_site(owner, index, VirtualRegister::local(3));
        let handoff =
            baseline_generated_property_handoff(owner, frame, instruction, &site).unwrap();

        assert_eq!(
            handoff,
            BaselineGeneratedPropertyHandoff {
                resume: BaselineGeneratedPropertyResume {
                    owner,
                    frame,
                    bytecode_index: index,
                    opcode: CoreOpcode::InByVal,
                },
                site,
                requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                may_throw: site.may_throw,
            }
        );

        for opcode in [
            CoreOpcode::DeleteByName,
            CoreOpcode::DeleteByValue,
            CoreOpcode::ArrayLength,
            CoreOpcode::CallWithThis,
            CoreOpcode::Construct,
        ] {
            assert_eq!(
                baseline_generated_property_handoff(
                    owner,
                    frame,
                    decoded_core_handoff_instruction(index, opcode),
                    &site,
                ),
                Err(BaselineGeneratedPropertyHandoffError::UnsupportedOpcode { opcode })
            );
        }
        assert_eq!(
            baseline_generated_property_handoff(
                owner,
                frame,
                decoded_core_handoff_instruction(BytecodeIndex::INVALID, CoreOpcode::GetByName),
                &site,
            ),
            Err(
                BaselineGeneratedPropertyHandoffError::InvalidBytecodeIndex {
                    bytecode_index: BytecodeIndex::INVALID,
                }
            )
        );
        assert_eq!(
            baseline_generated_property_handoff(
                owner,
                frame,
                decoded_handoff_instruction(
                    index,
                    Opcode::Generated(OpcodeId::from_generated_index(4095)),
                ),
                &site,
            ),
            Err(BaselineGeneratedPropertyHandoffError::NonCoreOpcode {
                opcode: Opcode::Generated(OpcodeId::from_generated_index(4095)),
            })
        );
    }

    #[test]
    fn generated_executor_load_callee_reads_active_frame_callee_without_fallback() {
        let block = code_block(vec![
            core_typed(0, CoreOpcode::LoadCallee, vec![Operand::Register(local(0))]),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let callee = cell_runtime_value();
        let (result, stack, registers) =
            execute_generated_with_frame_callee(owner(), &block, &artifact, callee);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(callee)
            ))
        );
        assert_eq!(read_local(&registers, &stack, 0), callee);
    }

    #[test]
    fn generated_executor_returns_js_call_handoff_after_generated_prefix() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let (result, stack, registers) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(1),
                        opcode: CoreOpcode::Call,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::Call,
                        BytecodeIndex::from_offset(1),
                        Some(BytecodeIndex::from_offset(2)),
                        local(1),
                        1,
                        RuntimeValue::undefined(),
                    ),
                    owner_continuation: expected_owner_continuation_site(
                        &artifact,
                        BytecodeIndex::from_offset(1),
                    ),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(BytecodeIndex::from_offset(1)));
        assert_eq!(read_local(&registers, &stack, 0), RuntimeValue::from_i32(7));
    }

    #[test]
    fn generated_executor_returns_call_with_this_handoff_after_generated_prefix() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::CallWithThis,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::Register(local(3)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let (result, stack, registers) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(1),
                        opcode: CoreOpcode::CallWithThis,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::CallWithThis,
                        BytecodeIndex::from_offset(1),
                        Some(BytecodeIndex::from_offset(2)),
                        local(1),
                        1,
                        RuntimeValue::undefined(),
                    ),
                    owner_continuation: expected_owner_continuation_site(
                        &artifact,
                        BytecodeIndex::from_offset(1),
                    ),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(BytecodeIndex::from_offset(1)));
        assert_eq!(read_local(&registers, &stack, 0), RuntimeValue::from_i32(7));
    }

    #[test]
    fn generated_call_link_sidecar_records_blocked_probe_and_preserves_js_call_handoff() {
        let bytecode_index = BytecodeIndex::from_offset(1);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::CallWithThis,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::Register(local(3)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let candidate = generated_call_link_candidate(
            CoreOpcode::CallWithThis,
            InlineCacheSlotId(71),
            bytecode_index,
            81,
            91,
            101,
            11,
        );
        let table = generated_call_link_candidate_table(owner(), vec![candidate.clone()]);
        let callee_value = generated_call_link_cell_value(0x5001);
        let this_value = generated_call_link_cell_value(0x5002);
        let mut host =
            SequencedGeneratedCallLinkProbeHost::new(vec![GeneratedCallLinkProbeResult::blocked(
                GeneratedCallLinkProbeMissReason::DirectCallDisallowed,
            )]);

        let (result, stack, registers, miss_records, blocked_records) =
            execute_generated_with_generated_call_link_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(2), callee_value), (local(3), this_value)],
            );
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index,
                        opcode: CoreOpcode::CallWithThis,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::CallWithThis,
                        bytecode_index,
                        Some(BytecodeIndex::from_offset(2)),
                        local(1),
                        1,
                        callee_value,
                    ),
                    owner_continuation: expected_owner_continuation_site(&artifact, bytecode_index),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(bytecode_index));
        assert_eq!(read_local(&registers, &stack, 0), RuntimeValue::from_i32(7));
        assert!(miss_records.is_empty());
        assert_eq!(
            blocked_records,
            vec![BaselineGeneratedCallLinkProbeBlockedRecord {
                owner: candidate.owner,
                bytecode_index,
                slot: candidate.slot,
                attachment_ordinal: candidate.attachment_ordinal,
                attachment_plan_ordinal: candidate.attachment_plan_ordinal,
                install_recheck_ordinal: candidate.install_recheck_ordinal,
                boundary_validation_ordinal: candidate.boundary_validation_ordinal,
                descriptor_ordinal: candidate.descriptor_ordinal,
                observation_ordinal: candidate.observation_ordinal,
                readiness_ordinal: candidate.readiness_ordinal,
                target_executable: candidate.target.executable,
                target_callee: candidate.target.callee,
                target_code_block: candidate.target.target_code_block,
                target_boundary: candidate.boundary.id,
                direct_call_status: candidate.direct_call_status,
                reason: GeneratedCallLinkProbeMissReason::DirectCallDisallowed,
            }]
        );
        assert_eq!(
            host.requests,
            vec![GeneratedCallLinkProbeSnapshot {
                owner: owner(),
                opcode: CoreOpcode::CallWithThis,
                bytecode_index: bytecode_index.offset(),
                argument_count_including_this: 1,
                candidate_slot: candidate.slot,
                candidate_attachment_ordinal: candidate.attachment_ordinal,
                callee_value,
                callee_value_kind: ValueKind::Cell,
                callee_object: None,
                this_value,
                this_value_kind: ValueKind::Cell,
                this_object: None,
            }]
        );
    }

    #[test]
    fn generated_call_link_sidecar_records_bounded_miss_or_candidate_not_found_and_hands_off() {
        let bytecode_index = BytecodeIndex::from_offset(1);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let first_candidate = generated_call_link_candidate(
            CoreOpcode::Call,
            InlineCacheSlotId(72),
            bytecode_index,
            82,
            92,
            102,
            21,
        );
        let second_candidate = generated_call_link_candidate(
            CoreOpcode::Call,
            InlineCacheSlotId(73),
            bytecode_index,
            83,
            93,
            103,
            22,
        );
        let table = generated_call_link_candidate_table(
            owner(),
            vec![first_candidate.clone(), second_candidate.clone()],
        );
        let callee_value = generated_call_link_cell_value(0x6001);
        let mut host = SequencedGeneratedCallLinkProbeHost::new(vec![
            GeneratedCallLinkProbeResult::miss(GeneratedCallLinkProbeMissReason::CalleeMismatch),
            GeneratedCallLinkProbeResult::miss(GeneratedCallLinkProbeMissReason::HostUnavailable),
        ]);

        let (result, stack, _, miss_records, blocked_records) =
            execute_generated_with_generated_call_link_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(2), callee_value)],
            );
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index,
                        opcode: CoreOpcode::Call,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::Call,
                        bytecode_index,
                        Some(BytecodeIndex::from_offset(2)),
                        local(1),
                        1,
                        callee_value,
                    ),
                    owner_continuation: expected_owner_continuation_site(&artifact, bytecode_index),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert!(blocked_records.is_empty());
        assert_eq!(host.requests.len(), 2);
        assert_eq!(
            miss_records,
            vec![
                BaselineGeneratedCallLinkProbeMissRecord {
                    owner: owner(),
                    bytecode_index,
                    slot: Some(first_candidate.slot),
                    attachment_ordinal: Some(first_candidate.attachment_ordinal),
                    attachment_plan_ordinal: Some(first_candidate.attachment_plan_ordinal),
                    install_recheck_ordinal: Some(first_candidate.install_recheck_ordinal),
                    boundary_validation_ordinal: first_candidate.boundary_validation_ordinal,
                    descriptor_ordinal: first_candidate.descriptor_ordinal,
                    observation_ordinal: first_candidate.observation_ordinal,
                    readiness_ordinal: first_candidate.readiness_ordinal,
                    target_executable: Some(first_candidate.target.executable),
                    target_callee: Some(first_candidate.target.callee),
                    target_code_block: Some(first_candidate.target.target_code_block),
                    target_boundary: Some(first_candidate.boundary.id),
                    direct_call_status: Some(first_candidate.direct_call_status),
                    reason: GeneratedCallLinkProbeMissReason::CalleeMismatch,
                },
                BaselineGeneratedCallLinkProbeMissRecord {
                    owner: owner(),
                    bytecode_index,
                    slot: Some(second_candidate.slot),
                    attachment_ordinal: Some(second_candidate.attachment_ordinal),
                    attachment_plan_ordinal: Some(second_candidate.attachment_plan_ordinal),
                    install_recheck_ordinal: Some(second_candidate.install_recheck_ordinal),
                    boundary_validation_ordinal: second_candidate.boundary_validation_ordinal,
                    descriptor_ordinal: second_candidate.descriptor_ordinal,
                    observation_ordinal: second_candidate.observation_ordinal,
                    readiness_ordinal: second_candidate.readiness_ordinal,
                    target_executable: Some(second_candidate.target.executable),
                    target_callee: Some(second_candidate.target.callee),
                    target_code_block: Some(second_candidate.target.target_code_block),
                    target_boundary: Some(second_candidate.boundary.id),
                    direct_call_status: Some(second_candidate.direct_call_status),
                    reason: GeneratedCallLinkProbeMissReason::HostUnavailable,
                },
            ]
        );

        let no_candidate = generated_call_link_candidate(
            CoreOpcode::Call,
            InlineCacheSlotId(74),
            BytecodeIndex::from_offset(9),
            84,
            94,
            104,
            23,
        );
        let table = generated_call_link_candidate_table(owner(), vec![no_candidate]);
        let mut host =
            SequencedGeneratedCallLinkProbeHost::new(vec![GeneratedCallLinkProbeResult::blocked(
                GeneratedCallLinkProbeMissReason::DirectCallDisallowed,
            )]);

        let (result, stack, _, miss_records, blocked_records) =
            execute_generated_with_generated_call_link_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(2), callee_value)],
            );
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index,
                        opcode: CoreOpcode::Call,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::Call,
                        bytecode_index,
                        Some(BytecodeIndex::from_offset(2)),
                        local(1),
                        1,
                        callee_value,
                    ),
                    owner_continuation: expected_owner_continuation_site(&artifact, bytecode_index),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert!(blocked_records.is_empty());
        assert!(host.requests.is_empty());
        assert_eq!(
            miss_records,
            vec![BaselineGeneratedCallLinkProbeMissRecord {
                owner: owner(),
                bytecode_index,
                slot: None,
                attachment_ordinal: None,
                attachment_plan_ordinal: None,
                install_recheck_ordinal: None,
                boundary_validation_ordinal: None,
                descriptor_ordinal: None,
                observation_ordinal: None,
                readiness_ordinal: None,
                target_executable: None,
                target_callee: None,
                target_code_block: None,
                target_boundary: None,
                direct_call_status: None,
                reason: GeneratedCallLinkProbeMissReason::CandidateNotFound,
            }]
        );
    }

    #[test]
    fn generated_call_link_sidecar_skips_nonmatching_candidates_for_known_callee() {
        let bytecode_index = BytecodeIndex::from_offset(1);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let nonmatching_candidate = generated_call_link_candidate(
            CoreOpcode::Call,
            InlineCacheSlotId(75),
            bytecode_index,
            85,
            95,
            105,
            24,
        );
        let matching_payload = 0x6002;
        let matching_value = generated_call_link_cell_value(matching_payload);
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let matching_cell = allocate_test_object_cell(&mut heap, matching_payload);
        let matching_candidate = generated_call_link_candidate_for_target(
            CoreOpcode::Call,
            InlineCacheSlotId(76),
            bytecode_index,
            ExecutableId(CellId(86)),
            ObjectId(matching_cell),
            CodeBlockId(CellId(106)),
            25,
        );
        let table = generated_call_link_candidate_table(
            owner(),
            vec![nonmatching_candidate.clone(), matching_candidate.clone()],
        );
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(2), matching_value).unwrap();
        let instruction = block.decoded_instruction_at(bytecode_index).unwrap();
        let fallback = fallback_site(owner(), frame, instruction);
        let mut miss_records = Vec::new();
        let mut blocked_records = Vec::new();
        let mut hot_slot_hits = 0;
        let mut host = SequencedGeneratedCallLinkProbeHost::new(vec![
            GeneratedCallLinkProbeResult::DirectCall(GeneratedCallLinkDirectCall::from_candidate(
                &matching_candidate,
            )),
        ]);

        let direct_call = execute_generated_call_link_sidecar_probe_with_host(
            &table,
            &[],
            &mut host,
            &mut miss_records,
            &mut blocked_records,
            &mut hot_slot_hits,
            &mut InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
            GeneratedCallLinkSidecarAttempt {
                window,
                code_block: &block,
                fallback,
                owner: owner(),
                opcode: CoreOpcode::Call,
                instruction,
            },
        )
        .expect("call-link sidecar probe");

        assert_eq!(
            direct_call.as_ref().map(|direct| direct.candidate.slot),
            Some(matching_candidate.slot)
        );
        assert_eq!(
            host.requests,
            vec![GeneratedCallLinkProbeSnapshot {
                owner: owner(),
                opcode: CoreOpcode::Call,
                bytecode_index: bytecode_index.offset(),
                argument_count_including_this: 1,
                candidate_slot: matching_candidate.slot,
                candidate_attachment_ordinal: matching_candidate.attachment_ordinal,
                callee_value: matching_value,
                callee_value_kind: ValueKind::Cell,
                callee_object: Some(ObjectId(matching_cell)),
                this_value: RuntimeValue::undefined(),
                this_value_kind: ValueKind::Undefined,
                this_object: None,
            }]
        );
        assert!(miss_records.is_empty());
        assert!(blocked_records.is_empty());
    }

    #[test]
    fn generated_call_link_sidecar_uses_hot_slot_before_host_probe() {
        let bytecode_index = BytecodeIndex::from_offset(1);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let matching_payload = 0x6102;
        let matching_value = generated_call_link_cell_value(matching_payload);
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let matching_cell = allocate_test_object_cell(&mut heap, matching_payload);
        let mut candidate = generated_call_link_candidate_for_target(
            CoreOpcode::Call,
            InlineCacheSlotId(77),
            bytecode_index,
            ExecutableId(CellId(87)),
            ObjectId(matching_cell),
            CodeBlockId(CellId(107)),
            26,
        );
        candidate.remaining_blockers =
            CallLinkReadinessBlockers::from_blocker(CallLinkReadinessBlocker::MayCallJsBoundary);
        candidate.direct_call_status = GeneratedCallLinkDirectCallStatus::Authorized;
        candidate.observation_ordinal = Some(100);
        candidate.descriptor_ordinal = Some(200);
        candidate.readiness_ordinal = Some(300);
        candidate.boundary_validation_ordinal = Some(400);
        candidate.attachment_plan_ordinal = 500;
        candidate.install_recheck_ordinal = 600;
        candidate.attachment_ordinal = 700;
        let authorization = GeneratedCallLinkDirectCall::from_candidate(&candidate);
        let hot_slots = vec![BaselineGeneratedJsDirectCallHotSlot {
            candidate: candidate.clone(),
            authorization,
            callee_object: ObjectId(matching_cell),
            argument_count_including_this: 1,
        }];
        let table = generated_call_link_candidate_table(owner(), vec![candidate.clone()]);
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(2), matching_value).unwrap();
        let instruction = block.decoded_instruction_at(bytecode_index).unwrap();
        let fallback = fallback_site(owner(), frame, instruction);
        let mut miss_records = Vec::new();
        let mut blocked_records = Vec::new();
        let mut hot_slot_hits = 0;
        let mut host = SequencedGeneratedCallLinkProbeHost::new(Vec::new());

        let direct_call = execute_generated_call_link_sidecar_probe_with_host(
            &table,
            &hot_slots,
            &mut host,
            &mut miss_records,
            &mut blocked_records,
            &mut hot_slot_hits,
            &mut InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
            GeneratedCallLinkSidecarAttempt {
                window,
                code_block: &block,
                fallback,
                owner: owner(),
                opcode: CoreOpcode::Call,
                instruction,
            },
        )
        .expect("call-link hot-slot sidecar probe")
        .expect("hot-slot direct call");

        assert_eq!(direct_call.candidate, candidate);
        assert_eq!(direct_call.authorization, authorization);
        assert_eq!(direct_call.callee_value, matching_value);
        assert_eq!(direct_call.callee_object, ObjectId(matching_cell));
        assert_eq!(direct_call.this_value, RuntimeValue::undefined());
        assert_eq!(direct_call.argument_count_including_this, 1);
        assert_eq!(hot_slot_hits, 1);
        assert!(
            host.requests.is_empty(),
            "monomorphic hot slot hit should avoid DispatchHost::probe_generated_call_link"
        );
        assert!(miss_records.is_empty());
        assert!(blocked_records.is_empty());
    }

    #[test]
    fn generated_call_link_sidecar_probes_when_property_sidecars_are_present() {
        let property_index = BytecodeIndex::from_offset(0);
        let call_index = BytecodeIndex::from_offset(1);
        let block = code_block(vec![
            core_typed(
                property_index.offset(),
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(
                call_index.offset(),
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(3)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let load_table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                property_index,
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let guarded_table = empty_property_load_guarded_candidate_table(owner());
        let candidate = generated_call_link_candidate(
            CoreOpcode::Call,
            InlineCacheSlotId(75),
            call_index,
            85,
            95,
            105,
            31,
        );
        let call_table = generated_call_link_candidate_table(owner(), vec![candidate.clone()]);
        let base = cell_runtime_value();
        let loaded_value = RuntimeValue::from_i32(42);
        let callee_value = generated_call_link_cell_value(0x7001);
        let mut property_host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                loaded_value,
            )]);
        property_host
            .call_link_results
            .push(GeneratedCallLinkProbeResult::blocked(
                GeneratedCallLinkProbeMissReason::DirectCallDisallowed,
            ));

        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(0), base).unwrap();
        registers.write(window, local(3), callee_value).unwrap();

        let result;
        let destination_root_sync_requests;
        let property_load_probe_miss_records;
        let guarded_property_load_probe_miss_records;
        let call_link_probe_miss_records;
        let call_link_probe_blocked_records;
        {
            let mut property_sidecars =
                BaselineGeneratedPropertyExecutionSidecars::new_with_generated_call_link(
                    &mut property_host,
                    Some((&load_table, &guarded_table)),
                    None,
                    &call_table,
                );
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact: &artifact,
                    owner: owner(),
                    code_block: &block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut property_sidecars,
            );
            destination_root_sync_requests =
                property_sidecars.destination_root_sync_requests().to_vec();
            property_load_probe_miss_records = property_sidecars
                .property_load_probe_miss_records()
                .to_vec();
            guarded_property_load_probe_miss_records = property_sidecars
                .guarded_property_load_probe_miss_records()
                .to_vec();
            call_link_probe_miss_records = property_sidecars
                .generated_call_link_probe_miss_records()
                .to_vec();
            call_link_probe_blocked_records = property_sidecars
                .generated_call_link_probe_blocked_records()
                .to_vec();
        }
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: call_index,
                        opcode: CoreOpcode::Call,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::Call,
                        call_index,
                        Some(BytecodeIndex::from_offset(2)),
                        local(2),
                        1,
                        callee_value,
                    ),
                    owner_continuation: expected_owner_continuation_site(&artifact, call_index),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(call_index));
        assert_eq!(read_local(&registers, &stack, 1), loaded_value);
        assert!(destination_root_sync_requests.is_empty());
        assert!(property_load_probe_miss_records.is_empty());
        assert!(guarded_property_load_probe_miss_records.is_empty());
        assert_eq!(property_host.probed_base_values, vec![base]);
        assert_eq!(property_host.probed_plan_keys, vec![property_cache_key(11)]);
        assert!(call_link_probe_miss_records.is_empty());
        assert_eq!(
            call_link_probe_blocked_records,
            vec![BaselineGeneratedCallLinkProbeBlockedRecord {
                owner: candidate.owner,
                bytecode_index: call_index,
                slot: candidate.slot,
                attachment_ordinal: candidate.attachment_ordinal,
                attachment_plan_ordinal: candidate.attachment_plan_ordinal,
                install_recheck_ordinal: candidate.install_recheck_ordinal,
                boundary_validation_ordinal: candidate.boundary_validation_ordinal,
                descriptor_ordinal: candidate.descriptor_ordinal,
                observation_ordinal: candidate.observation_ordinal,
                readiness_ordinal: candidate.readiness_ordinal,
                target_executable: candidate.target.executable,
                target_callee: candidate.target.callee,
                target_code_block: candidate.target.target_code_block,
                target_boundary: candidate.boundary.id,
                direct_call_status: candidate.direct_call_status,
                reason: GeneratedCallLinkProbeMissReason::DirectCallDisallowed,
            }]
        );
        assert_eq!(
            property_host.call_link_requests,
            vec![GeneratedCallLinkProbeSnapshot {
                owner: owner(),
                opcode: CoreOpcode::Call,
                bytecode_index: call_index.offset(),
                argument_count_including_this: 1,
                candidate_slot: candidate.slot,
                candidate_attachment_ordinal: candidate.attachment_ordinal,
                callee_value,
                callee_value_kind: ValueKind::Cell,
                callee_object: None,
                this_value: RuntimeValue::undefined(),
                this_value_kind: ValueKind::Undefined,
                this_object: None,
            }]
        );
    }

    #[test]
    fn generated_call_link_no_sidecar_preserves_existing_call_handoff_behavior() {
        let bytecode_index = BytecodeIndex::from_offset(1);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::UnsignedImmediate(0),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let (result, stack, registers) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::JsCall(
                BaselineGeneratedJsCallHandoff {
                    resume: BaselineGeneratedJsCallResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index,
                        opcode: CoreOpcode::Call,
                    },
                    continuation: expected_js_call_continuation(
                        owner(),
                        frame.id,
                        CoreOpcode::Call,
                        bytecode_index,
                        Some(BytecodeIndex::from_offset(2)),
                        local(1),
                        1,
                        RuntimeValue::undefined(),
                    ),
                    owner_continuation: expected_owner_continuation_site(&artifact, bytecode_index),
                    direct_call: None,
                    requires_no_gc_exit_reentry: true,
                    may_throw: true,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(bytecode_index));
        assert_eq!(read_local(&registers, &stack, 0), RuntimeValue::from_i32(7));
    }

    #[test]
    fn generated_executor_returns_property_handoff_after_generated_prefix() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let (result, stack, registers) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap();
        let site = property_handoff_site(owner(), BytecodeIndex::from_offset(1), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(1),
                        opcode: CoreOpcode::GetByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(BytecodeIndex::from_offset(1)));
        assert_eq!(read_local(&registers, &stack, 0), RuntimeValue::from_i32(7));
    }

    #[test]
    fn generated_executor_returns_put_by_name_property_handoff_after_generated_prefix() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(2)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(0)),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let (result, stack, registers) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap();
        let site = property_store_handoff_site(owner(), BytecodeIndex::from_offset(1), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(1),
                        opcode: CoreOpcode::PutByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(frame.bytecode_index, Some(BytecodeIndex::from_offset(1)));
        assert_eq!(read_local(&registers, &stack, 0), RuntimeValue::from_i32(7));
    }

    #[test]
    fn generated_get_by_name_without_sidecar_remains_handoff_only() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let initial_destination = RuntimeValue::from_i32(17);
        let (result, stack, registers) = execute_generated_with_initial_locals(
            owner(),
            &block,
            &artifact,
            &[
                (local(0), initial_destination),
                (local(1), cell_runtime_value()),
            ],
        );
        let frame = stack.top_frame().unwrap();
        let site = property_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::GetByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 0), initial_destination);
    }

    #[test]
    fn generated_get_by_name_sidecar_hit_with_immediate_continues_to_return() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let table = property_load_plan_table(
            owner(),
            vec![
                property_load_plan(
                    owner(),
                    BytecodeIndex::from_offset(0),
                    11,
                    StructureId::new(1),
                    PropertyOffset::new(0),
                ),
                property_load_plan(
                    owner(),
                    BytecodeIndex::from_offset(0),
                    11,
                    StructureId::new(2),
                    PropertyOffset::new(1),
                ),
            ],
        );
        let mut host = SequencedPropertyLoadProbeHost::new(vec![
            GeneratedPropertyLoadProbeResult::miss(
                GeneratedPropertyLoadProbeMissReason::StructureMismatch,
            ),
            GeneratedPropertyLoadProbeResult::hit(RuntimeValue::from_i32(42)),
        ]);

        let (result, _, _, root_sync_requests, probe_miss_records) =
            execute_generated_with_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
            ))
        );
        assert!(root_sync_requests.is_empty());
        assert_eq!(
            host.probed_base_structures,
            vec![Some(StructureId::new(2)), Some(StructureId::new(1))]
        );
        assert_eq!(
            host.probed_plan_keys,
            vec![property_cache_key(11), property_cache_key(11)]
        );
        assert_eq!(
            probe_miss_records,
            vec![BaselineGeneratedPropertyLoadProbeMissRecord {
                owner: owner(),
                bytecode_index: BytecodeIndex::from_offset(0),
                key: property_cache_key(11),
                base_structure: Some(StructureId::new(2)),
                offset: Some(PropertyOffset::new(1)),
                reason: GeneratedPropertyLoadProbeMissReason::StructureMismatch,
            }]
        );
    }

    #[test]
    fn generated_get_by_name_sidecar_skips_known_structure_mismatch_before_host_probe() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let table = property_load_plan_table(
            owner(),
            vec![
                property_load_plan(
                    owner(),
                    BytecodeIndex::from_offset(0),
                    11,
                    StructureId::new(2),
                    PropertyOffset::new(1),
                ),
                property_load_plan(
                    owner(),
                    BytecodeIndex::from_offset(0),
                    11,
                    StructureId::new(1),
                    PropertyOffset::new(0),
                ),
            ],
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, _, _, root_sync_requests, probe_miss_records) =
            execute_generated_with_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
            ))
        );
        assert!(root_sync_requests.is_empty());
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_base_structures, vec![Some(StructureId::new(2))]);
        assert!(probe_miss_records.is_empty());
    }

    #[test]
    fn generated_get_by_name_megamorphic_sidecar_hits_by_active_structure() {
        use crate::jit::{
            GeneratedPropertyLoadMegamorphicCacheEntry,
            GeneratedPropertyLoadMegamorphicCacheEntryKind, GeneratedPropertyLoadMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyLoadMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 0,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: StructureId::new(2),
                epoch: 1,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                    offset: PropertyOffset::new(7),
                },
            },
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_megamorphic_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
            ))
        );
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_records.is_empty());
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_plan_keys, vec![key]);
        assert_eq!(host.probed_base_structures, vec![Some(StructureId::new(2))]);
    }

    #[test]
    fn generated_get_by_name_megamorphic_sidecar_needs_base_structure_before_probe() {
        use crate::jit::{
            GeneratedPropertyLoadMegamorphicCacheEntry,
            GeneratedPropertyLoadMegamorphicCacheEntryKind, GeneratedPropertyLoadMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyLoadMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 0,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: StructureId::new(2),
                epoch: 1,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                    offset: PropertyOffset::new(7),
                },
            },
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_megamorphic_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert!(matches!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(_))
        ));
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_records.is_empty());
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert!(host.probed_base_values.is_empty());
    }

    #[test]
    fn generated_get_by_name_megamorphic_sidecar_skips_pending_structure_chain_invalidation() {
        use crate::jit::{
            GeneratedPropertyLoadMegamorphicCacheEntry,
            GeneratedPropertyLoadMegamorphicCacheEntryKind, GeneratedPropertyLoadMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyLoadMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 0,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: StructureId::new(2),
                epoch: 1,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                    offset: PropertyOffset::new(7),
                },
            },
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.pending_structure_chain_invalidations = true;

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_megamorphic_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert!(matches!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(_))
        ));
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_records.is_empty());
        assert!(host.sidecar_base_structure_queries.is_empty());
        assert!(host.probed_base_values.is_empty());
    }

    #[test]
    fn generated_get_by_name_megamorphic_sidecar_missing_entry_writes_undefined_without_host_probe()
    {
        use crate::jit::{
            GeneratedPropertyLoadMegamorphicCacheEntry,
            GeneratedPropertyLoadMegamorphicCacheEntryKind, GeneratedPropertyLoadMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyLoadMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 0,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: StructureId::new(2),
                epoch: 1,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::Missing,
            },
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_megamorphic_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::undefined())
            ))
        );
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_records.is_empty());
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert!(host.probed_base_values.is_empty());
    }

    #[test]
    fn generated_get_by_name_megamorphic_sidecar_prototype_entry_uses_holder_probe() {
        use crate::jit::{
            GeneratedPropertyLoadMegamorphicCacheEntry,
            GeneratedPropertyLoadMegamorphicCacheEntryKind, GeneratedPropertyLoadMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let holder = ObjectId(CellId(902));
        let offset = PropertyOffset::new(5);
        let base_structure = StructureId::new(2);
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyLoadMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 0,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure,
                epoch: 1,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::PrototypeData {
                    holder,
                    offset,
                },
            },
        );
        let base = cell_runtime_value();
        let loaded = RuntimeValue::from_i32(84);
        let mut host = SequencedPropertyLoadProbeHost::new(Vec::new());
        host.holder_results = vec![GeneratedPropertyLoadProbeResult::hit(loaded)];
        host.sidecar_base_structure = Some(base_structure);

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_megamorphic_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(loaded)
            ))
        );
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_records.is_empty());
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert!(host.probed_base_values.is_empty());
        assert!(host.guarded_probed_base_values.is_empty());
        assert_eq!(
            host.holder_requests,
            vec![GeneratedPropertyLoadMegamorphicHolderProbeRequest {
                key,
                base_structure,
                holder,
                offset,
            }]
        );
    }

    #[test]
    fn generated_get_by_name_megamorphic_sidecar_ignores_unrelated_sites_without_host_query() {
        use crate::jit::{
            GeneratedPropertyLoadMegamorphicCacheEntry,
            GeneratedPropertyLoadMegamorphicCacheEntryKind, GeneratedPropertyLoadMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = GeneratedPropertyLoadMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyLoadMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 99,
                key,
            },
            GeneratedPropertyLoadMegamorphicCacheEntry {
                key,
                base_structure: StructureId::new(2),
                epoch: 1,
                kind: GeneratedPropertyLoadMegamorphicCacheEntryKind::OwnData {
                    offset: PropertyOffset::new(7),
                },
            },
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_megamorphic_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert!(matches!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(_))
        ));
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_records.is_empty());
        assert!(host.sidecar_base_structure_queries.is_empty());
        assert!(host.probed_base_values.is_empty());
    }

    #[test]
    fn generated_get_by_name_sidecar_probes_newest_matching_candidate_first() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let table = property_load_plan_table(
            owner(),
            vec![
                property_load_plan(
                    owner(),
                    BytecodeIndex::from_offset(0),
                    11,
                    StructureId::new(1),
                    PropertyOffset::new(0),
                ),
                property_load_plan(
                    owner(),
                    BytecodeIndex::from_offset(0),
                    11,
                    StructureId::new(2),
                    PropertyOffset::new(1),
                ),
            ],
        );
        let base = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);

        let (result, _, _, root_sync_requests, probe_miss_records) =
            execute_generated_with_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), base)],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
            ))
        );
        assert!(root_sync_requests.is_empty());
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_base_structures, vec![Some(StructureId::new(2))]);
        assert_eq!(host.probed_plan_keys, vec![property_cache_key(11)]);
        assert!(probe_miss_records.is_empty());
    }

    #[test]
    fn generated_get_by_name_sidecar_cell_hit_records_destination_root_sync() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let cell_value = cell_runtime_value();
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                cell_value,
            )]);

        let (result, stack, _, root_sync_requests, _) =
            execute_generated_with_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(cell_value)
            ))
        );
        assert_eq!(
            root_sync_requests,
            vec![BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                frame: frame.id,
                bytecode_index: BytecodeIndex::from_offset(0),
                destination: local(0),
            }]
        );
    }

    #[test]
    fn generated_get_by_name_sidecar_host_unavailable_keeps_property_handoff() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let initial_destination = RuntimeValue::from_i32(17);
        let mut host = SequencedPropertyLoadProbeHost::default();

        let (result, stack, registers, root_sync_requests, probe_miss_records) =
            execute_generated_with_property_load_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[
                    (local(0), initial_destination),
                    (local(1), cell_runtime_value()),
                ],
            );
        let frame = stack.top_frame().unwrap();
        let site = property_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::GetByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 0), initial_destination);
        assert!(root_sync_requests.is_empty());
        assert_eq!(host.probed_plan_keys, vec![property_cache_key(11)]);
        assert_eq!(
            probe_miss_records,
            vec![BaselineGeneratedPropertyLoadProbeMissRecord {
                owner: owner(),
                bytecode_index: BytecodeIndex::from_offset(0),
                key: property_cache_key(11),
                base_structure: Some(StructureId::new(1)),
                offset: Some(PropertyOffset::new(0)),
                reason: GeneratedPropertyLoadProbeMissReason::HostUnavailable,
            }]
        );
    }

    #[test]
    fn baseline_generated_property_store_sidecar_hit_commits_through_host() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let table = property_store_mutation_candidate_table(owner(), vec![candidate.clone()]);
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let return_value = RuntimeValue::from_i32(13);
        let mut host = SequencedPropertyStoreMutationHost::default();

        let (result, stack, registers, probe_miss_records, rejection_records, commit_records) =
            execute_generated_with_property_store_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[
                    (local(0), base),
                    (local(2), stored_value),
                    (local(3), return_value),
                ],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(return_value)
            ))
        );
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
        assert!(probe_miss_records.is_empty());
        assert!(rejection_records.is_empty());
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_stored_values, vec![stored_value]);
        assert_eq!(host.committed_base_values, vec![base]);
        assert_eq!(host.committed_stored_values, vec![stored_value]);
        assert_eq!(host.committed_keys, vec![property_cache_key(11)]);
        assert_eq!(commit_records.len(), 1);
        assert_eq!(commit_records[0].owner, owner());
        assert_eq!(
            commit_records[0].bytecode_index,
            BytecodeIndex::from_offset(0)
        );
        assert_eq!(commit_records[0].slot, InlineCacheSlotId(0));
        assert_eq!(commit_records[0].key, property_cache_key(11));
        assert_eq!(commit_records[0].store_plan_ordinal, 1);
        assert_eq!(commit_records[0].readiness_ordinal, 301);
        assert_eq!(commit_records[0].commit.stored_value, stored_value);
    }

    #[test]
    fn baseline_generated_property_store_megamorphic_sidecar_hits_by_active_structure() {
        use crate::jit::{
            GeneratedPropertyStoreMegamorphicCacheEntry,
            GeneratedPropertyStoreMegamorphicCandidateTable, GeneratedPropertyStoreMegamorphicSite,
        };

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = GeneratedPropertyStoreMegamorphicCandidateTable::test_with_primary_entry(
            owner(),
            1,
            GeneratedPropertyStoreMegamorphicSite {
                owner: owner(),
                slot: InlineCacheSlotId(0),
                bytecode_index: 0,
                key,
            },
            GeneratedPropertyStoreMegamorphicCacheEntry {
                key,
                old_structure: StructureId::new(2),
                new_structure: StructureId::new(2),
                epoch: 1,
                offset: PropertyOffset::new(7),
                reallocating: false,
            },
        );
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let return_value = RuntimeValue::from_i32(13);
        let mut host = SequencedPropertyStoreMutationHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, registers) = execute_generated_with_megamorphic_property_store_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[
                (local(0), base),
                (local(2), stored_value),
                (local(3), return_value),
            ],
        );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(return_value)
            ))
        );
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_stored_values, vec![stored_value]);
        assert_eq!(host.probed_plan_keys, vec![key]);
        assert_eq!(host.probed_base_structures, vec![Some(StructureId::new(2))]);
        assert_eq!(host.committed_base_values, vec![base]);
        assert_eq!(host.committed_keys, vec![key]);
        assert_eq!(host.committed_stored_values, vec![stored_value]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_hits_true_by_active_structure() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let base = cell_runtime_value();
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, registers) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base)],
        );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_bool(true))
            ))
        );
        assert_eq!(
            read_local(&registers, &stack, 0),
            RuntimeValue::from_bool(true)
        );
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_hits_false_by_active_structure() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            false,
        );
        let base = cell_runtime_value();
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, registers) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base)],
        );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_bool(false))
            ))
        );
        assert_eq!(
            read_local(&registers, &stack, 0),
            RuntimeValue::from_bool(false)
        );
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_stale_entry_falls_back() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            2,
            1,
            true,
        );
        let base = cell_runtime_value();
        let initial_destination = RuntimeValue::from_i32(17);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, registers) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(0), initial_destination), (local(1), base)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_id_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InById,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 0), initial_destination);
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_skips_pending_structure_chain_invalidation(
    ) {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let base = cell_runtime_value();
        let mut host = SequencedPropertyLoadProbeHost {
            pending_structure_chain_invalidations: true,
            ..Default::default()
        };
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_id_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InById,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert!(host.sidecar_base_structure_queries.is_empty());
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_ignores_unrelated_sites_without_host_query(
    ) {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(9),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), cell_runtime_value())],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_id_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InById,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert!(host.sidecar_base_structure_queries.is_empty());
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_keeps_primitive_base_on_handoff() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let base = RuntimeValue::from_i32(7);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_id_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InById,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_hits_in_by_val_atom_key() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.has_sidecar_runtime_key_results = vec![Some(key)];
        let base = cell_runtime_value();
        let runtime_key = generated_call_link_cell_value(0x5678);

        let (result, stack, registers) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base), (local(3), runtime_key)],
        );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_bool(true))
            ))
        );
        assert_eq!(
            read_local(&registers, &stack, 0),
            RuntimeValue::from_bool(true)
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_hits_in_by_val_cached_false() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            false,
        );
        let base = cell_runtime_value();
        let runtime_key = generated_call_link_cell_value(0x5678);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.has_sidecar_runtime_key_results = vec![Some(key)];

        let (result, stack, registers) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base), (local(3), runtime_key)],
        );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_bool(false))
            ))
        );
        assert_eq!(
            read_local(&registers, &stack, 0),
            RuntimeValue::from_bool(false)
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_in_by_val_pending_invalidation_skips_base_query(
    ) {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let runtime_key = generated_call_link_cell_value(0x5678);
        let mut host = SequencedPropertyLoadProbeHost {
            pending_structure_chain_invalidations: true,
            ..Default::default()
        };
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.has_sidecar_runtime_key_results = vec![Some(key)];

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), cell_runtime_value()), (local(3), runtime_key)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_value_handoff_site(owner(), BytecodeIndex::from_offset(0), local(3));

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InByVal,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert!(host.sidecar_base_structure_queries.is_empty());
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_in_by_val_mismatched_key_falls_back_without_base_query(
    ) {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let runtime_key = generated_call_link_cell_value(0x5678);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.has_sidecar_runtime_key_results = vec![Some(property_cache_key(12))];

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), cell_runtime_value()), (local(3), runtime_key)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_value_handoff_site(owner(), BytecodeIndex::from_offset(0), local(3));

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InByVal,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert!(host.sidecar_base_structure_queries.is_empty());
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_in_by_val_dynamic_key_falls_back_without_base_query(
    ) {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let runtime_key = RuntimeValue::from_i32(11);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), cell_runtime_value()), (local(3), runtime_key)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_value_handoff_site(owner(), BytecodeIndex::from_offset(0), local(3));

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InByVal,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert!(host.sidecar_base_structure_queries.is_empty());
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_in_by_val_index_key_falls_back_without_base_query(
    ) {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let runtime_key = generated_call_link_cell_value(0x5678);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.has_sidecar_runtime_key_results = vec![Some(indexed_property_cache_key(0))];

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), cell_runtime_value()), (local(3), runtime_key)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_value_handoff_site(owner(), BytecodeIndex::from_offset(0), local(3));

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InByVal,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert!(host.sidecar_base_structure_queries.is_empty());
    }

    #[test]
    fn baseline_generated_property_has_megamorphic_sidecar_in_by_val_primitive_base_falls_back() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let key = property_cache_key(11);
        let table = property_has_megamorphic_candidate_table(
            owner(),
            BytecodeIndex::from_offset(0),
            key,
            StructureId::new(2),
            1,
            1,
            true,
        );
        let base = RuntimeValue::from_i32(7);
        let runtime_key = generated_call_link_cell_value(0x5678);
        let mut host = SequencedPropertyLoadProbeHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));
        host.has_sidecar_runtime_key_results = vec![Some(key)];

        let (result, stack, _) = execute_generated_with_megamorphic_property_has_sidecar(
            owner(),
            &block,
            &artifact,
            &table,
            &mut host,
            &[(local(1), base), (local(3), runtime_key)],
        );
        let frame = stack.top_frame().unwrap();
        let site = in_by_value_handoff_site(owner(), BytecodeIndex::from_offset(0), local(3));

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::InByVal,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(host.has_sidecar_runtime_key_queries, vec![runtime_key]);
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
    }

    #[test]
    fn baseline_generated_property_store_mixed_load_store_sidecars_share_one_host() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(
                1,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(12),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let load_table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let guarded_table = empty_property_load_guarded_candidate_table(owner());
        let store_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(1),
                12,
                StructureId::new(1),
                PropertyOffset::new(1),
            ),
            1,
        );
        let store_table =
            property_store_mutation_candidate_table(owner(), vec![store_candidate.clone()]);
        let base = cell_runtime_value();
        let loaded_value = RuntimeValue::from_i32(42);
        let mut host = SequencedPropertyStoreMutationHost {
            load_results: vec![GeneratedPropertyLoadProbeResult::hit(loaded_value)],
            ..Default::default()
        };

        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(0), base).unwrap();

        let result;
        let destination_root_sync_requests;
        let property_load_probe_miss_records;
        let property_store_probe_miss_records;
        let property_store_mutation_rejection_records;
        let property_store_mutation_commit_records;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
                &mut host,
                Some((&load_table, &guarded_table)),
                Some(&store_table),
            );
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact: &artifact,
                    owner: owner(),
                    code_block: &block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
            destination_root_sync_requests = sidecars.destination_root_sync_requests().to_vec();
            property_load_probe_miss_records = sidecars.property_load_probe_miss_records().to_vec();
            property_store_probe_miss_records =
                sidecars.property_store_probe_miss_records().to_vec();
            property_store_mutation_rejection_records = sidecars
                .property_store_mutation_rejection_records()
                .to_vec();
            property_store_mutation_commit_records =
                sidecars.property_store_mutation_commit_records().to_vec();
        }

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(loaded_value)
            ))
        );
        assert!(destination_root_sync_requests.is_empty());
        assert!(property_load_probe_miss_records.is_empty());
        assert!(property_store_probe_miss_records.is_empty());
        assert!(property_store_mutation_rejection_records.is_empty());
        assert_eq!(read_local(&registers, &stack, 1), loaded_value);
        assert_eq!(host.load_probed_base_values, vec![base]);
        assert_eq!(host.load_probed_plan_keys, vec![property_cache_key(11)]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_stored_values, vec![loaded_value]);
        assert_eq!(host.probed_plan_keys, vec![property_cache_key(12)]);
        assert_eq!(host.committed_base_values, vec![base]);
        assert_eq!(host.committed_stored_values, vec![loaded_value]);
        assert_eq!(property_store_mutation_commit_records.len(), 1);
        assert_eq!(
            property_store_mutation_commit_records[0].slot,
            InlineCacheSlotId(0)
        );
        assert_eq!(
            property_store_mutation_commit_records[0].bytecode_index,
            BytecodeIndex::from_offset(1)
        );
    }

    #[test]
    fn baseline_generated_property_increment_sidecar_skips_numeric_update_chain() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(5)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(
                1,
                CoreOpcode::ToNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(
                2,
                CoreOpcode::Move,
                vec![Operand::Register(local(2)), Operand::Register(local(1))],
            ),
            core_typed(
                3,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                4,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(4)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(
                5,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(5)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(4)),
                ],
            ),
            core_typed(6, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let load_table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let guarded_table = empty_property_load_guarded_candidate_table(owner());
        let store_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(5),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let store_table =
            property_store_mutation_candidate_table(owner(), vec![store_candidate.clone()]);
        let base = cell_runtime_value();
        let old_value = RuntimeValue::from_i32(41);
        let updated_value = RuntimeValue::from_i32(42);
        let mut host = SequencedPropertyStoreMutationHost {
            load_results: vec![GeneratedPropertyLoadProbeResult::hit(old_value)],
            ..Default::default()
        };

        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(5), base).unwrap();

        let result;
        let property_load_probe_miss_records;
        let property_store_probe_miss_records;
        let property_store_mutation_rejection_records;
        let property_store_mutation_commit_records;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
                &mut host,
                Some((&load_table, &guarded_table)),
                Some(&store_table),
            );
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact: &artifact,
                    owner: owner(),
                    code_block: &block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
            property_load_probe_miss_records = sidecars.property_load_probe_miss_records().to_vec();
            property_store_probe_miss_records =
                sidecars.property_store_probe_miss_records().to_vec();
            property_store_mutation_rejection_records = sidecars
                .property_store_mutation_rejection_records()
                .to_vec();
            property_store_mutation_commit_records =
                sidecars.property_store_mutation_commit_records().to_vec();
        }

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(old_value)
            ))
        );
        assert!(property_load_probe_miss_records.is_empty());
        assert!(property_store_probe_miss_records.is_empty());
        assert!(property_store_mutation_rejection_records.is_empty());
        assert_eq!(property_store_mutation_commit_records.len(), 1);
        assert_eq!(read_local(&registers, &stack, 0), old_value);
        assert_eq!(read_local(&registers, &stack, 1), old_value);
        assert_eq!(read_local(&registers, &stack, 2), old_value);
        assert_eq!(read_local(&registers, &stack, 3), RuntimeValue::from_i32(1));
        assert_eq!(read_local(&registers, &stack, 4), updated_value);
        assert_eq!(host.load_probed_base_values, vec![base]);
        assert_eq!(host.load_probed_plan_keys, vec![property_cache_key(11)]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_stored_values, vec![updated_value]);
        assert_eq!(host.probed_plan_keys, vec![property_cache_key(11)]);
        assert_eq!(host.committed_base_values, vec![base]);
        assert_eq!(host.committed_stored_values, vec![updated_value]);
    }

    #[test]
    fn baseline_generated_property_increment_sidecar_preserves_register_aliasing_order() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(5)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(
                1,
                CoreOpcode::ToNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                3,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(4)),
                    Operand::Register(local(1)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(
                4,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(5)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(4)),
                ],
            ),
            core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(4))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let load_table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let guarded_table = empty_property_load_guarded_candidate_table(owner());
        let store_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(4),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let store_table =
            property_store_mutation_candidate_table(owner(), vec![store_candidate.clone()]);
        let base = cell_runtime_value();
        let mut host = SequencedPropertyStoreMutationHost {
            load_results: vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(41),
            )],
            ..Default::default()
        };

        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(5), base).unwrap();

        let result;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
                &mut host,
                Some((&load_table, &guarded_table)),
                Some(&store_table),
            );
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact: &artifact,
                    owner: owner(),
                    code_block: &block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
        }

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(2))
            ))
        );
        assert_eq!(read_local(&registers, &stack, 1), RuntimeValue::from_i32(1));
        assert_eq!(read_local(&registers, &stack, 4), RuntimeValue::from_i32(2));
        assert_eq!(host.probed_stored_values, vec![RuntimeValue::from_i32(2)]);
        assert_eq!(
            host.committed_stored_values,
            vec![RuntimeValue::from_i32(2)]
        );
    }

    #[test]
    fn baseline_generated_property_increment_sidecar_store_miss_resumes_at_put() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(5)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(
                1,
                CoreOpcode::ToNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                3,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(4)),
                    Operand::Register(local(1)),
                    Operand::Register(local(3)),
                ],
            ),
            core_typed(
                4,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(5)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(4)),
                ],
            ),
            core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(4))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let load_table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let guarded_table = empty_property_load_guarded_candidate_table(owner());
        let store_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(4),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let store_table =
            property_store_mutation_candidate_table(owner(), vec![store_candidate.clone()]);
        let base = cell_runtime_value();
        let old_value = RuntimeValue::from_i32(41);
        let updated_value = RuntimeValue::from_i32(42);
        let mut host = SequencedPropertyStoreMutationHost {
            load_results: vec![GeneratedPropertyLoadProbeResult::hit(old_value)],
            probe_results: vec![GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::StructureMismatch,
            )],
            ..Default::default()
        };

        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let window = stack.top_frame().unwrap().register_window;
        registers.write(window, local(5), base).unwrap();

        let result;
        let property_store_probe_miss_records;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
                &mut host,
                Some((&load_table, &guarded_table)),
                Some(&store_table),
            );
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact: &artifact,
                    owner: owner(),
                    code_block: &block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
            property_store_probe_miss_records =
                sidecars.property_store_probe_miss_records().to_vec();
        }

        let Ok(BaselineGeneratedExecutionResult::Property(handoff)) = result else {
            panic!("expected PutByName property handoff after fused numeric prefix");
        };
        assert_eq!(handoff.site.bytecode_index, BytecodeIndex::from_offset(4));
        assert_eq!(handoff.site.opcode, CoreOpcode::PutByName);
        assert_eq!(read_local(&registers, &stack, 1), old_value);
        assert_eq!(read_local(&registers, &stack, 3), RuntimeValue::from_i32(1));
        assert_eq!(read_local(&registers, &stack, 4), updated_value);
        assert_eq!(host.probed_stored_values, vec![updated_value]);
        assert_eq!(property_store_probe_miss_records.len(), 1);
        assert_eq!(
            property_store_probe_miss_records[0].bytecode_index,
            BytecodeIndex::from_offset(4)
        );
    }

    #[test]
    fn baseline_generated_property_store_probe_miss_falls_back_and_records_miss() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let table = property_store_mutation_candidate_table(owner(), vec![candidate]);
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let mut host = SequencedPropertyStoreMutationHost::new(
            vec![GeneratedPropertyStoreProbeResult::miss(
                GeneratedPropertyStoreProbeMissReason::StructureMismatch,
            )],
            Vec::new(),
        );

        let (result, stack, registers, probe_miss_records, rejection_records, commit_records) =
            execute_generated_with_property_store_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(0), base), (local(2), stored_value)],
            );
        let frame = stack.top_frame().unwrap();
        let site = property_store_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::PutByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
        assert_eq!(host.probed_base_values, vec![base]);
        assert!(host.committed_base_values.is_empty());
        assert!(rejection_records.is_empty());
        assert!(commit_records.is_empty());
        assert_eq!(
            probe_miss_records,
            vec![BaselineGeneratedPropertyStoreProbeMissRecord {
                owner: owner(),
                bytecode_index: BytecodeIndex::from_offset(0),
                slot: InlineCacheSlotId(0),
                key: property_cache_key(11),
                plan_kind: PropertyStoreAccessCasePlanKind::DataOnlyReplace,
                base_structure: Some(StructureId::new(1)),
                planned_new_structure: None,
                offset: Some(PropertyOffset::new(0)),
                store_plan_ordinal: 1,
                readiness_ordinal: 301,
                stored_value_kind: ValueKind::Int32,
                reason: GeneratedPropertyStoreProbeMissReason::StructureMismatch,
            }]
        );
    }

    #[test]
    fn baseline_generated_property_store_sidecar_skips_known_structure_mismatch_before_host_probe()
    {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let skipped_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let matching_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(2),
                PropertyOffset::new(1),
            ),
            2,
        );
        let table = property_store_mutation_candidate_table(
            owner(),
            vec![matching_candidate, skipped_candidate],
        );
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let return_value = RuntimeValue::from_i32(13);
        let mut host = SequencedPropertyStoreMutationHost::default();
        host.sidecar_base_structure = Some(StructureId::new(2));

        let (result, stack, registers, probe_miss_records, rejection_records, commit_records) =
            execute_generated_with_property_store_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[
                    (local(0), base),
                    (local(2), stored_value),
                    (local(3), return_value),
                ],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(return_value)
            ))
        );
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_base_structures, vec![Some(StructureId::new(2))]);
        assert!(probe_miss_records.is_empty());
        assert!(rejection_records.is_empty());
        assert_eq!(commit_records.len(), 1);
        assert_eq!(commit_records[0].store_plan_ordinal, 2);
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
    }

    #[test]
    fn baseline_generated_property_store_sidecar_probes_newest_matching_candidate_first() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let older_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let newer_candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(2),
                PropertyOffset::new(1),
            ),
            2,
        );
        let table = property_store_mutation_candidate_table(
            owner(),
            vec![older_candidate, newer_candidate],
        );
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let return_value = RuntimeValue::from_i32(13);
        let mut host = SequencedPropertyStoreMutationHost::default();

        let (result, stack, registers, probe_miss_records, rejection_records, commit_records) =
            execute_generated_with_property_store_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[
                    (local(0), base),
                    (local(2), stored_value),
                    (local(3), return_value),
                ],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(return_value)
            ))
        );
        assert_eq!(host.sidecar_base_structure_queries, vec![base]);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_base_structures, vec![Some(StructureId::new(2))]);
        assert_eq!(host.probed_stored_values, vec![stored_value]);
        assert_eq!(host.probed_plan_keys, vec![property_cache_key(11)]);
        assert!(probe_miss_records.is_empty());
        assert!(rejection_records.is_empty());
        assert_eq!(commit_records.len(), 1);
        assert_eq!(commit_records[0].store_plan_ordinal, 2);
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
    }

    #[test]
    fn baseline_generated_property_store_put_by_value_sidecar_rereads_runtime_key() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByValue,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let candidate = property_store_mutation_candidate(
            property_store_indexed_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                0,
                StructureId::new(1),
            ),
            1,
        );
        let table = property_store_mutation_candidate_table(owner(), vec![candidate.clone()]);
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let runtime_key = RuntimeValue::from_i32(1);
        let mut host = SequencedPropertyStoreMutationHost::default();

        let (result, stack, registers, probe_miss_records, rejection_records, commit_records) =
            execute_generated_with_property_store_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[
                    (local(0), base),
                    (local(1), runtime_key),
                    (local(2), stored_value),
                ],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(stored_value)
            ))
        );
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.probed_stored_values, vec![stored_value]);
        assert_eq!(host.probed_plan_keys, vec![indexed_property_cache_key(1)]);
        assert_eq!(host.committed_base_values, vec![base]);
        assert_eq!(host.committed_keys, vec![indexed_property_cache_key(1)]);
        assert_eq!(host.committed_stored_values, vec![stored_value]);
        assert!(probe_miss_records.is_empty());
        assert!(rejection_records.is_empty());
        assert_eq!(commit_records.len(), 1);
        assert_eq!(commit_records[0].key, indexed_property_cache_key(0));
        assert_eq!(commit_records[0].commit.key, indexed_property_cache_key(1));
        assert_eq!(
            commit_records[0].plan_kind,
            PropertyStoreAccessCasePlanKind::DataOnlyIndexedStore
        );
    }

    #[test]
    fn baseline_generated_property_store_mutation_rejection_falls_back_and_records_rejection() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let candidate = property_store_mutation_candidate(
            property_store_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            ),
            1,
        );
        let table = property_store_mutation_candidate_table(owner(), vec![candidate]);
        let base = cell_runtime_value();
        let stored_value = RuntimeValue::from_i32(99);
        let mut host = SequencedPropertyStoreMutationHost::new(
            Vec::new(),
            vec![GeneratedPropertyStoreMutationResult::rejected(
                GeneratedPropertyStoreMutationMissReason::BarrierRejected,
            )],
        );

        let (result, stack, registers, probe_miss_records, rejection_records, commit_records) =
            execute_generated_with_property_store_sidecar(
                owner(),
                &block,
                &artifact,
                &table,
                &mut host,
                &[(local(0), base), (local(2), stored_value)],
            );
        let frame = stack.top_frame().unwrap();
        let site = property_store_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::PutByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
        assert_eq!(host.probed_base_values, vec![base]);
        assert_eq!(host.committed_base_values, vec![base]);
        assert!(probe_miss_records.is_empty());
        assert!(commit_records.is_empty());
        assert_eq!(
            rejection_records,
            vec![BaselineGeneratedPropertyStoreMutationRejectionRecord {
                owner: owner(),
                bytecode_index: BytecodeIndex::from_offset(0),
                slot: InlineCacheSlotId(0),
                key: property_cache_key(11),
                plan_kind: PropertyStoreAccessCasePlanKind::DataOnlyReplace,
                base_structure: Some(StructureId::new(1)),
                planned_new_structure: None,
                offset: Some(PropertyOffset::new(0)),
                store_plan_ordinal: 1,
                readiness_ordinal: 301,
                stored_value_kind: ValueKind::Int32,
                reason: GeneratedPropertyStoreMutationMissReason::BarrierRejected,
            }]
        );
    }

    #[test]
    fn baseline_generated_property_store_no_sidecar_leaves_put_by_name_handoff_unchanged() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(2)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let stored_value = RuntimeValue::from_i32(99);
        let (result, stack, registers) = execute_generated_with_initial_locals(
            owner(),
            &block,
            &artifact,
            &[(local(0), cell_runtime_value()), (local(2), stored_value)],
        );
        let frame = stack.top_frame().unwrap();
        let site = property_store_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::PutByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 2), stored_value);
    }

    #[test]
    fn baseline_generated_property_store_sidecar_remains_record_only_without_vm_wiring() {
        let source = include_str!("baseline.rs");
        for forbidden in [
            concat!("VmGeneratedPropertyStore", "MutationReadiness"),
            concat!("record_generated_", "property_store"),
            concat!("property_store_mutation_candidate_table_", "for_owner"),
            concat!("generated_property_store_", "entry"),
            concat!("select_generated_", "property_store"),
            concat!("install_generated_", "property_store"),
            concat!("CodeBlockMutationAuthority::", "VmMainThread"),
        ] {
            assert!(
                !source.contains(forbidden),
                "unexpected VM/generated store wiring found: {forbidden}"
            );
        }
    }

    #[test]
    fn generated_guarded_property_load_prototype_data_hit_records_destination_root_sync() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let plan_table = property_load_plan_table(owner(), Vec::new());
        let candidate =
            prototype_data_guarded_candidate(owner(), BytecodeIndex::from_offset(0), 11, 1);
        let guarded_table = property_load_guarded_candidate_table(owner(), vec![candidate]);
        let cell_value = cell_runtime_value();
        let mut host = SequencedPropertyLoadProbeHost::new_guarded(vec![
            GeneratedGuardedPropertyLoadProbeResult::hit(cell_value),
        ]);

        let (result, stack, _, root_sync_requests, probe_miss_records, guarded_miss_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner(),
                &block,
                &artifact,
                &plan_table,
                &guarded_table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );
        let frame = stack.top_frame().unwrap();

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(cell_value)
            ))
        );
        assert!(host.probed_plan_keys.is_empty());
        assert_eq!(host.guarded_probed_plan_keys, vec![property_cache_key(11)]);
        assert_eq!(
            root_sync_requests,
            vec![BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                frame: frame.id,
                bytecode_index: BytecodeIndex::from_offset(0),
                destination: local(0),
            }]
        );
        assert!(probe_miss_records.is_empty());
        assert!(guarded_miss_records.is_empty());
    }

    #[test]
    fn generated_guarded_property_load_negative_lookup_hit_writes_undefined() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let plan_table = property_load_plan_table(owner(), Vec::new());
        let candidate =
            negative_lookup_guarded_candidate(owner(), BytecodeIndex::from_offset(0), 11, 1);
        let guarded_table = property_load_guarded_candidate_table(owner(), vec![candidate]);
        let mut host = SequencedPropertyLoadProbeHost::new_guarded(vec![
            GeneratedGuardedPropertyLoadProbeResult::hit(RuntimeValue::undefined()),
        ]);

        let (result, _, _, root_sync_requests, probe_miss_records, guarded_miss_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner(),
                &block,
                &artifact,
                &plan_table,
                &guarded_table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::undefined())
            ))
        );
        assert!(host.probed_plan_keys.is_empty());
        assert_eq!(host.guarded_probed_plan_keys, vec![property_cache_key(11)]);
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert!(guarded_miss_records.is_empty());
    }

    #[test]
    fn generated_guarded_property_load_miss_keeps_property_handoff_and_records_metadata() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let plan_table = property_load_plan_table(owner(), Vec::new());
        let candidate =
            prototype_data_guarded_candidate(owner(), BytecodeIndex::from_offset(0), 11, 1);
        let guarded_table = property_load_guarded_candidate_table(owner(), vec![candidate.clone()]);
        let initial_destination = RuntimeValue::from_i32(17);
        let mut host = SequencedPropertyLoadProbeHost::default();

        let (result, stack, registers, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner(),
                &block,
                &artifact,
                &plan_table,
                &guarded_table,
                &mut host,
                &[
                    (local(0), initial_destination),
                    (local(1), cell_runtime_value()),
                ],
            );
        let frame = stack.top_frame().unwrap();
        let site = property_handoff_site(owner(), BytecodeIndex::from_offset(0), 11);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(
                BaselineGeneratedPropertyHandoff {
                    resume: BaselineGeneratedPropertyResume {
                        owner: owner(),
                        frame: frame.id,
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::GetByName,
                    },
                    site,
                    requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
                    may_throw: site.may_throw,
                }
            ))
        );
        assert_eq!(read_local(&registers, &stack, 0), initial_destination);
        assert!(host.probed_plan_keys.is_empty());
        assert_eq!(host.guarded_probed_plan_keys, vec![property_cache_key(11)]);
        assert!(root_sync_requests.is_empty());
        assert!(probe_miss_records.is_empty());
        assert_eq!(
            guarded_records,
            vec![BaselineGeneratedGuardedPropertyLoadProbeMissRecord {
                owner: owner(),
                bytecode_index: BytecodeIndex::from_offset(0),
                slot: candidate.plan.slot,
                guard_plan_ordinal: candidate.guard_plan_ordinal,
                materialization_ordinal: candidate.materialization_ordinal,
                dependency_ordinals: candidate.dependency_ordinals,
                binding_set_ids: candidate.binding_set_ids,
                candidate_kind: candidate.candidate_kind,
                base_structure: candidate.plan.descriptor.base_structure,
                reason: GeneratedGuardedPropertyLoadProbeMissReason::HostUnavailable,
                requirement: candidate.plan.descriptor.requirement,
                key: property_cache_key(11),
                prototype_depth: candidate.plan.descriptor.prototype_depth,
                chain_index: None,
                outcome: candidate.plan.descriptor.chain.outcome,
            }]
        );
    }

    #[test]
    fn generated_guarded_property_load_own_data_hit_does_not_call_guarded_probe() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let plan_table = property_load_plan_table(
            owner(),
            vec![property_load_plan(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                StructureId::new(1),
                PropertyOffset::new(0),
            )],
        );
        let guarded_table = property_load_guarded_candidate_table(
            owner(),
            vec![prototype_data_guarded_candidate(
                owner(),
                BytecodeIndex::from_offset(0),
                11,
                1,
            )],
        );
        let mut host =
            SequencedPropertyLoadProbeHost::new(vec![GeneratedPropertyLoadProbeResult::hit(
                RuntimeValue::from_i32(42),
            )]);

        let (result, _, _, _, _, guarded_miss_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner(),
                &block,
                &artifact,
                &plan_table,
                &guarded_table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
            ))
        );
        assert_eq!(host.probed_plan_keys, vec![property_cache_key(11)]);
        assert!(host.guarded_probed_plan_keys.is_empty());
        assert!(guarded_miss_records.is_empty());
    }

    #[test]
    fn generated_guarded_property_load_owner_key_and_bytecode_mismatch_skip_candidates() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = mixed_artifact_for_block(owner(), &block);
        let plan_table = property_load_plan_table(owner(), Vec::new());
        let owner_mismatch_table = property_load_guarded_candidate_table(
            other_owner(),
            vec![prototype_data_guarded_candidate(
                other_owner(),
                BytecodeIndex::from_offset(0),
                11,
                1,
            )],
        );
        let mut host = SequencedPropertyLoadProbeHost::new_guarded(vec![
            GeneratedGuardedPropertyLoadProbeResult::hit(RuntimeValue::from_i32(99)),
        ]);

        let (result, _, _, _, _, guarded_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner(),
                &block,
                &artifact,
                &plan_table,
                &owner_mismatch_table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );

        assert!(matches!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(_))
        ));
        assert!(host.guarded_probed_plan_keys.is_empty());
        assert!(guarded_records.is_empty());

        let mismatch_table = property_load_guarded_candidate_table(
            owner(),
            vec![
                prototype_data_guarded_candidate(owner(), BytecodeIndex::from_offset(0), 12, 1),
                negative_lookup_guarded_candidate(owner(), BytecodeIndex::from_offset(9), 11, 2),
            ],
        );
        let mut host = SequencedPropertyLoadProbeHost::new_guarded(vec![
            GeneratedGuardedPropertyLoadProbeResult::hit(RuntimeValue::from_i32(99)),
        ]);

        let (result, _, _, _, _, guarded_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner(),
                &block,
                &artifact,
                &plan_table,
                &mismatch_table,
                &mut host,
                &[(local(1), cell_runtime_value())],
            );

        assert!(matches!(
            result,
            Ok(BaselineGeneratedExecutionResult::Property(_))
        ));
        assert!(host.probed_plan_keys.is_empty());
        assert!(host.guarded_probed_plan_keys.is_empty());
        assert!(guarded_records.is_empty());
    }

    fn artifact_for_block(
        owner: CodeBlockId,
        code_block: &CodeBlock,
    ) -> BaselineGeneratedCodeArtifact {
        artifact_for_block_with_subset(
            owner,
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
        )
    }

    fn artifact_for_block_with_subset(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        opcode_subset: BaselineSupportedOpcodeSubset,
    ) -> BaselineGeneratedCodeArtifact {
        let proof = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            code_block,
            owner,
            tiering_snapshot(owner),
            opcode_subset,
            Vec::new(),
        )
        .unwrap();
        BaselineGeneratedCodeArtifact::new(
            JitCodeId(101),
            owner,
            proof,
            BaselineGeneratedCodeBody::new(BaselineGeneratedCodeBodyId(202), opcode_subset),
            CodeLiveness::Live,
            CodeFinalizationAuthority::MainThread,
        )
        .unwrap()
    }

    fn mixed_artifact_for_block(
        owner: CodeBlockId,
        code_block: &CodeBlock,
    ) -> BaselineGeneratedCodeArtifact {
        let opcode_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary;
        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_for_mixed_vm_install(
                code_block,
                owner,
                tiering_snapshot(owner),
                opcode_subset,
                Vec::new(),
                None,
            )
            .unwrap();
        BaselineGeneratedCodeArtifact::new(
            JitCodeId(101),
            owner,
            proof,
            BaselineGeneratedCodeBody::new(BaselineGeneratedCodeBodyId(202), opcode_subset),
            CodeLiveness::Live,
            CodeFinalizationAuthority::MainThread,
        )
        .unwrap()
    }

    fn artifact_for_block_with_runtime_helper_metadata(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        opcode_subset: BaselineSupportedOpcodeSubset,
        runtime_helper_plan: BaselineGeneratedRuntimeHelperPlanMetadata,
    ) -> BaselineGeneratedCodeArtifact {
        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                code_block,
                owner,
                tiering_snapshot(owner),
                opcode_subset,
                Vec::new(),
                &runtime_helper_plan,
            )
            .unwrap();
        BaselineGeneratedCodeArtifact::new_with_runtime_helper_plan(
            JitCodeId(101),
            owner,
            proof,
            BaselineGeneratedCodeBody::new(BaselineGeneratedCodeBodyId(202), opcode_subset),
            runtime_helper_plan,
            CodeLiveness::Live,
            CodeFinalizationAuthority::MainThread,
        )
        .unwrap()
    }

    fn tiering_snapshot(owner: CodeBlockId) -> TieringSnapshot {
        TieringSnapshot {
            owner,
            from_tier: JitType::None,
            to_tier: JitType::Baseline,
            trigger: TieringTrigger::EntryCounter,
            counters: Default::default(),
            osr_entry_bytecode_index: None,
            epoch: 1,
        }
    }

    fn enter_program_frame(
        stack: &mut ExecutionContextStack,
        registers: &mut RegisterFile,
        owner: CodeBlockId,
        code_block: &CodeBlock,
    ) -> CallFrameId {
        stack.enter(ExecutionEntryRecord::Program(ProgramExecutionEntry {
            code_block: owner,
            global_object: GlobalObjectId(ObjectId(CellId(1))),
            this_value: RuntimeValue::undefined(),
        }));
        stack
            .push_frame(
                registers,
                FramePushRequest {
                    code_block: Some(owner),
                    callee: None,
                    callee_value: None,
                    lexical_scope: None,
                    shape: code_block.unlinked().frame(),
                    argument_count_including_this: 1,
                    argument_values: Vec::new(),
                    start_bytecode_index: Some(BytecodeIndex::from_offset(0)),
                    return_bytecode_index: None,
                },
            )
            .unwrap()
    }

    fn execute_generated(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
    ) -> (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
    ) {
        execute_generated_with_initial_locals(owner, code_block, artifact, &[])
    }

    fn execute_generated_with_initial_locals(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
    ) {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }
        let result = execute_baseline_generated_code(BaselineGeneratedExecutionRequest {
            artifact,
            owner,
            code_block,
            expected_frame: frame,
            execution: InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
        });
        (result, stack, registers)
    }

    fn execute_generated_with_frame_callee(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        callee: RuntimeValue,
    ) -> (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
    ) {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        stack.top_frame_mut().unwrap().callee_value = Some(callee);
        let result = execute_baseline_generated_code(BaselineGeneratedExecutionRequest {
            artifact,
            owner,
            code_block,
            expected_frame: frame,
            execution: InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
        });
        (result, stack, registers)
    }

    fn execute_generated_with_property_load_sidecar(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        plan_table: &PropertyLoadAccessCasePlanTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
        Vec<BaselineGeneratedPropertyLoadDestinationRootSyncRequest>,
        Vec<BaselineGeneratedPropertyLoadProbeMissRecord>,
    ) {
        let guarded_candidate_table = empty_property_load_guarded_candidate_table(owner);
        let (result, stack, registers, root_sync_requests, probe_miss_records, guarded_records) =
            execute_generated_with_property_load_sidecar_tables(
                owner,
                code_block,
                artifact,
                plan_table,
                &guarded_candidate_table,
                host,
                initial_locals,
            );
        assert!(guarded_records.is_empty());
        (
            result,
            stack,
            registers,
            root_sync_requests,
            probe_miss_records,
        )
    }

    type PropertyLoadSidecarTablesExecutionResult = (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
        Vec<BaselineGeneratedPropertyLoadDestinationRootSyncRequest>,
        Vec<BaselineGeneratedPropertyLoadProbeMissRecord>,
        Vec<BaselineGeneratedGuardedPropertyLoadProbeMissRecord>,
    );

    fn execute_generated_with_property_load_sidecar_tables(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        plan_table: &PropertyLoadAccessCasePlanTable,
        guarded_candidate_table: &PropertyLoadGuardedCandidateTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> PropertyLoadSidecarTablesExecutionResult {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }

        let result;
        let root_sync_requests;
        let probe_miss_records;
        let guarded_probe_miss_records;
        {
            let mut sidecar = BaselineGeneratedPropertyLoadExecutionSidecar::new(
                plan_table,
                guarded_candidate_table,
                host,
            );
            result = execute_baseline_generated_code_with_property_load_sidecar(
                BaselineGeneratedExecutionRequest {
                    artifact,
                    owner,
                    code_block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecar,
            );
            root_sync_requests = sidecar.destination_root_sync_requests().to_vec();
            probe_miss_records = sidecar.probe_miss_records().to_vec();
            guarded_probe_miss_records = sidecar.guarded_probe_miss_records().to_vec();
        }

        (
            result,
            stack,
            registers,
            root_sync_requests,
            probe_miss_records,
            guarded_probe_miss_records,
        )
    }

    fn execute_generated_with_megamorphic_property_load_sidecar(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        megamorphic_table: &GeneratedPropertyLoadMegamorphicCandidateTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> PropertyLoadSidecarTablesExecutionResult {
        let plan_table = property_load_plan_table(owner, Vec::new());
        let guarded_candidate_table = empty_property_load_guarded_candidate_table(owner);
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }

        let result;
        let root_sync_requests;
        let probe_miss_records;
        let guarded_probe_miss_records;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(
                host,
                Some((&plan_table, &guarded_candidate_table)),
                None,
            )
            .with_property_load_megamorphic_candidate_table(Some(megamorphic_table));
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact,
                    owner,
                    code_block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
            root_sync_requests = sidecars.destination_root_sync_requests().to_vec();
            probe_miss_records = sidecars.property_load_probe_miss_records().to_vec();
            guarded_probe_miss_records =
                sidecars.guarded_property_load_probe_miss_records().to_vec();
        }

        (
            result,
            stack,
            registers,
            root_sync_requests,
            probe_miss_records,
            guarded_probe_miss_records,
        )
    }

    type PropertyStoreSidecarExecutionResult = (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
        Vec<BaselineGeneratedPropertyStoreProbeMissRecord>,
        Vec<BaselineGeneratedPropertyStoreMutationRejectionRecord>,
        Vec<BaselineGeneratedPropertyStoreMutationCommitRecord>,
    );

    fn execute_generated_with_property_store_sidecar(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        candidate_table: &PropertyStoreMutationCandidateTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> PropertyStoreSidecarExecutionResult {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }

        let result;
        let probe_miss_records;
        let mutation_rejection_records;
        let mutation_commit_records;
        {
            let mut sidecar =
                BaselineGeneratedPropertyStoreExecutionSidecar::new(candidate_table, host);
            result = execute_baseline_generated_code_with_property_store_sidecar(
                BaselineGeneratedExecutionRequest {
                    artifact,
                    owner,
                    code_block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecar,
            );
            probe_miss_records = sidecar.probe_miss_records().to_vec();
            mutation_rejection_records = sidecar.mutation_rejection_records().to_vec();
            mutation_commit_records = sidecar.mutation_commit_records().to_vec();
        }

        (
            result,
            stack,
            registers,
            probe_miss_records,
            mutation_rejection_records,
            mutation_commit_records,
        )
    }

    fn execute_generated_with_megamorphic_property_store_sidecar(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        megamorphic_table: &GeneratedPropertyStoreMegamorphicCandidateTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
    ) {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }

        let result;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(host, None, None)
                .with_property_store_megamorphic_candidate_table(Some(megamorphic_table));
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact,
                    owner,
                    code_block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
        }

        (result, stack, registers)
    }

    fn execute_generated_with_megamorphic_property_has_sidecar(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        megamorphic_table: &GeneratedPropertyHasMegamorphicCandidateTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
    ) {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }

        let result;
        {
            let mut sidecars = BaselineGeneratedPropertyExecutionSidecars::new(host, None, None)
                .with_property_has_megamorphic_candidate_table(Some(megamorphic_table));
            result = execute_baseline_generated_code_with_property_sidecars(
                BaselineGeneratedExecutionRequest {
                    artifact,
                    owner,
                    code_block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecars,
            );
        }

        (result, stack, registers)
    }

    type GeneratedCallLinkSidecarExecutionResult = (
        Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
        Vec<BaselineGeneratedCallLinkProbeMissRecord>,
        Vec<BaselineGeneratedCallLinkProbeBlockedRecord>,
    );

    fn execute_generated_with_generated_call_link_sidecar(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        candidate_table: &GeneratedCallLinkCandidateTable,
        host: &mut dyn DispatchHost,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> GeneratedCallLinkSidecarExecutionResult {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }

        let result;
        let probe_miss_records;
        let probe_blocked_records;
        {
            let mut sidecar = BaselineGeneratedCallLinkExecutionSidecar::new(candidate_table, host);
            result = execute_baseline_generated_code_with_generated_call_link_sidecar(
                BaselineGeneratedExecutionRequest {
                    artifact,
                    owner,
                    code_block,
                    expected_frame: frame,
                    execution: InterpreterExecutionState {
                        stack: &mut stack,
                        registers: &mut registers,
                        exceptions: &mut exceptions,
                        heap: &mut heap,
                    },
                },
                &mut sidecar,
            );
            probe_miss_records = sidecar.probe_miss_records().to_vec();
            probe_blocked_records = sidecar.probe_blocked_records().to_vec();
        }

        (
            result,
            stack,
            registers,
            probe_miss_records,
            probe_blocked_records,
        )
    }

    fn execute_generated_with_runtime_helper_table(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        artifact: &BaselineGeneratedCodeArtifact,
        runtime_helper_plan: BaselineGeneratedRuntimeHelperPlan<'_>,
    ) -> (
        Result<BaselineGeneratedExecutionWithRuntimeHelpersResult, BaselineGeneratedExecutionError>,
        ExecutionContextStack,
        RegisterFile,
    ) {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let result = execute_baseline_generated_code_with_runtime_helpers(
            BaselineGeneratedExecutionRequest {
                artifact,
                owner,
                code_block,
                expected_frame: frame,
                execution: InterpreterExecutionState {
                    stack: &mut stack,
                    registers: &mut registers,
                    exceptions: &mut exceptions,
                    heap: &mut heap,
                },
            },
            runtime_helper_plan,
        );
        (result, stack, registers)
    }

    fn runtime_helper_plan_for_block<'proof>(
        code_block: &CodeBlock,
        proofs: &'proof [BaselineGeneratedRuntimeHelperProof],
    ) -> BaselineGeneratedRuntimeHelperPlan<'proof> {
        BaselineGeneratedRuntimeHelperPlan::new(
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(code_block).unwrap(),
            proofs,
        )
    }

    fn runtime_helper_metadata_for_block(
        code_block: &CodeBlock,
        proofs: Vec<BaselineGeneratedRuntimeHelperProof>,
    ) -> BaselineGeneratedRuntimeHelperPlanMetadata {
        BaselineGeneratedRuntimeHelperPlanMetadata::from_code_block_snapshot(code_block, proofs)
            .unwrap()
    }

    fn execute_interpreter(
        owner: CodeBlockId,
        code_block: &CodeBlock,
    ) -> (ExecutionCompletion, ExecutionContextStack, RegisterFile) {
        execute_interpreter_with_initial_locals(owner, code_block, &[])
    }

    fn execute_interpreter_with_initial_locals(
        owner: CodeBlockId,
        code_block: &CodeBlock,
        initial_locals: &[(VirtualRegister, RuntimeValue)],
    ) -> (ExecutionCompletion, ExecutionContextStack, RegisterFile) {
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        enter_program_frame(&mut stack, &mut registers, owner, code_block);
        let window = stack.top_frame().unwrap().register_window;
        for (register, value) in initial_locals {
            registers.write(window, *register, *value).unwrap();
        }
        let mut host = CoreOpcodeDispatchHost::new();
        let result = execute_code_block(
            InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
            owner,
            code_block,
            &mut host,
            DispatchConfig::default(),
        );
        (result, stack, registers)
    }

    fn cell_runtime_value() -> RuntimeValue {
        RuntimeValue::from_encoded(
            static_value_representation_layout()
                .encode_cell_payload(0x1234)
                .unwrap(),
        )
    }

    fn unknown_runtime_value() -> RuntimeValue {
        let value = RuntimeValue::from_encoded(EncodedJsValue(0xff));
        assert_eq!(value.kind(), ValueKind::Unknown);
        value
    }

    fn read_local(
        registers: &RegisterFile,
        stack: &ExecutionContextStack,
        index: u32,
    ) -> RuntimeValue {
        let frame = stack.top_frame().unwrap();
        registers
            .read(frame.register_window, local(index), None)
            .unwrap()
    }

    fn jump_if_false_block() -> CodeBlock {
        code_block(vec![
            core_typed(
                0,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(3)),
                ],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            core_typed(
                3,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(22)],
            ),
            core_typed(4, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn primitive_truthiness_artifact(code_block: &CodeBlock) -> BaselineGeneratedCodeArtifact {
        artifact_for_block_with_subset(
            owner(),
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness,
        )
    }

    fn primitive_boolean_artifact(code_block: &CodeBlock) -> BaselineGeneratedCodeArtifact {
        artifact_for_block_with_subset(
            owner(),
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean,
        )
    }

    fn primitive_number_artifact(code_block: &CodeBlock) -> BaselineGeneratedCodeArtifact {
        artifact_for_block_with_subset(
            owner(),
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber,
        )
    }

    fn primitive_to_number_void_artifact(code_block: &CodeBlock) -> BaselineGeneratedCodeArtifact {
        artifact_for_block_with_subset(
            owner(),
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid,
        )
    }

    fn pure_number_binary_artifact(code_block: &CodeBlock) -> BaselineGeneratedCodeArtifact {
        artifact_for_block_with_subset(
            owner(),
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
        )
    }

    fn is_returned_double_nan(completion: &ExecutionCompletion) -> bool {
        matches!(
            completion,
            ExecutionCompletion::Returned(value)
                if matches!(value.as_number(), Some(NumberValue::DoubleBits(bits)) if bits.to_f64().is_nan())
        )
    }

    fn assert_generated_fallback(
        result: &Result<BaselineGeneratedExecutionResult, BaselineGeneratedExecutionError>,
        request: BaselineFallbackRequest,
        reason: BaselineGeneratedFallbackReason,
    ) {
        assert_eq!(
            result,
            &Ok(BaselineGeneratedExecutionResult::Fallback(
                BaselineGeneratedFallback { request, reason }
            ))
        );
    }

    fn core_fallback_reason(
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        cause: BaselineGeneratedFallbackCause,
    ) -> BaselineGeneratedFallbackReason {
        BaselineGeneratedFallbackReason {
            bytecode_index,
            opcode: BaselineGeneratedFallbackOpcode::Core(opcode),
            cause,
        }
    }

    fn non_core_fallback_reason(
        bytecode_index: BytecodeIndex,
        opcode: Opcode,
        cause: BaselineGeneratedFallbackCause,
    ) -> BaselineGeneratedFallbackReason {
        BaselineGeneratedFallbackReason {
            bytecode_index,
            opcode: BaselineGeneratedFallbackOpcode::NonCore(opcode),
            cause,
        }
    }

    fn new_object_block() -> CodeBlock {
        code_block(vec![
            core_typed(0, CoreOpcode::NewObject, vec![Operand::Register(local(0))]),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    fn new_array_block() -> CodeBlock {
        code_block(vec![
            core_typed(0, CoreOpcode::NewArray, vec![Operand::Register(local(0))]),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    fn type_of_block() -> CodeBlock {
        code_block(vec![
            core_typed(
                0,
                CoreOpcode::TypeOf,
                vec![Operand::Register(local(0)), Operand::Register(local(1))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    fn load_string_block() -> CodeBlock {
        code_block_with_string_literals(
            vec![
                core_typed(
                    0,
                    CoreOpcode::LoadString,
                    vec![Operand::Register(local(0)), Operand::IdentifierIndex(9)],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            ],
            vec![(9, "generated string".to_string())],
        )
    }

    fn load_bigint_block() -> CodeBlock {
        code_block_with_string_literals(
            vec![
                core_typed(
                    0,
                    CoreOpcode::LoadBigInt,
                    vec![Operand::Register(local(0)), Operand::IdentifierIndex(10)],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            ],
            vec![(10, "12345678901234567890n".to_string())],
        )
    }

    fn new_object_shadow_artifact() -> BaselineGeneratedCodeArtifact {
        let shadow = load_undefined_return_block();
        artifact_for_block(owner(), &shadow)
    }

    fn load_undefined_return_block() -> CodeBlock {
        code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadUndefined,
                vec![Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    #[test]
    fn generated_instruction_without_runtime_helper_table_falls_back_for_new_object() {
        let block = code_block(Vec::new());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let (_, window) = baseline_active_frame(&stack, frame, owner()).unwrap();
        let mut execution = InterpreterExecutionState {
            stack: &mut stack,
            registers: &mut registers,
            exceptions: &mut exceptions,
            heap: &mut heap,
        };
        let bytecode_index = BytecodeIndex::from_offset(0);
        let operands = [Operand::Register(local(0))];
        let instruction = DecodedInstruction {
            opcode: CoreOpcode::NewObject.opcode(),
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &operands,
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };

        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: core_fallback_reason(
                        bytecode_index,
                        CoreOpcode::NewObject,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );
    }

    #[test]
    fn generated_instruction_without_runtime_helper_table_falls_back_for_load_string() {
        let block = code_block(Vec::new());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let (_, window) = baseline_active_frame(&stack, frame, owner()).unwrap();
        let mut execution = InterpreterExecutionState {
            stack: &mut stack,
            registers: &mut registers,
            exceptions: &mut exceptions,
            heap: &mut heap,
        };
        let bytecode_index = BytecodeIndex::from_offset(0);
        let operands = [Operand::Register(local(0)), Operand::IdentifierIndex(9)];
        let instruction = DecodedInstruction {
            opcode: CoreOpcode::LoadString.opcode(),
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &operands,
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };

        assert!(
            !BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary
                .supports(CoreOpcode::LoadString)
        );
        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: core_fallback_reason(
                        bytecode_index,
                        CoreOpcode::LoadString,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );
    }

    #[test]
    fn generated_instruction_without_runtime_helper_table_falls_back_for_load_bigint() {
        let block = code_block(Vec::new());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let (_, window) = baseline_active_frame(&stack, frame, owner()).unwrap();
        let mut execution = InterpreterExecutionState {
            stack: &mut stack,
            registers: &mut registers,
            exceptions: &mut exceptions,
            heap: &mut heap,
        };
        let bytecode_index = BytecodeIndex::from_offset(0);
        let operands = [Operand::Register(local(0)), Operand::IdentifierIndex(10)];
        let instruction = DecodedInstruction {
            opcode: CoreOpcode::LoadBigInt.opcode(),
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &operands,
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };

        assert!(
            !BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary
                .supports(CoreOpcode::LoadBigInt)
        );
        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: core_fallback_reason(
                        bytecode_index,
                        CoreOpcode::LoadBigInt,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_returns_handoff_for_new_object_proof() {
        let block = new_object_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = new_object_runtime_boundary_proof_at(bytecode_index);
        let metadata = runtime_helper_metadata_for_block(
            &block,
            vec![BaselineGeneratedRuntimeHelperProof::new(
                bytecode_index,
                proof,
            )],
        );
        let artifact = artifact_for_block_with_runtime_helper_metadata(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            metadata,
        );
        let (result, stack, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            artifact.runtime_helper_plan().unwrap(),
        );
        let frame = stack.top_frame().unwrap().id;

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(
                    BaselineGeneratedRuntimeHelperHandoff {
                        resume: BaselineGeneratedRuntimeHelperResume {
                            owner: owner(),
                            frame,
                            bytecode_index,
                            opcode: CoreOpcode::NewObject,
                        },
                        safepoint: CompilerSafepointId(7),
                        root_map: BytecodeRootMapId(42),
                        root_count: 1,
                        requires_no_gc_exit_reentry: true,
                        may_throw: true,
                    }
                )
            )
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_returns_handoff_for_new_array_proof() {
        let block = new_array_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = new_array_runtime_boundary_proof_at(bytecode_index);
        let metadata = runtime_helper_metadata_for_block(
            &block,
            vec![BaselineGeneratedRuntimeHelperProof::new(
                bytecode_index,
                proof,
            )],
        );
        let artifact = artifact_for_block_with_runtime_helper_metadata(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            metadata,
        );
        let (result, stack, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            artifact.runtime_helper_plan().unwrap(),
        );
        let frame = stack.top_frame().unwrap().id;

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(
                    BaselineGeneratedRuntimeHelperHandoff {
                        resume: BaselineGeneratedRuntimeHelperResume {
                            owner: owner(),
                            frame,
                            bytecode_index,
                            opcode: CoreOpcode::NewArray,
                        },
                        safepoint: CompilerSafepointId(7),
                        root_map: BytecodeRootMapId(42),
                        root_count: 1,
                        requires_no_gc_exit_reentry: true,
                        may_throw: true,
                    }
                )
            )
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_returns_handoff_for_typeof_proof() {
        let block = type_of_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = type_of_runtime_boundary_proof_at(bytecode_index);
        let metadata = runtime_helper_metadata_for_block(
            &block,
            vec![BaselineGeneratedRuntimeHelperProof::new(
                bytecode_index,
                proof,
            )],
        );
        let artifact = artifact_for_block_with_runtime_helper_metadata(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            metadata,
        );
        let (result, stack, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            artifact.runtime_helper_plan().unwrap(),
        );
        let frame = stack.top_frame().unwrap().id;

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(
                    BaselineGeneratedRuntimeHelperHandoff {
                        resume: BaselineGeneratedRuntimeHelperResume {
                            owner: owner(),
                            frame,
                            bytecode_index,
                            opcode: CoreOpcode::TypeOf,
                        },
                        safepoint: CompilerSafepointId(7),
                        root_map: BytecodeRootMapId(42),
                        root_count: 2,
                        requires_no_gc_exit_reentry: true,
                        may_throw: true,
                    }
                )
            )
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_returns_handoff_for_load_string_proof() {
        let block = load_string_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = runtime_boundary_proof(CoreOpcode::LoadString, bytecode_index);
        let metadata = runtime_helper_metadata_for_block(
            &block,
            vec![BaselineGeneratedRuntimeHelperProof::new(
                bytecode_index,
                proof,
            )],
        );
        let artifact = artifact_for_block_with_runtime_helper_metadata(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            metadata,
        );
        let (result, stack, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            artifact.runtime_helper_plan().unwrap(),
        );
        let frame = stack.top_frame().unwrap().id;

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(
                    BaselineGeneratedRuntimeHelperHandoff {
                        resume: BaselineGeneratedRuntimeHelperResume {
                            owner: owner(),
                            frame,
                            bytecode_index,
                            opcode: CoreOpcode::LoadString,
                        },
                        safepoint: CompilerSafepointId(7),
                        root_map: BytecodeRootMapId(42),
                        root_count: 1,
                        requires_no_gc_exit_reentry: true,
                        may_throw: true,
                    }
                )
            )
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_returns_handoff_for_load_bigint_proof() {
        let block = load_bigint_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = runtime_boundary_proof(CoreOpcode::LoadBigInt, bytecode_index);
        let metadata = runtime_helper_metadata_for_block(
            &block,
            vec![BaselineGeneratedRuntimeHelperProof::new(
                bytecode_index,
                proof,
            )],
        );
        let artifact = artifact_for_block_with_runtime_helper_metadata(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            metadata,
        );
        let (result, stack, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            artifact.runtime_helper_plan().unwrap(),
        );
        let frame = stack.top_frame().unwrap().id;

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::RuntimeHelper(
                    BaselineGeneratedRuntimeHelperHandoff {
                        resume: BaselineGeneratedRuntimeHelperResume {
                            owner: owner(),
                            frame,
                            bytecode_index,
                            opcode: CoreOpcode::LoadBigInt,
                        },
                        safepoint: CompilerSafepointId(7),
                        root_map: BytecodeRootMapId(42),
                        root_count: 1,
                        requires_no_gc_exit_reentry: true,
                        may_throw: true,
                    }
                )
            )
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_rejects_invalid_and_mismatched_proofs() {
        let block = new_object_block();
        let artifact = new_object_shadow_artifact();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let mut invalid = new_object_runtime_boundary_proof_at(bytecode_index);
        invalid.no_gc_exit_reentry = false;
        let invalid_proofs = [BaselineGeneratedRuntimeHelperProof::new(
            bytecode_index,
            invalid,
        )];
        let invalid_plan = runtime_helper_plan_for_block(&block, &invalid_proofs);

        let (invalid_result, _, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, invalid_plan);

        assert_eq!(
            invalid_result,
            Err(BaselineGeneratedExecutionError::RuntimeHelperHandoff {
                bytecode_index,
                opcode: BaselineGeneratedFallbackOpcode::Core(CoreOpcode::NewObject),
                error: BaselineGeneratedRuntimeHelperHandoffError::MissingNoGcExitReentry {
                    opcode: CoreOpcode::NewObject,
                },
            })
        );

        let new_array = runtime_boundary_proof(CoreOpcode::NewArray, bytecode_index);
        let mismatched_proofs = [BaselineGeneratedRuntimeHelperProof::new(
            bytecode_index,
            new_array,
        )];
        let mismatched_plan = runtime_helper_plan_for_block(&block, &mismatched_proofs);
        let (mismatched_result, _, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            mismatched_plan,
        );

        assert_eq!(
            mismatched_result,
            Err(BaselineGeneratedExecutionError::RuntimeHelperHandoff {
                bytecode_index,
                opcode: BaselineGeneratedFallbackOpcode::Core(CoreOpcode::NewObject),
                error: BaselineGeneratedRuntimeHelperHandoffError::OpcodeMismatch {
                    instruction: CoreOpcode::NewObject,
                    proof: CoreOpcode::NewArray,
                },
            })
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_rejects_new_array_mismatched_proof() {
        let block = new_array_block();
        let artifact = new_object_shadow_artifact();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = new_object_runtime_boundary_proof_at(bytecode_index);
        let mismatched_proofs = [BaselineGeneratedRuntimeHelperProof::new(
            bytecode_index,
            proof,
        )];
        let mismatched_plan = runtime_helper_plan_for_block(&block, &mismatched_proofs);

        let (result, _, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            mismatched_plan,
        );

        assert_eq!(
            result,
            Err(BaselineGeneratedExecutionError::RuntimeHelperHandoff {
                bytecode_index,
                opcode: BaselineGeneratedFallbackOpcode::Core(CoreOpcode::NewArray),
                error: BaselineGeneratedRuntimeHelperHandoffError::OpcodeMismatch {
                    instruction: CoreOpcode::NewArray,
                    proof: CoreOpcode::NewObject,
                },
            })
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_rejects_typeof_mismatched_proof() {
        let block = type_of_block();
        let artifact = new_object_shadow_artifact();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = new_object_runtime_boundary_proof_at(bytecode_index);
        let mismatched_proofs = [BaselineGeneratedRuntimeHelperProof::new(
            bytecode_index,
            proof,
        )];
        let mismatched_plan = runtime_helper_plan_for_block(&block, &mismatched_proofs);

        let (result, _, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            mismatched_plan,
        );

        assert_eq!(
            result,
            Err(BaselineGeneratedExecutionError::RuntimeHelperHandoff {
                bytecode_index,
                opcode: BaselineGeneratedFallbackOpcode::Core(CoreOpcode::TypeOf),
                error: BaselineGeneratedRuntimeHelperHandoffError::OpcodeMismatch {
                    instruction: CoreOpcode::TypeOf,
                    proof: CoreOpcode::NewObject,
                },
            })
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_falls_back_when_proof_absent() {
        let block = new_object_block();
        let artifact = new_object_shadow_artifact();
        let empty: [BaselineGeneratedRuntimeHelperProof; 0] = [];
        let plan = runtime_helper_plan_for_block(&block, &empty);
        let (result, stack, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, plan);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(
                    BaselineGeneratedFallback {
                        request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                        reason: core_fallback_reason(
                            bytecode_index,
                            CoreOpcode::NewObject,
                            BaselineGeneratedFallbackCause::UnsupportedOpcode,
                        ),
                    }
                )
            )
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_falls_back_when_new_array_proof_absent() {
        let block = new_array_block();
        let artifact = new_object_shadow_artifact();
        let empty: [BaselineGeneratedRuntimeHelperProof; 0] = [];
        let plan = runtime_helper_plan_for_block(&block, &empty);
        let (result, stack, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, plan);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(
                    BaselineGeneratedFallback {
                        request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                        reason: core_fallback_reason(
                            bytecode_index,
                            CoreOpcode::NewArray,
                            BaselineGeneratedFallbackCause::UnsupportedOpcode,
                        ),
                    }
                )
            )
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_falls_back_when_typeof_proof_absent() {
        let block = type_of_block();
        let artifact = new_object_shadow_artifact();
        let empty: [BaselineGeneratedRuntimeHelperProof; 0] = [];
        let plan = runtime_helper_plan_for_block(&block, &empty);
        let (result, stack, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, plan);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(
                    BaselineGeneratedFallback {
                        request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                        reason: core_fallback_reason(
                            bytecode_index,
                            CoreOpcode::TypeOf,
                            BaselineGeneratedFallbackCause::UnsupportedOpcode,
                        ),
                    }
                )
            )
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_falls_back_when_load_string_proof_absent() {
        let block = load_string_block();
        let artifact = new_object_shadow_artifact();
        let empty: [BaselineGeneratedRuntimeHelperProof; 0] = [];
        let plan = runtime_helper_plan_for_block(&block, &empty);
        let (result, stack, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, plan);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(
                    BaselineGeneratedFallback {
                        request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                        reason: core_fallback_reason(
                            bytecode_index,
                            CoreOpcode::LoadString,
                            BaselineGeneratedFallbackCause::UnsupportedOpcode,
                        ),
                    }
                )
            )
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_falls_back_when_load_bigint_proof_absent() {
        let block = load_bigint_block();
        let artifact = new_object_shadow_artifact();
        let empty: [BaselineGeneratedRuntimeHelperProof; 0] = [];
        let plan = runtime_helper_plan_for_block(&block, &empty);
        let (result, stack, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, plan);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::Fallback(
                    BaselineGeneratedFallback {
                        request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                        reason: core_fallback_reason(
                            bytecode_index,
                            CoreOpcode::LoadBigInt,
                            BaselineGeneratedFallbackCause::UnsupportedOpcode,
                        ),
                    }
                )
            )
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_rejects_stale_helper_plan_snapshot() {
        let block = new_object_block();
        let stale = load_undefined_return_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = new_object_runtime_boundary_proof_at(bytecode_index);
        let stale_metadata = runtime_helper_metadata_for_block(
            &stale,
            vec![BaselineGeneratedRuntimeHelperProof::new(
                bytecode_index,
                proof,
            )],
        );
        let artifact = artifact_for_block_with_runtime_helper_metadata(
            owner(),
            &stale,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            stale_metadata,
        );

        let (result, _, _) = execute_generated_with_runtime_helper_table(
            owner(),
            &block,
            &artifact,
            artifact.runtime_helper_plan().unwrap(),
        );

        assert!(matches!(
            result,
            Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch { .. })
        ));
    }

    #[test]
    fn owned_runtime_helper_metadata_rejects_duplicate_bytecode_proofs() {
        let block = new_object_block();
        let bytecode_index = BytecodeIndex::from_offset(0);
        let proof = new_object_runtime_boundary_proof_at(bytecode_index);

        assert_eq!(
            BaselineGeneratedRuntimeHelperPlanMetadata::from_code_block_snapshot(
                &block,
                vec![
                    BaselineGeneratedRuntimeHelperProof::new(bytecode_index, proof),
                    BaselineGeneratedRuntimeHelperProof::new(bytecode_index, proof),
                ],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanDuplicateProof {
                    bytecode_index,
                }
            )
        );
    }

    #[test]
    fn explicit_runtime_helper_entrypoint_executes_current_no_heap_subset_normally() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(41)],
            ),
            core_typed(
                1,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(0)),
                    Operand::Register(local(0)),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);
        let empty: [BaselineGeneratedRuntimeHelperProof; 0] = [];
        let plan = runtime_helper_plan_for_block(&block, &empty);

        let (result, _, _) =
            execute_generated_with_runtime_helper_table(owner(), &block, &artifact, plan);

        assert_eq!(
            result,
            Ok(
                BaselineGeneratedExecutionWithRuntimeHelpersResult::Completed(
                    ExecutionCompletion::Returned(RuntimeValue::from_i32(82))
                )
            )
        );
    }

    #[test]
    fn plain_generated_execution_remains_independent_of_runtime_helper_metadata() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(13)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);

        let (result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                ExecutionCompletion::Returned(RuntimeValue::from_i32(13))
            ))
        );
    }

    #[test]
    fn constants_move_and_return_match_interpreter() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadUndefined,
                vec![Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::LoadNull, vec![Operand::Register(local(1))]),
            core_typed(
                2,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(2)), Operand::UnsignedImmediate(1)],
            ),
            core_typed(
                3,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                4,
                CoreOpcode::Move,
                vec![Operand::Register(local(4)), Operand::Register(local(3))],
            ),
            core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(4))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);

        let (interpreter_result, interpreter_stack, interpreter_registers) =
            execute_interpreter(owner(), &block);
        let (generated_result, generated_stack, generated_registers) =
            execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(7))
        );
        for index in 0..=4 {
            assert_eq!(
                read_local(&generated_registers, &generated_stack, index),
                read_local(&interpreter_registers, &interpreter_stack, index)
            );
        }
    }

    #[test]
    fn int32_arithmetic_matches_interpreter() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(6)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                2,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(
                3,
                CoreOpcode::SubInt32,
                vec![
                    Operand::Register(local(3)),
                    Operand::Register(local(2)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(
                4,
                CoreOpcode::MulInt32,
                vec![
                    Operand::Register(local(4)),
                    Operand::Register(local(3)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(4))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);

        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(42))
        );
    }

    #[test]
    fn load_double_decodes_low_high_immediates() {
        let value = -40.5f64;
        let bits = value.to_bits();
        let block = code_block(vec![
            load_double_instruction(0, local(0), value),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = primitive_number_artifact(&block);

        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, generated_stack, generated_registers) =
            execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_double(value))
        );
        assert_eq!(
            read_local(&generated_registers, &generated_stack, 0),
            RuntimeValue::from_double(f64::from_bits(bits))
        );
    }

    #[test]
    fn to_number_primitive_cases_match_interpreter() {
        let cases = [
            (
                "int32",
                RuntimeValue::from_i32(-7),
                Some(RuntimeValue::from_i32(-7)),
            ),
            (
                "double",
                RuntimeValue::from_double(-0.0),
                Some(RuntimeValue::from_double(-0.0)),
            ),
            (
                "true",
                RuntimeValue::from_bool(true),
                Some(RuntimeValue::from_i32(1)),
            ),
            (
                "false",
                RuntimeValue::from_bool(false),
                Some(RuntimeValue::from_i32(0)),
            ),
            (
                "null",
                RuntimeValue::null(),
                Some(RuntimeValue::from_i32(0)),
            ),
            ("undefined", RuntimeValue::undefined(), None),
        ];
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::ToNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_to_number_void_artifact(&block);

        for (case_index, (name, value, expected)) in cases.into_iter().enumerate() {
            let initial_locals = [(local(0), value)];
            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            if let Some(expected) = expected {
                assert_eq!(
                    generated_result,
                    Ok(BaselineGeneratedExecutionResult::Completed(
                        interpreter_result.clone()
                    )),
                    "case {case_index}: {name}"
                );
                assert_eq!(
                    interpreter_result,
                    ExecutionCompletion::Returned(expected),
                    "case {case_index}: {name}"
                );
            } else {
                assert!(
                    is_returned_double_nan(&interpreter_result),
                    "case {case_index}: {name}: expected interpreter double NaN"
                );
                assert!(
                    matches!(
                        generated_result,
                        Ok(BaselineGeneratedExecutionResult::Completed(ref completion))
                            if is_returned_double_nan(completion)
                    ),
                    "case {case_index}: {name}: expected generated double NaN"
                );
            }
        }
    }

    #[test]
    fn void_reads_source_and_writes_undefined() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::Void,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_to_number_void_artifact(&block);
        for (name, value) in [
            ("cell", cell_runtime_value()),
            ("unknown", unknown_runtime_value()),
        ] {
            let initial_locals = [(local(0), value)];
            let (generated_result, generated_stack, generated_registers) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    ExecutionCompletion::Returned(RuntimeValue::undefined())
                )),
                "{name}"
            );
            assert_eq!(
                read_local(&generated_registers, &generated_stack, 1),
                RuntimeValue::undefined(),
                "{name}"
            );
        }

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::Void,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(VirtualRegister::argument_or_header(0)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_to_number_void_artifact(&block);
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::Void,
                BaselineGeneratedFallbackCause::RegisterRead {
                    register: VirtualRegister::argument_or_header(0),
                    error: BaselineGeneratedRegisterFallbackCause::CannotAddressHeaderAsValue,
                },
            ),
        );
    }

    #[test]
    fn negate_number_matches_numeric_only_interpreter_results() {
        let cases = [
            ("zero", 0, RuntimeValue::from_double(-0.0)),
            ("normal", 7, RuntimeValue::from_i32(-7)),
            ("min", i32::MIN, RuntimeValue::from_double(2_147_483_648.0)),
        ];

        for (case_index, (name, value, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(value)],
                ),
                core_typed(
                    1,
                    CoreOpcode::NegateNumber,
                    vec![Operand::Register(local(1)), Operand::Register(local(0))],
                ),
                core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            ]);
            let artifact = primitive_number_artifact(&block);

            let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
            let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(expected),
                "case {case_index}: {name}"
            );
        }

        let bits = 40.5f64.to_bits();
        let block = code_block(vec![
            load_double_instruction(0, local(0), f64::from_bits(bits)),
            core_typed(
                1,
                CoreOpcode::NegateNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_number_artifact(&block);
        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_double(-40.5))
        );
    }

    #[test]
    fn negate_number_primitive_coercions_match_interpreter() {
        let cases = [
            (
                "true",
                RuntimeValue::from_bool(true),
                Some(RuntimeValue::from_i32(-1)),
            ),
            (
                "false",
                RuntimeValue::from_bool(false),
                Some(RuntimeValue::from_double(-0.0)),
            ),
            (
                "null",
                RuntimeValue::null(),
                Some(RuntimeValue::from_double(-0.0)),
            ),
            ("undefined", RuntimeValue::undefined(), None),
        ];
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::NegateNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_number_artifact(&block);

        for (case_index, (name, value, expected)) in cases.into_iter().enumerate() {
            let initial_locals = [(local(0), value)];
            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            if let Some(expected) = expected {
                assert_eq!(
                    generated_result,
                    Ok(BaselineGeneratedExecutionResult::Completed(
                        interpreter_result.clone()
                    )),
                    "case {case_index}: {name}"
                );
                assert_eq!(
                    interpreter_result,
                    ExecutionCompletion::Returned(expected),
                    "case {case_index}: {name}"
                );
            } else {
                assert!(
                    is_returned_double_nan(&interpreter_result),
                    "case {case_index}: {name}: expected interpreter double NaN"
                );
                assert!(
                    matches!(
                        generated_result,
                        Ok(BaselineGeneratedExecutionResult::Completed(ref completion))
                            if is_returned_double_nan(completion)
                    ),
                    "case {case_index}: {name}: expected generated double NaN"
                );
            }
        }
    }

    #[test]
    fn div_number_writes_double_result() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(2)],
            ),
            core_typed(
                2,
                CoreOpcode::DivNumber,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = primitive_number_artifact(&block);

        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_double(3.5))
        );
    }

    #[test]
    fn mod_number_keeps_int32_remainder_when_checked_result_succeeds() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(4)],
            ),
            core_typed(
                2,
                CoreOpcode::ModNumber,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = primitive_number_artifact(&block);

        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(3))
        );
    }

    #[test]
    fn mod_number_falls_through_to_f64_result_for_divisor_zero_overflow_and_double() {
        let cases = [
            (
                "divisor zero",
                RuntimeValue::from_i32(7),
                RuntimeValue::from_i32(0),
                None,
            ),
            (
                "overflow",
                RuntimeValue::from_i32(i32::MIN),
                RuntimeValue::from_i32(-1),
                Some(RuntimeValue::from_double(-0.0)),
            ),
            (
                "double",
                RuntimeValue::from_double(7.5),
                RuntimeValue::from_i32(2),
                Some(RuntimeValue::from_double(1.5)),
            ),
        ];

        for (case_index, (name, left, right, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    CoreOpcode::ModNumber,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = primitive_number_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            if let Some(expected) = expected {
                assert_eq!(
                    interpreter_result,
                    ExecutionCompletion::Returned(expected),
                    "case {case_index}: {name}"
                );
            } else {
                let returned_nan = match interpreter_result {
                    ExecutionCompletion::Returned(value) => {
                        matches!(value.as_number(), Some(NumberValue::DoubleBits(bits)) if bits.to_f64().is_nan())
                    }
                    _ => false,
                };
                assert!(
                    returned_nan,
                    "case {case_index}: {name}: expected double NaN"
                );
            }
        }
    }

    #[test]
    fn pure_number_binary_add_sub_mul_write_double_for_double_and_overflow_inputs() {
        let cases = [
            (
                "add mixed",
                CoreOpcode::AddInt32,
                RuntimeValue::from_i32(1),
                RuntimeValue::from_double(2.5),
                RuntimeValue::from_double(3.5),
            ),
            (
                "sub mixed",
                CoreOpcode::SubInt32,
                RuntimeValue::from_double(5.5),
                RuntimeValue::from_i32(2),
                RuntimeValue::from_double(3.5),
            ),
            (
                "mul double integral",
                CoreOpcode::MulInt32,
                RuntimeValue::from_double(1.5),
                RuntimeValue::from_i32(2),
                RuntimeValue::from_double(3.0),
            ),
            (
                "add overflow",
                CoreOpcode::AddInt32,
                RuntimeValue::from_i32(i32::MAX),
                RuntimeValue::from_i32(1),
                RuntimeValue::from_double(2_147_483_648.0),
            ),
            (
                "sub overflow",
                CoreOpcode::SubInt32,
                RuntimeValue::from_i32(i32::MIN),
                RuntimeValue::from_i32(1),
                RuntimeValue::from_double(-2_147_483_649.0),
            ),
            (
                "mul overflow",
                CoreOpcode::MulInt32,
                RuntimeValue::from_i32(1_073_741_824),
                RuntimeValue::from_i32(4),
                RuntimeValue::from_double(4_294_967_296.0),
            ),
        ];

        for (case_index, (name, opcode, left, right, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = pure_number_binary_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(expected),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn pure_number_binary_div_mod_edges_match_interpreter() {
        let cases = [
            (
                "division by zero",
                CoreOpcode::DivNumber,
                RuntimeValue::from_i32(1),
                RuntimeValue::from_i32(0),
                Some(RuntimeValue::from_double(f64::INFINITY)),
            ),
            (
                "zero divided by zero",
                CoreOpcode::DivNumber,
                RuntimeValue::from_i32(0),
                RuntimeValue::from_i32(0),
                None,
            ),
            (
                "modulo zero",
                CoreOpcode::ModNumber,
                RuntimeValue::from_i32(7),
                RuntimeValue::from_i32(0),
                None,
            ),
            (
                "min modulo negative one",
                CoreOpcode::ModNumber,
                RuntimeValue::from_i32(i32::MIN),
                RuntimeValue::from_i32(-1),
                Some(RuntimeValue::from_double(-0.0)),
            ),
            (
                "double modulo",
                CoreOpcode::ModNumber,
                RuntimeValue::from_double(7.5),
                RuntimeValue::from_i32(2),
                Some(RuntimeValue::from_double(1.5)),
            ),
        ];

        for (case_index, (name, opcode, left, right, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = pure_number_binary_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            if let Some(expected) = expected {
                assert_eq!(
                    generated_result,
                    Ok(BaselineGeneratedExecutionResult::Completed(
                        interpreter_result.clone()
                    )),
                    "case {case_index}: {name}"
                );
                assert_eq!(
                    interpreter_result,
                    ExecutionCompletion::Returned(expected),
                    "case {case_index}: {name}"
                );
            } else {
                assert!(
                    is_returned_double_nan(&interpreter_result),
                    "case {case_index}: {name}: expected interpreter double NaN"
                );
                assert!(
                    matches!(
                        generated_result,
                        Ok(BaselineGeneratedExecutionResult::Completed(ref completion))
                            if is_returned_double_nan(completion)
                    ),
                    "case {case_index}: {name}: expected generated double NaN"
                );
            }
        }
    }

    #[test]
    fn pure_number_binary_relational_handles_mixed_double_and_nan_cases() {
        let cases = [
            (
                "less than mixed",
                CoreOpcode::LessThanInt32,
                RuntimeValue::from_double(1.5),
                RuntimeValue::from_i32(2),
                true,
            ),
            (
                "less equal mixed",
                CoreOpcode::LessEqualInt32,
                RuntimeValue::from_i32(2),
                RuntimeValue::from_double(2.5),
                true,
            ),
            (
                "greater than mixed",
                CoreOpcode::GreaterThanInt32,
                RuntimeValue::from_double(3.5),
                RuntimeValue::from_i32(3),
                true,
            ),
            (
                "greater equal false",
                CoreOpcode::GreaterEqualInt32,
                RuntimeValue::from_i32(3),
                RuntimeValue::from_double(3.5),
                false,
            ),
            (
                "nan less than",
                CoreOpcode::LessThanInt32,
                RuntimeValue::from_double(f64::NAN),
                RuntimeValue::from_i32(1),
                false,
            ),
            (
                "nan less equal",
                CoreOpcode::LessEqualInt32,
                RuntimeValue::from_i32(1),
                RuntimeValue::from_double(f64::NAN),
                false,
            ),
            (
                "nan greater than",
                CoreOpcode::GreaterThanInt32,
                RuntimeValue::from_double(f64::NAN),
                RuntimeValue::from_double(f64::NAN),
                false,
            ),
        ];

        for (case_index, (name, opcode, left, right, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = pure_number_binary_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(RuntimeValue::from_bool(expected)),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn pure_number_binary_bitwise_applies_double_to_int32_conversion() {
        let cases = [
            (
                "fractional or",
                CoreOpcode::BitOrInt32,
                RuntimeValue::from_double(1.5),
                RuntimeValue::from_i32(2),
                RuntimeValue::from_i32(3),
            ),
            (
                "nan and",
                CoreOpcode::BitAndInt32,
                RuntimeValue::from_double(f64::NAN),
                RuntimeValue::from_i32(7),
                RuntimeValue::from_i32(0),
            ),
            (
                "infinity xor",
                CoreOpcode::BitXorInt32,
                RuntimeValue::from_double(f64::INFINITY),
                RuntimeValue::from_i32(5),
                RuntimeValue::from_i32(5),
            ),
            (
                "negative fractional unsigned shift",
                CoreOpcode::UnsignedRightShiftInt32,
                RuntimeValue::from_double(-1.5),
                RuntimeValue::from_i32(0),
                RuntimeValue::from_double(4_294_967_295.0),
            ),
        ];

        for (case_index, (name, opcode, left, right, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = pure_number_binary_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(expected),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn pure_number_binary_non_number_operands_fall_back_with_value_kind() {
        let cases = vec![
            (
                "add boolean left",
                CoreOpcode::AddInt32,
                RuntimeValue::from_bool(true),
                RuntimeValue::from_i32(1),
                1,
                local(0),
                ValueKind::Boolean,
            ),
            (
                "sub null right",
                CoreOpcode::SubInt32,
                RuntimeValue::from_i32(1),
                RuntimeValue::null(),
                2,
                local(1),
                ValueKind::Null,
            ),
            (
                "bit and undefined left",
                CoreOpcode::BitAndInt32,
                RuntimeValue::undefined(),
                RuntimeValue::from_i32(7),
                1,
                local(0),
                ValueKind::Undefined,
            ),
            (
                "unsigned shift cell right",
                CoreOpcode::UnsignedRightShiftInt32,
                RuntimeValue::from_i32(-1),
                cell_runtime_value(),
                2,
                local(1),
                ValueKind::Cell,
            ),
            (
                "less than unknown left",
                CoreOpcode::LessThanInt32,
                unknown_runtime_value(),
                RuntimeValue::from_i32(1),
                1,
                local(0),
                ValueKind::Unknown,
            ),
            (
                "greater equal boolean right",
                CoreOpcode::GreaterEqualInt32,
                RuntimeValue::from_i32(1),
                RuntimeValue::from_bool(false),
                2,
                local(1),
                ValueKind::Boolean,
            ),
        ];

        for (case_index, (name, opcode, left, right, operand_index, register, value_kind)) in
            cases.into_iter().enumerate()
        {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = pure_number_binary_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];
            let (result, stack, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
            let frame = stack.top_frame().unwrap().id;
            let bytecode_index = BytecodeIndex::from_offset(0);

            assert_generated_fallback(
                &result,
                BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                core_fallback_reason(
                    bytecode_index,
                    opcode,
                    BaselineGeneratedFallbackCause::NonNumberOperand {
                        operand_index,
                        register,
                        value_kind,
                    },
                ),
            );
            assert_eq!(
                stack.top_frame().unwrap().bytecode_index,
                Some(bytecode_index),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn int32_bitwise_matches_interpreter() {
        let cases = [
            (CoreOpcode::BitNotInt32, -43, 0, RuntimeValue::from_i32(42)),
            (CoreOpcode::BitOrInt32, 8, 2, RuntimeValue::from_i32(10)),
            (CoreOpcode::BitXorInt32, 7, 3, RuntimeValue::from_i32(4)),
            (CoreOpcode::BitAndInt32, 5, 3, RuntimeValue::from_i32(1)),
            (CoreOpcode::LeftShiftInt32, 1, 5, RuntimeValue::from_i32(32)),
            (
                CoreOpcode::RightShiftInt32,
                -8,
                1,
                RuntimeValue::from_i32(-4),
            ),
            (
                CoreOpcode::UnsignedRightShiftInt32,
                8,
                33,
                RuntimeValue::from_i32(4),
            ),
            (
                CoreOpcode::UnsignedRightShiftInt32,
                i32::MIN,
                0,
                RuntimeValue::from_double(2_147_483_648.0),
            ),
            (
                CoreOpcode::UnsignedRightShiftInt32,
                -1,
                0,
                RuntimeValue::from_double(4_294_967_295.0),
            ),
        ];

        for (case_index, (opcode, left, right, expected)) in cases.into_iter().enumerate() {
            let mut instructions = vec![core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(left)],
            )];
            if opcode == CoreOpcode::BitNotInt32 {
                instructions.push(core_typed(
                    1,
                    opcode,
                    vec![Operand::Register(local(2)), Operand::Register(local(0))],
                ));
                instructions.push(core_typed(
                    2,
                    CoreOpcode::Return,
                    vec![Operand::Register(local(2))],
                ));
            } else {
                instructions.push(core_typed(
                    1,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(1)), Operand::SignedImmediate(right)],
                ));
                instructions.push(core_typed(
                    2,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ));
                instructions.push(core_typed(
                    3,
                    CoreOpcode::Return,
                    vec![Operand::Register(local(2))],
                ));
            }
            let block = code_block(instructions);
            let artifact = artifact_for_block(owner(), &block);

            let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
            let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {opcode:?}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(expected),
                "case {case_index}: {opcode:?}"
            );
        }
    }

    #[test]
    fn bit_not_int32_primitive_coercions_match_interpreter() {
        let cases = [
            (
                "true",
                RuntimeValue::from_bool(true),
                RuntimeValue::from_i32(-2),
            ),
            (
                "false",
                RuntimeValue::from_bool(false),
                RuntimeValue::from_i32(-1),
            ),
            ("null", RuntimeValue::null(), RuntimeValue::from_i32(-1)),
            (
                "undefined",
                RuntimeValue::undefined(),
                RuntimeValue::from_i32(-1),
            ),
            (
                "double positive",
                RuntimeValue::from_double(1.5),
                RuntimeValue::from_i32(-2),
            ),
            (
                "double negative",
                RuntimeValue::from_double(-1.5),
                RuntimeValue::from_i32(0),
            ),
            (
                "double nan",
                RuntimeValue::from_double(f64::NAN),
                RuntimeValue::from_i32(-1),
            ),
        ];
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::BitNotInt32,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);

        for (case_index, (name, value, expected)) in cases.into_iter().enumerate() {
            let initial_locals = [(local(0), value)];
            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(expected),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn int32_relational_matches_interpreter() {
        let cases = [
            (CoreOpcode::LessThanInt32, 1, 2, true),
            (CoreOpcode::LessEqualInt32, 2, 2, true),
            (CoreOpcode::GreaterThanInt32, 5, 3, true),
            (CoreOpcode::GreaterEqualInt32, 3, 4, false),
        ];

        for (case_index, (opcode, left, right, expected)) in cases.into_iter().enumerate() {
            let block = code_block(vec![
                core_typed(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(left)],
                ),
                core_typed(
                    1,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(1)), Operand::SignedImmediate(right)],
                ),
                core_typed(
                    2,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = artifact_for_block_with_subset(
                owner(),
                &block,
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational,
            );

            let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
            let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {opcode:?}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(RuntimeValue::from_bool(expected)),
                "case {case_index}: {opcode:?}"
            );
        }
    }

    #[test]
    fn unconditional_jump_sets_generated_pc() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(BytecodeIndex::from_offset(2))],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(11)],
            ),
            core_typed(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(22)],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block_with_subset(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps,
        );

        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(22))
        );
    }

    #[test]
    fn jump_if_not_nullish_taken_sets_generated_pc() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            core_typed(
                1,
                CoreOpcode::JumpIfNotNullish,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(4)),
                ],
            ),
            core_typed(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            core_typed(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(22)],
            ),
            core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = artifact_for_block_with_subset(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps,
        );

        let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
        let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            generated_result,
            Ok(BaselineGeneratedExecutionResult::Completed(
                interpreter_result.clone()
            ))
        );
        assert_eq!(
            interpreter_result,
            ExecutionCompletion::Returned(RuntimeValue::from_i32(22))
        );
    }

    #[test]
    fn jump_if_not_nullish_falls_through_for_nullish_values() {
        for (case_index, load_opcode) in [CoreOpcode::LoadUndefined, CoreOpcode::LoadNull]
            .into_iter()
            .enumerate()
        {
            let block = code_block(vec![
                core_typed(0, load_opcode, vec![Operand::Register(local(0))]),
                core_typed(
                    1,
                    CoreOpcode::JumpIfNotNullish,
                    vec![
                        Operand::Register(local(0)),
                        Operand::BytecodeIndex(BytecodeIndex::from_offset(4)),
                    ],
                ),
                core_typed(
                    2,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
                ),
                core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
                core_typed(
                    4,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(1)), Operand::SignedImmediate(22)],
                ),
                core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            ]);
            let artifact = artifact_for_block_with_subset(
                owner(),
                &block,
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps,
            );

            let (interpreter_result, _, _) = execute_interpreter(owner(), &block);
            let (generated_result, _, _) = execute_generated(owner(), &block, &artifact);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {load_opcode:?}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(RuntimeValue::from_i32(11)),
                "case {case_index}: {load_opcode:?}"
            );
        }
    }

    #[test]
    fn jump_if_false_falsy_primitives_take_branch() {
        let cases = [
            ("undefined", RuntimeValue::undefined()),
            ("null", RuntimeValue::null()),
            ("false", RuntimeValue::from_bool(false)),
            ("int32 zero", RuntimeValue::from_i32(0)),
            ("double positive zero", RuntimeValue::from_double(0.0)),
            ("double negative zero", RuntimeValue::from_double(-0.0)),
            ("double nan", RuntimeValue::from_double(f64::NAN)),
        ];

        for (case_index, (name, value)) in cases.into_iter().enumerate() {
            let block = jump_if_false_block();
            let artifact = primitive_truthiness_artifact(&block);
            let initial_locals = [(local(0), value)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(RuntimeValue::from_i32(22)),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn jump_if_false_truthy_primitives_fall_through() {
        let cases = [
            ("true", RuntimeValue::from_bool(true)),
            ("nonzero int32", RuntimeValue::from_i32(-7)),
            ("nonzero double", RuntimeValue::from_double(3.5)),
        ];

        for (case_index, (name, value)) in cases.into_iter().enumerate() {
            let block = jump_if_false_block();
            let artifact = primitive_truthiness_artifact(&block);
            let initial_locals = [(local(0), value)];

            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(RuntimeValue::from_i32(11)),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn jump_if_false_cell_condition_falls_back_at_current_bytecode_index() {
        let block = jump_if_false_block();
        let artifact = primitive_truthiness_artifact(&block);
        let value = cell_runtime_value();
        let initial_locals = [(local(0), value)];
        let (result, stack, _) =
            execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::JumpIfFalse,
                BaselineGeneratedFallbackCause::UnsupportedTruthinessOperand {
                    operand_index: 0,
                    register: local(0),
                    value_kind: ValueKind::Cell,
                },
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn jump_if_false_unknown_condition_falls_back_at_current_bytecode_index() {
        let block = jump_if_false_block();
        let artifact = primitive_truthiness_artifact(&block);
        let value = RuntimeValue::from_encoded(EncodedJsValue(0xff));
        assert_eq!(value.kind(), ValueKind::Unknown);
        let initial_locals = [(local(0), value)];
        let (result, stack, _) =
            execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::JumpIfFalse,
                BaselineGeneratedFallbackCause::UnsupportedTruthinessOperand {
                    operand_index: 0,
                    register: local(0),
                    value_kind: ValueKind::Unknown,
                },
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn malformed_jump_if_false_target_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::JumpIfFalse,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(3)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = primitive_truthiness_artifact(&block);
        let initial_locals = [(local(0), RuntimeValue::from_bool(true))];
        let (result, stack, _) =
            execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::JumpIfFalse,
                BaselineGeneratedFallbackCause::OperandAccess {
                    error: OperandAccessError::UnexpectedOperandKind {
                        opcode: CoreOpcode::JumpIfFalse.opcode(),
                        index: 1,
                        expected: OperandKind::BytecodeIndex,
                        actual: OperandKind::UnsignedImmediate,
                    },
                },
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn primitive_strict_equality_matrix_matches_interpreter() {
        let values = [
            ("undefined", RuntimeValue::undefined()),
            ("null", RuntimeValue::null()),
            ("false", RuntimeValue::from_bool(false)),
            ("true", RuntimeValue::from_bool(true)),
            ("int32 zero", RuntimeValue::from_i32(0)),
            ("int32 one", RuntimeValue::from_i32(1)),
            ("double positive zero", RuntimeValue::from_double(0.0)),
            ("double negative zero", RuntimeValue::from_double(-0.0)),
            ("double one", RuntimeValue::from_double(1.0)),
            ("double nan", RuntimeValue::from_double(f64::NAN)),
        ];

        for opcode in [CoreOpcode::StrictEqual, CoreOpcode::StrictNotEqual] {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = primitive_boolean_artifact(&block);

            for (left_index, (left_name, left)) in values.iter().copied().enumerate() {
                for (right_index, (right_name, right)) in values.iter().copied().enumerate() {
                    let initial_locals = [(local(0), left), (local(1), right)];
                    let (interpreter_result, _, _) =
                        execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
                    let (generated_result, _, _) = execute_generated_with_initial_locals(
                        owner(),
                        &block,
                        &artifact,
                        &initial_locals,
                    );
                    let equals = left.strict_equals(right);
                    let expected = matches!(opcode, CoreOpcode::StrictEqual) == equals;

                    assert_eq!(
                        generated_result,
                        Ok(BaselineGeneratedExecutionResult::Completed(
                            interpreter_result.clone()
                        )),
                        "case {left_index}/{right_index}: {opcode:?} {left_name} {right_name}"
                    );
                    assert_eq!(
                        interpreter_result,
                        ExecutionCompletion::Returned(RuntimeValue::from_bool(expected)),
                        "case {left_index}/{right_index}: {opcode:?} {left_name} {right_name}"
                    );
                }
            }
        }
    }

    #[test]
    fn logical_not_primitive_cases_match_interpreter() {
        let cases = [
            ("undefined", RuntimeValue::undefined()),
            ("null", RuntimeValue::null()),
            ("false", RuntimeValue::from_bool(false)),
            ("true", RuntimeValue::from_bool(true)),
            ("int32 zero", RuntimeValue::from_i32(0)),
            ("nonzero int32", RuntimeValue::from_i32(-7)),
            ("double positive zero", RuntimeValue::from_double(0.0)),
            ("double negative zero", RuntimeValue::from_double(-0.0)),
            ("double nan", RuntimeValue::from_double(f64::NAN)),
            ("nonzero double", RuntimeValue::from_double(3.5)),
        ];
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LogicalNot,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_boolean_artifact(&block);

        for (case_index, (name, value)) in cases.into_iter().enumerate() {
            let initial_locals = [(local(0), value)];
            let (interpreter_result, _, _) =
                execute_interpreter_with_initial_locals(owner(), &block, &initial_locals);
            let (generated_result, _, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
            let expected = !local_primitive_truthiness(value).unwrap();

            assert_eq!(
                generated_result,
                Ok(BaselineGeneratedExecutionResult::Completed(
                    interpreter_result.clone()
                )),
                "case {case_index}: {name}"
            );
            assert_eq!(
                interpreter_result,
                ExecutionCompletion::Returned(RuntimeValue::from_bool(expected)),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn strict_equality_cell_and_unknown_operands_fall_back_at_current_bytecode_index() {
        let cases = [
            (
                "strict equal left cell",
                CoreOpcode::StrictEqual,
                cell_runtime_value(),
                RuntimeValue::from_i32(7),
                1,
                local(0),
                ValueKind::Cell,
            ),
            (
                "strict not equal left unknown",
                CoreOpcode::StrictNotEqual,
                unknown_runtime_value(),
                RuntimeValue::from_i32(7),
                1,
                local(0),
                ValueKind::Unknown,
            ),
            (
                "strict equal right cell",
                CoreOpcode::StrictEqual,
                RuntimeValue::from_bool(true),
                cell_runtime_value(),
                2,
                local(1),
                ValueKind::Cell,
            ),
            (
                "strict not equal right unknown",
                CoreOpcode::StrictNotEqual,
                RuntimeValue::from_bool(true),
                unknown_runtime_value(),
                2,
                local(1),
                ValueKind::Unknown,
            ),
        ];

        for (case_index, (name, opcode, left, right, operand_index, register, value_kind)) in
            cases.into_iter().enumerate()
        {
            let block = code_block(vec![
                core_typed(
                    0,
                    opcode,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            ]);
            let artifact = primitive_boolean_artifact(&block);
            let initial_locals = [(local(0), left), (local(1), right)];
            let (result, stack, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
            let frame = stack.top_frame().unwrap().id;
            let bytecode_index = BytecodeIndex::from_offset(0);

            assert_generated_fallback(
                &result,
                BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                core_fallback_reason(
                    bytecode_index,
                    opcode,
                    BaselineGeneratedFallbackCause::UnsupportedStrictEqualityOperand {
                        operand_index,
                        register,
                        value_kind,
                    },
                ),
            );
            assert_eq!(
                stack.top_frame().unwrap().bytecode_index,
                Some(bytecode_index),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn logical_not_cell_and_unknown_operands_fall_back_at_source_operand_index() {
        let cases = [
            ("cell", cell_runtime_value(), ValueKind::Cell),
            ("unknown", unknown_runtime_value(), ValueKind::Unknown),
        ];
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LogicalNot,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = primitive_boolean_artifact(&block);

        for (case_index, (name, value, value_kind)) in cases.into_iter().enumerate() {
            let initial_locals = [(local(0), value)];
            let (result, stack, _) =
                execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
            let frame = stack.top_frame().unwrap().id;
            let bytecode_index = BytecodeIndex::from_offset(0);

            assert_generated_fallback(
                &result,
                BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                core_fallback_reason(
                    bytecode_index,
                    CoreOpcode::LogicalNot,
                    BaselineGeneratedFallbackCause::UnsupportedTruthinessOperand {
                        operand_index: 1,
                        register: local(0),
                        value_kind,
                    },
                ),
            );
            assert_eq!(
                stack.top_frame().unwrap().bytecode_index,
                Some(bytecode_index),
                "case {case_index}: {name}"
            );
        }
    }

    #[test]
    fn invalid_taken_branch_target_returns_execution_error() {
        let invalid_target = BytecodeIndex::from_offset(99);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(invalid_target)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block_with_subset(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps,
        );
        let (result, stack, _registers) = execute_generated(owner(), &block, &artifact);

        assert_eq!(
            result,
            Err(BaselineGeneratedExecutionError::Execution(
                ExecutionError::InvalidBytecodeIndex(invalid_target)
            ))
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(BytecodeIndex::from_offset(0))
        );
    }

    #[test]
    fn malformed_branch_target_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::JumpIfNotNullish,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(2)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block_with_subset(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps,
        );
        let (result, stack, _registers) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::JumpIfNotNullish,
                BaselineGeneratedFallbackCause::OperandAccess {
                    error: OperandAccessError::UnexpectedOperandKind {
                        opcode: CoreOpcode::JumpIfNotNullish.opcode(),
                        index: 1,
                        expected: OperandKind::BytecodeIndex,
                        actual: OperandKind::UnsignedImmediate,
                    },
                },
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn old_generated_subsets_reject_branch_opcodes() {
        let block = code_block(Vec::new());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let (_, window) = baseline_active_frame(&stack, frame, owner()).unwrap();
        let mut execution = InterpreterExecutionState {
            stack: &mut stack,
            registers: &mut registers,
            exceptions: &mut exceptions,
            heap: &mut heap,
        };

        let bytecode_index = BytecodeIndex::from_offset(0);
        let operands = [Operand::BytecodeIndex(BytecodeIndex::from_offset(0))];
        let instruction = DecodedInstruction {
            opcode: CoreOpcode::Jump.opcode(),
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &operands,
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };

        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: core_fallback_reason(
                        bytecode_index,
                        CoreOpcode::Jump,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );
    }

    #[test]
    fn binary_non_int32_bitwise_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(1)), Operand::UnsignedImmediate(1)],
            ),
            core_typed(
                2,
                CoreOpcode::BitAndInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(2);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::BitAndInt32,
                BaselineGeneratedFallbackCause::NonInt32Operand {
                    operand_index: 2,
                    register: local(1),
                },
            ),
        );

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(0)],
            ),
            core_typed(
                1,
                CoreOpcode::UnsignedRightShiftInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);
        let (result, stack, _) = execute_generated_with_initial_locals(
            owner(),
            &block,
            &artifact,
            &[(local(0), RuntimeValue::from_double(1.5))],
        );
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(1);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::UnsignedRightShiftInt32,
                BaselineGeneratedFallbackCause::NonInt32Operand {
                    operand_index: 1,
                    register: local(0),
                },
            ),
        );
    }

    #[test]
    fn non_int32_relational_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                2,
                CoreOpcode::LessThanInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = artifact_for_block_with_subset(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational,
        );
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(2);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::LessThanInt32,
                BaselineGeneratedFallbackCause::NonInt32Operand {
                    operand_index: 1,
                    register: local(0),
                },
            ),
        );

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(1)), Operand::UnsignedImmediate(1)],
            ),
            core_typed(
                2,
                CoreOpcode::GreaterEqualInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = artifact_for_block_with_subset(
            owner(),
            &block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational,
        );
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(2);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::GreaterEqualInt32,
                BaselineGeneratedFallbackCause::NonInt32Operand {
                    operand_index: 2,
                    register: local(1),
                },
            ),
        );
    }

    #[test]
    fn int32_overflow_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(local(0)),
                    Operand::SignedImmediate(i32::MAX),
                ],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                2,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(2);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::AddInt32,
                BaselineGeneratedFallbackCause::Int32Overflow,
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn non_int32_arithmetic_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
            ),
            core_typed(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                2,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(2);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::AddInt32,
                BaselineGeneratedFallbackCause::NonInt32Operand {
                    operand_index: 1,
                    register: local(0),
                },
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn primitive_numeric_coercion_cell_and_unknown_fall_back_with_value_kind() {
        let values = [
            ("cell", cell_runtime_value(), ValueKind::Cell),
            ("unknown", unknown_runtime_value(), ValueKind::Unknown),
        ];

        for opcode in [
            CoreOpcode::ToNumber,
            CoreOpcode::NegateNumber,
            CoreOpcode::BitNotInt32,
        ] {
            for (case_index, (name, value, value_kind)) in values.into_iter().enumerate() {
                let block = code_block(vec![
                    core_typed(
                        0,
                        opcode,
                        vec![Operand::Register(local(1)), Operand::Register(local(0))],
                    ),
                    core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
                ]);
                let artifact = if opcode == CoreOpcode::ToNumber {
                    primitive_to_number_void_artifact(&block)
                } else if opcode == CoreOpcode::NegateNumber {
                    primitive_number_artifact(&block)
                } else {
                    artifact_for_block(owner(), &block)
                };
                let initial_locals = [(local(0), value)];
                let (result, stack, _) = execute_generated_with_initial_locals(
                    owner(),
                    &block,
                    &artifact,
                    &initial_locals,
                );
                let frame = stack.top_frame().unwrap().id;
                let bytecode_index = BytecodeIndex::from_offset(0);

                assert_generated_fallback(
                    &result,
                    BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    core_fallback_reason(
                        bytecode_index,
                        opcode,
                        BaselineGeneratedFallbackCause::UnsupportedPrimitiveNumericCoercionOperand {
                            operand_index: 1,
                            register: local(0),
                            value_kind,
                        },
                    ),
                );
                assert_eq!(
                    stack.top_frame().unwrap().bytecode_index,
                    Some(bytecode_index),
                    "case {case_index}: {opcode:?} {name}"
                );
            }
        }

        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::DivNumber,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let artifact = primitive_number_artifact(&block);
        let initial_locals = [
            (local(0), RuntimeValue::from_i32(1)),
            (local(1), RuntimeValue::null()),
        ];
        let (result, stack, _) =
            execute_generated_with_initial_locals(owner(), &block, &artifact, &initial_locals);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::DivNumber,
                BaselineGeneratedFallbackCause::NonNumberOperand {
                    operand_index: 2,
                    register: local(1),
                    value_kind: ValueKind::Null,
                },
            ),
        );
    }

    #[test]
    fn mismatched_unsupported_opcode_rejects_before_executing() {
        let supported_shape = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                1,
                CoreOpcode::Move,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let artifact = artifact_for_block(owner(), &supported_shape);
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            core_typed(
                1,
                CoreOpcode::PowNumber,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(0)),
                    Operand::Register(local(0)),
                ],
            ),
            core_typed(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let (result, _, _registers) = execute_generated(owner(), &block, &artifact);

        assert!(matches!(
            result,
            Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch { .. })
        ));
    }

    #[test]
    fn unsupported_opcode_instruction_fallback_carries_opcode_reason() {
        let block = code_block(Vec::new());
        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let (_, window) = baseline_active_frame(&stack, frame, owner()).unwrap();
        let mut execution = InterpreterExecutionState {
            stack: &mut stack,
            registers: &mut registers,
            exceptions: &mut exceptions,
            heap: &mut heap,
        };

        let bytecode_index = BytecodeIndex::from_offset(0);
        let core_instruction = DecodedInstruction {
            opcode: CoreOpcode::PowNumber.opcode(),
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &[],
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };

        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                core_instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: core_fallback_reason(
                        bytecode_index,
                        CoreOpcode::PowNumber,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );

        let core_instruction = DecodedInstruction {
            opcode: CoreOpcode::PowNumber.opcode(),
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &[],
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };
        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                core_instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: core_fallback_reason(
                        bytecode_index,
                        CoreOpcode::PowNumber,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );

        let bytecode_index = BytecodeIndex::from_offset(1);
        let raw_opcode = Opcode::Generated(OpcodeId::from_generated_index(4095));
        let raw_instruction = DecodedInstruction {
            opcode: raw_opcode,
            width: OperandWidth::Narrow,
            bytecode_index,
            operands: &[],
            schema: None,
            source: DecodedInstructionSource::TypedPlaceholder,
        };

        assert_eq!(
            execute_instruction(
                BaselineInstructionContext::new(
                    BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber,
                    owner(),
                    frame,
                    &block,
                    None,
                ),
                window,
                &mut execution,
                raw_instruction,
                None,
                None,
            ),
            Ok(BaselineInstructionOutcome::Fallback(
                BaselineGeneratedFallback {
                    request: BaselineFallbackRequest::new(owner(), frame, bytecode_index),
                    reason: non_core_fallback_reason(
                        bytecode_index,
                        raw_opcode,
                        BaselineGeneratedFallbackCause::UnsupportedOpcode,
                    ),
                }
            ))
        );
    }

    #[test]
    fn bad_operand_falls_back_at_current_bytecode_index() {
        let block = code_block(vec![
            core_typed(0, CoreOpcode::LoadInt32, vec![Operand::Register(local(0))]),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);
        let (result, stack, _) = execute_generated(owner(), &block, &artifact);
        let frame = stack.top_frame().unwrap().id;
        let bytecode_index = BytecodeIndex::from_offset(0);

        assert_generated_fallback(
            &result,
            BaselineFallbackRequest::new(owner(), frame, bytecode_index),
            core_fallback_reason(
                bytecode_index,
                CoreOpcode::LoadInt32,
                BaselineGeneratedFallbackCause::BadImmediate {
                    operand_index: 1,
                    error: OperandAccessError::MissingOperand {
                        opcode: CoreOpcode::LoadInt32.opcode(),
                        index: 1,
                    },
                },
            ),
        );
        assert_eq!(
            stack.top_frame().unwrap().bytecode_index,
            Some(bytecode_index)
        );
    }

    #[test]
    fn mismatched_same_owner_operand_snapshot_fails_before_executing() {
        let installed = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(9)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block(owner(), &installed);
        let current = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(10)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);

        let (result, _, _registers) = execute_generated(owner(), &current, &artifact);

        assert!(matches!(
            result,
            Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch { .. })
        ));
    }

    #[test]
    fn mismatched_same_owner_bytecode_indices_fail_before_executing() {
        let installed = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(9)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block(owner(), &installed);
        let current = code_block(vec![
            core_typed(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(9)],
            ),
            core_typed(5, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);

        let (result, _, _registers) = execute_generated(owner(), &current, &artifact);

        assert!(matches!(
            result,
            Err(BaselineGeneratedExecutionError::CodeBlockSnapshotMismatch { .. })
        ));
    }

    #[test]
    fn wrong_owner_artifact_and_frame_fail_before_executing() {
        let block = code_block(vec![
            core_typed(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(9)],
            ),
            core_typed(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let artifact = artifact_for_block(owner(), &block);

        let (owner_result, _, _owner_registers) =
            execute_generated(other_owner(), &block, &artifact);
        assert_eq!(
            owner_result,
            Err(BaselineGeneratedExecutionError::OwnerMismatch {
                expected: owner(),
                actual: other_owner(),
            })
        );

        let mut invalid_artifact = artifact.clone();
        invalid_artifact.liveness = CodeLiveness::Unallocated;
        let (artifact_result, _, _artifact_registers) =
            execute_generated(owner(), &block, &invalid_artifact);
        assert_eq!(
            artifact_result,
            Err(BaselineGeneratedExecutionError::ArtifactValidation(
                JitCodeValidationError::BaselineGeneratedCodeNotLive
            ))
        );

        let mut invalid_effect_artifact = artifact.clone();
        invalid_effect_artifact.body.effect_contract = None;
        let (effect_result, _, _effect_registers) =
            execute_generated(owner(), &block, &invalid_effect_artifact);
        assert_eq!(
            effect_result,
            Err(BaselineGeneratedExecutionError::ArtifactValidation(
                JitCodeValidationError::BaselineGeneratedCodeEffectContractMismatch
            ))
        );

        let mut code_block_stack = ExecutionContextStack::default();
        let mut code_block_registers = RegisterFile::default();
        let mut code_block_exceptions = ExceptionState::default();
        let mut code_block_heap = Heap::new();
        let code_block_frame = enter_program_frame(
            &mut code_block_stack,
            &mut code_block_registers,
            other_owner(),
            &block,
        );
        let code_block_result =
            execute_baseline_generated_code(BaselineGeneratedExecutionRequest {
                artifact: &artifact,
                owner: owner(),
                code_block: &block,
                expected_frame: code_block_frame,
                execution: InterpreterExecutionState {
                    stack: &mut code_block_stack,
                    registers: &mut code_block_registers,
                    exceptions: &mut code_block_exceptions,
                    heap: &mut code_block_heap,
                },
            });
        assert_eq!(
            code_block_result,
            Err(BaselineGeneratedExecutionError::Execution(
                ExecutionError::CodeBlockMismatch {
                    expected: owner(),
                    actual: Some(other_owner()),
                }
            ))
        );

        let mut stack = ExecutionContextStack::default();
        let mut registers = RegisterFile::default();
        let mut exceptions = ExceptionState::default();
        let mut heap = Heap::new();
        let frame = enter_program_frame(&mut stack, &mut registers, owner(), &block);
        let result = execute_baseline_generated_code(BaselineGeneratedExecutionRequest {
            artifact: &artifact,
            owner: owner(),
            code_block: &block,
            expected_frame: CallFrameId(frame.0 + 1),
            execution: InterpreterExecutionState {
                stack: &mut stack,
                registers: &mut registers,
                exceptions: &mut exceptions,
                heap: &mut heap,
            },
        });

        assert_eq!(
            result,
            Err(BaselineGeneratedExecutionError::Execution(
                ExecutionError::FrameMismatch {
                    expected: CallFrameId(frame.0 + 1),
                    actual: Some(frame),
                }
            ))
        );
    }
}
