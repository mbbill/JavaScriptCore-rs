//! Deferred JIT integration contracts for the Rust JavaScriptCore skeleton.
//!
//! This module intentionally reserves the shape of JIT-visible execution state
//! without generating code, interpreting bytecode, patching entrypoints, or
//! defining a JavaScript execution path.

#![forbid(unsafe_code)]

pub(crate) mod abi;
pub(crate) mod baseline;
pub(crate) mod code;
pub(crate) mod disassembly;
pub(crate) mod emission;
pub(crate) mod emitter;
pub(crate) mod executable;
pub(crate) mod ic;
pub(crate) mod integration;
pub(crate) mod machine;
pub(crate) mod plan;
pub(crate) mod semantics;
pub(crate) mod tiering;
pub(crate) mod watchpoint;

pub use abi::{
    AbiValue, BaselineAbiValidationError, CallBoundaryId, CallBoundaryMetadata, EntryAbi,
    Entrypoint, EntrypointKind, FrameSlot, FrameSlotRole, PatchpointDescriptor, PatchpointKind,
    RegisterBinding, RegisterRole,
};
pub use code::{
    BaselineArityCheckNativeEntry, BaselineArityCheckUnavailableReason,
    BaselineGeneratedCodeArtifact, BaselineGeneratedCodeBody, BaselineGeneratedCodeBodyId,
    BaselineNativeEntryCallableAuthority, BaselineNativeEntryCallableKind,
    BaselineNativeEntryCallableValidationError, BaselineNativeEntryDescriptor,
    BaselineNativeEntryToken, BaselineNativeEntryTokenKind, CodeBlockJitSlots,
    CodeFinalizationAuthority, CodeInstallBarrier, CodeInvalidationReason, CodeInvalidationState,
    CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership, CodeReplacement, CodeRetentionPolicy,
    JitCodeArtifact, JitCodeId, JitCodeRef, JitCodeValidationError, JitOpaqueByproductId, JitType,
    OpaqueByproductDescriptor,
};
pub use disassembly::{
    DisassemblyAnnotation, DisassemblyExecutionAnnotation, DisassemblyExecutionKind,
    DisassemblyFormat, DisassemblyInstruction, DisassemblyMetadata,
    DisassemblyMetadataValidationError, DisassemblySection, DisassemblySemanticAnnotation,
    DisassemblySemanticKind, DisassemblySource,
};
pub use emission::{
    record_baseline_machine_code_emission, BaselineMachineCodeEmissionRecord,
    BaselineMachineCodeEmissionRelocationSource, BaselineMachineCodeEmissionRequest,
    BaselineMachineCodeEmissionValidationError, BaselineMachineCodeEmitterKind,
};
pub use emitter::{
    emit_p6_arm64_baseline_callable_semantic_bytes,
    emit_p6_x86_64_baseline_callable_semantic_bytes, emit_p6_x86_64_baseline_semantic_bytes,
    emit_p6_x86_64_non_callable_return_stub, plan_p6_x86_64_baseline_lowering,
    record_p6_arm64_baseline_backend_contract_from_plan,
    record_p6_x86_64_baseline_backend_contract,
    record_p6_x86_64_baseline_backend_contract_from_plan, select_p6_x86_64_baseline_instructions,
    BaselineMachineCodeByteGenerationAuthority, BaselineMachineCodeByteGenerationError,
    BaselineMachineCodeByteGenerationProofRequirement, BaselineMachineCodeByteGenerationProofShape,
    BaselineMachineCodeByteGenerationRequest, BaselineMachineCodeByteGenerationResult,
    BaselineMachineCodeByteGenerationShape, P10X86_64BaselinePropertyNativeExitReturnPayload,
    P10X86_64BaselinePropertyNativeExitStubRecord, P14X86_64BaselineLoopBackedgeReturnPayload,
    P14X86_64BaselineLoopBackedgeSafepointStubRecord, P6X86_64BaselineArithmeticSideExitContract,
    P6X86_64BaselineArithmeticSideExitReason, P6X86_64BaselineBackendArtifactContract,
    P6X86_64BaselineBackendArtifactPresence, P6X86_64BaselineBackendContractError,
    P6X86_64BaselineBackendContractRecord, P6X86_64BaselineBackendInstructionContract,
    P6X86_64BaselineBranchTargetRejectionReason, P6X86_64BaselineBytecodeBranchKind,
    P6X86_64BaselineBytecodeBranchRecord, P6X86_64BaselineCallableEpilogueRecord,
    P6X86_64BaselineCallablePrologueRecord, P6X86_64BaselineCheckedInt32Arithmetic,
    P6X86_64BaselineConstantsLocationContract, P6X86_64BaselineControlFlowBranchContract,
    P6X86_64BaselineFrameLayoutContract, P6X86_64BaselineImmediateOperand,
    P6X86_64BaselineInstructionByteRecord, P6X86_64BaselineInstructionSelectionByteEmission,
    P6X86_64BaselineInstructionSelectionCallableAuthority,
    P6X86_64BaselineInstructionSelectionError,
    P6X86_64BaselineInstructionSelectionOperandLocationError,
    P6X86_64BaselineInstructionSelectionPlan, P6X86_64BaselineInstructionSelectionReadiness,
    P6X86_64BaselineInt32ArithmeticExitPolicy, P6X86_64BaselineInt32ArithmeticOperation,
    P6X86_64BaselineInt32OperandGuard, P6X86_64BaselineLoweredInstruction,
    P6X86_64BaselineLoweredOperation, P6X86_64BaselineLoweringByteEmission,
    P6X86_64BaselineLoweringCallableAuthority, P6X86_64BaselineLoweringError,
    P6X86_64BaselineLoweringPlan, P6X86_64BaselineLoweringRequest,
    P6X86_64BaselineLoweringRequirement, P6X86_64BaselineLoweringResult,
    P6X86_64BaselineLoweringValidationShape, P6X86_64BaselineMachineInstruction,
    P6X86_64BaselineMachineMemoryOperand, P6X86_64BaselineMachineOperand,
    P6X86_64BaselineMulNegativeZeroPolicy, P6X86_64BaselineOperandLocation,
    P6X86_64BaselineOperandLocationError, P6X86_64BaselineOperandLocationRecord,
    P6X86_64BaselineOperandRole, P6X86_64BaselinePhysicalRegisterBinding,
    P6X86_64BaselinePhysicalRegisterMap, P6X86_64BaselineReturnRegisterContract,
    P6X86_64BaselineRuntimeHelperNativeExitStubRecord, P6X86_64BaselineSelectedInstruction,
    P6X86_64BaselineSelectedInstructionEffects, P6X86_64BaselineSelectedSideExitReason,
    P6X86_64BaselineSemanticByteEmissionAuthority, P6X86_64BaselineSemanticByteEmissionError,
    P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionShape,
    P6X86_64BaselineSemanticOperandRejectionReason, P6X86_64BaselineSideExitDestinationEffect,
    P6X86_64BaselineSideExitLabel, P6X86_64BaselineSideExitPlaceholderRecord,
    P6X86_64BaselineSideExitReturnPayload, P6X86_64BaselineSideExitReturnStubRecord,
    P6X86_64BaselineSymbolicAbiContract, P6X86_64BaselineSymbolicRegister,
    P6X86_64BaselineTerminalPolicy, P6X86_64BaselineTerminalPolicyRecord,
    P6X86_64BaselineValueLayoutContract, P9X86_64BaselineJsCallNativeExitReturnPayload,
    P9X86_64BaselineJsCallNativeExitStubRecord,
    P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
    P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG,
    P6_X86_64_BASELINE_CALLABLE_SIDE_EXIT_SENTINEL,
    P6_X86_64_BASELINE_RUNTIME_HELPER_NATIVE_EXIT_PAYLOAD_INDEX_BASE,
    P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG,
    P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS,
    P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
};
pub(crate) use emitter::{
    emit_p6_x86_64_baseline_callable_semantic_bytes_with_owner_continuation_map,
    P14X86_64BaselineBackedgeSafepointAuthority,
};
pub use executable::{
    finalize_link_buffer, record_link_buffer_copy_link, ExecutableAllocationOutcome,
    ExecutableAllocationRecord, ExecutableAllocationRequest, ExecutableLedgerValidationError,
    LinkBufferCopyLinkOutcome, LinkBufferCopyLinkRecord, LinkBufferCopyLinkRequest,
    LinkBufferFinalizationOutcome, LinkBufferFinalizationRecord, LinkBufferFinalizationRequest,
};
pub use ic::{
    classify_inline_cache_semantics, classify_inline_cache_slot,
    derive_property_load_guard_dependencies, plan_property_load_access_case_from_observation,
    plan_property_load_guard_plan_from_observation, AccessCaseDescriptor, AccessCaseKind, CacheKey,
    CallLinkAttachmentPlan, CallLinkAttachmentPlanTable, CallLinkAttachmentTargetDescriptor,
    CallLinkInfoDescriptor, CallLinkMode, CallLinkReadinessBlocker, CallLinkReadinessBlockers,
    GeneratedCallLinkCandidate, GeneratedCallLinkCandidateTable, GeneratedCallLinkDirectCallStatus,
    GeneratedCallLinkProbeBlock, GeneratedCallLinkProbeMiss, GeneratedCallLinkProbeMissReason,
    GeneratedCallLinkProbeRequest, GeneratedCallLinkProbeResult,
    GeneratedGuardedPropertyLoadProbeHit, GeneratedGuardedPropertyLoadProbeMiss,
    GeneratedGuardedPropertyLoadProbeMissReason, GeneratedGuardedPropertyLoadProbeRequest,
    GeneratedGuardedPropertyLoadProbeResult, GeneratedPropertyHasMegamorphicCacheEntry,
    GeneratedPropertyHasMegamorphicCandidateTable, GeneratedPropertyHasMegamorphicLookup,
    GeneratedPropertyHasMegamorphicSite, GeneratedPropertyLoadDestinationRootSync,
    GeneratedPropertyLoadMegamorphicCacheEntry, GeneratedPropertyLoadMegamorphicCacheEntryKind,
    GeneratedPropertyLoadMegamorphicCandidateTable,
    GeneratedPropertyLoadMegamorphicHolderProbeRequest, GeneratedPropertyLoadMegamorphicLookup,
    GeneratedPropertyLoadMegamorphicSite, GeneratedPropertyLoadProbeHit,
    GeneratedPropertyLoadProbeMiss, GeneratedPropertyLoadProbeMissReason,
    GeneratedPropertyLoadProbeRequest, GeneratedPropertyLoadProbeResult,
    GeneratedPropertyStoreMegamorphicCacheEntry, GeneratedPropertyStoreMegamorphicCandidateTable,
    GeneratedPropertyStoreMegamorphicLookup, GeneratedPropertyStoreMegamorphicSite,
    GeneratedPropertyStoreProbeHit, GeneratedPropertyStoreProbeMiss,
    GeneratedPropertyStoreProbeMissReason, GeneratedPropertyStoreProbeRequest,
    GeneratedPropertyStoreProbeResult, InlineCacheBarrierMetadata, InlineCacheBarrierTarget,
    InlineCacheCaseClassification, InlineCacheDispatch, InlineCacheFallbackSemantics,
    InlineCacheInvalidation, InlineCacheInvalidationReason, InlineCacheKind,
    InlineCacheMissHandoffDescriptor, InlineCacheMissKind, InlineCacheRegistryMutationAuthority,
    InlineCacheSchemaOwner, InlineCacheSchemaRegistry, InlineCacheSemanticClass,
    InlineCacheSemanticSummary, InlineCacheSlot, InlineCacheSlotBuilder, InlineCacheSlotId,
    InlineCacheState, InlineCacheStub, InlineCacheStubId, InlineCacheStubKind,
    InlineCacheValidationError, LinkedCallKind, PropertyHasObservationDescriptor,
    PropertyLoadAccessCaseEffects, PropertyLoadAccessCasePlan, PropertyLoadAccessCasePlanContract,
    PropertyLoadAccessCasePlanKind, PropertyLoadAccessCasePlanTable, PropertyLoadAccessCaseRooting,
    PropertyLoadBaseNormalization, PropertyLoadDataOnlyOwnLoadEffects, PropertyLoadExitEffect,
    PropertyLoadGuardChainCertificate, PropertyLoadGuardChainEntry,
    PropertyLoadGuardChainEntryProof, PropertyLoadGuardChainOutcome,
    PropertyLoadGuardDependencyDescriptor, PropertyLoadGuardDescriptor, PropertyLoadGuardPlan,
    PropertyLoadGuardRequirement, PropertyLoadGuardedCandidate, PropertyLoadGuardedCandidateKind,
    PropertyLoadGuardedCandidateTable, PropertyLoadHeapEffect, PropertyLoadHostBoundaryEffect,
    PropertyLoadObservationBlocker, PropertyLoadObservationBlockers,
    PropertyLoadObservationChainEntry, PropertyLoadObservationDescriptor,
    PropertyLoadObservationReadiness, PropertyLoadResultEffect, PropertyLoadReturnedCellRooting,
    PropertyStoreAccessCaseEffects, PropertyStoreAccessCasePlan,
    PropertyStoreAccessCasePlanContract, PropertyStoreAccessCasePlanKind,
    PropertyStoreAccessCasePlanTable, PropertyStoreBarrierEffect,
    PropertyStoreDataOnlyReplaceEffects, PropertyStoreDataOnlyTransitionEffects,
    PropertyStoreExitEffect, PropertyStoreHeapEffect, PropertyStoreHostBoundaryEffect,
    PropertyStoreMutationBarrierEvidence, PropertyStoreMutationBarrierEvidenceMismatchField,
    PropertyStoreMutationCandidate, PropertyStoreMutationCandidateTable,
    PropertyStoreStoredValueEffect, StaticInlineCacheSchema, INLINE_CACHE_SCHEMA_REGISTRY,
    STATIC_INLINE_CACHE_SCHEMAS,
};
pub use integration::{
    descriptor_only_artifact_with_interpreter_fallback, descriptor_only_integration_record,
    fallback_invalidation_reason, ftl_descriptor_failure_artifact, interpreter_fallback_record,
    interpreter_stack_diagnostic_ordinal, plan_llint_offlineasm_integration,
    request_for_tier_fallback, CompilerIntegrationComponent, CompilerIntegrationError,
    DifferentialOutcome, InterpreterDifferentialRecord, LLIntOfflineAsmIntegrationRecord,
    LLIntOfflineAsmIntegrationRequest, OptimizedArtifactAvailability, OptimizedArtifactDescriptor,
    OptimizedTierDiagnostic, OptimizedTierIntegrationRecord,
};
pub use machine::{
    CodePatchPlan, CodePatchRecord, CodePatchState, ExecutableAllocationId,
    ExecutableAllocationLifecycle, ExecutableMemoryProtection, ExecutableMutationAuthority,
    MachineCodeHandle, MachineCodeOwnership, MachineCodeRange, MachineCodeValidationError,
    PatchWriteBarrier, RelocationKind,
};
pub use plan::{
    install_barriers_for_request, order_compilation_requests, BaselineBytecodeEligibilityProof,
    BaselineBytecodeEligibilityRecord, BaselineBytecodeInstruction, BaselineBytecodeRange,
    BaselineExceptionMetadataPresence, BaselineGeneratedEffectContract,
    BaselineGeneratedEffectRejectionReason, BaselineOpcodeEffect, BaselineOpcodeRejectionReason,
    BaselineRootMapRequirements, BaselineRootMapRequirementsProof, BaselineSupportedOpcodeSubset,
    CompilationCancellation, CompilationMode, CompilationOutcome, CompilationPriority,
    CompilationProduct, CompilationRequest, CompilationRequestBuilder, CompilationRequestKind,
    CompilerRootSlotDescriptor, CompilerRootSlotLocation, CompilerSafepointDescriptor,
    CompilerSafepointId, CompilerSafepointKind, CompilerSafepointRootBinding,
    CompilerSafepointRootTarget, CompilerSafepointTargetedRootPlan, JitCompilationKeyId,
    JitPlanDescriptorRegistry, JitPlanLifecycle, JitPlanRegistryMutationAuthority,
    JitPlanSchemaOwner, JitPlanSchemaProvenance, JitPlanStage, JitPlanTier, JitPlanValidationError,
    JitWorklistDescriptor, JitWorklistId, JitWorklistPlanState, JitWorklistThreadState,
    StaticJitPlanDescriptor, JIT_PLAN_DESCRIPTOR_REGISTRY, STATIC_JIT_PLAN_DESCRIPTORS,
};
pub use plan::{CompilationPlan, CompilationPlanId, CompilationPlanState, JitPlanHost};
pub use semantics::EffectSummary;
pub use tiering::{
    select_tier_plan, BaselineTierPlan, OptimizingTierPlan, OsrState, StaticTierDescriptor,
    TierCounters, TierDescriptorMutationAuthority, TierDescriptorOwner, TierDescriptorTable,
    TierFallbackReason, TierFallbackResultRecord, TierFallbackResumeKind, TierFallbackSemantics,
    TierFallbackTarget, TierPlanDescriptor, TierPlanKind, TierPlanPriorityHint, TierPlanProfile,
    TierThresholds, TierTransition, TieringPolicy, TieringSnapshot, TieringState, TieringTrigger,
    TieringValidationError, STATIC_TIER_DESCRIPTORS, TIER_DESCRIPTOR_TABLE,
};
pub use watchpoint::{
    DependencyStrength, WatchpointDependency, WatchpointDependencyId, WatchpointFireEvent,
    WatchpointFirePolicy, WatchpointOwner, WatchpointSetDescriptor, WatchpointSetId,
    WatchpointSetState, WatchpointTarget,
};
