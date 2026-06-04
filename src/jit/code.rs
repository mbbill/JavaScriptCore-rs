//! Code-block side-data reserved for future JIT tiers.
//!
//! The owning code-block-equivalent module should store this as optional side
//! data. Interpreter semantics must not require any field here to be populated.
//! This module describes ownership, liveness, and invalidation boundaries only;
//! executable allocation, patching, and deallocation remain deferred.

use crate::jit::abi::{BaselineAbiDescriptor, BaselineAbiValidationError, BASELINE_ABI_DESCRIPTOR};
use crate::jit::plan::{
    BaselineBytecodeEligibilityProof, BaselineGeneratedEffectContract,
    BaselineGeneratedOwnerContinuationMapMetadata, BaselineGeneratedPropertyHandoffPlan,
    BaselineGeneratedPropertyHandoffPlanMetadata, BaselineGeneratedRuntimeHelperPlan,
    BaselineGeneratedRuntimeHelperPlanMetadata, BaselineSupportedOpcodeSubset,
};
use crate::jit::{
    DisassemblyMetadata, EntryAbi, Entrypoint, EntrypointKind, ExecutableAllocationLifecycle,
    ExecutableMemoryProtection, InlineCacheSlot, MachineCodeHandle, MachineCodeOwnership,
    MachineCodeRange, PatchpointDescriptor, TieringState, WatchpointDependency, WatchpointSetId,
};
use crate::runtime::{CodeBlockId, ExecutableId, NativeCodeId};

/// Execution tier represented by a code-block-equivalent object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitType {
    None,
    InterpreterThunk,
    Baseline,
    Dfg,
    Ftl,
    WasmIpInt,
    WasmBbq,
    WasmOmg,
}

/// Stable identity for future compiled code storage.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JitCodeId(pub u64);

/// Opaque reference to future compiled code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitCodeRef {
    pub id: JitCodeId,
    pub tier: JitType,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JitOpaqueByproductId(pub u64);

/// GC and invalidation status for generated-code side data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeLiveness {
    Unallocated,
    Compiling,
    Live,
    PendingInvalidation,
    PendingJettison,
    Invalidated,
    Finalized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeFinalizationAuthority {
    CompilerThread,
    MainThread,
    GcFinalizer,
    ExecutableAllocator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeRetentionPolicy {
    CodeBlockKeepsAlive,
    SharedStubRegistry,
    ImmortalThunk,
    GcWeakUntilInstalled,
    ExternalOwner,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueByproductDescriptor {
    pub id: JitOpaqueByproductId,
    pub owner_code: JitCodeId,
    pub retention: CodeRetentionPolicy,
    pub traced_edges_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JitCodeValidationError {
    EntrypointTierMismatch,
    NativeAndMachineCodeMismatch,
    MachineCodeInvalid,
    PatchpointOwnerMismatch,
    DependencyBarrierMissing,
    ByproductOwnerMismatch(JitOpaqueByproductId),
    SuccessfulCodeNotLive,
    InvalidationReasonMissing,
    BaselineEntryTierMismatch,
    BaselineEntryOriginMismatch,
    BaselineEntryOwnerMissing,
    BaselineEntryOwnerMismatch,
    BaselineEntryOwnershipMismatch,
    BaselineEntryNotLive,
    BaselineEntryMissingNativeCode,
    BaselineEntryEntrypointMissing,
    BaselineEntryMachineOwnerMismatch,
    BaselineEntryNativeSymbolMismatch,
    BaselineEntryMachineCodeNotExecutable,
    BaselineEntryAbiDescriptorInvalid,
    BaselineNativeEntryTierMismatch,
    BaselineNativeEntryOriginMismatch,
    BaselineNativeEntryOwnerMissing,
    BaselineNativeEntryOwnerMismatch,
    BaselineNativeEntryOwnershipMismatch,
    BaselineNativeEntryNotLive,
    BaselineNativeEntryMachineCodeInvalid,
    BaselineNativeEntryMachineOwnerMismatch,
    BaselineNativeEntryNativeSymbolMismatch,
    BaselineNativeEntryMachineCodeNotExecutable,
    BaselineNativeEntryEntrypointMismatch,
    BaselineNativeEntryAbiDescriptorInvalid,
    BaselineGeneratedCodeOwnerMismatch,
    BaselineGeneratedCodeNotLive,
    BaselineGeneratedCodeOpcodeSubsetMismatch,
    BaselineGeneratedCodeEffectContractMismatch,
    BaselineGeneratedCodeAbiDescriptorInvalid,
    BaselineGeneratedRuntimeHelperPlanSnapshotMismatch,
    BaselineGeneratedPropertyHandoffPlanSnapshotMismatch,
    BaselineGeneratedOwnerContinuationMapSnapshotMismatch,
}

/// JIT-visible side-data slots reserved on linked code state.
///
/// Slot ownership belongs to the code-block-equivalent runtime owner. The JIT
/// borrows `CodeBlockId` for liveness checks and owns only tier side data,
/// invalidation metadata, and generated-code descriptors stored here.
#[derive(Clone, Debug)]
pub struct CodeBlockJitSlots {
    pub owner: Option<CodeBlockId>,
    pub tier: JitType,
    pub entrypoint: Entrypoint,
    pub code: Option<JitCodeRef>,
    pub tiering: TieringState,
    pub liveness: CodeLiveness,
    pub inline_caches: Vec<InlineCacheSlot>,
    pub watchpoints: Vec<WatchpointDependency>,
    pub invalidation: CodeInvalidationState,
}

/// Provenance for a future compiled artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeOriginKind {
    BaselineCodeBlock,
    DfgReplacement,
    FtlReplacement,
    OsrEntry,
    InlineCacheStub,
    HostThunk,
    WasmFunction,
    WasmBridge,
}

/// Origin metadata used for ownership and diagnostic reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeOrigin {
    pub kind: CodeOriginKind,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub bytecode_index: Option<u32>,
}

/// Ownership mode for generated code storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeOwnership {
    CodeBlockOwned,
    SharedStubSet,
    WasmCalleeGroup,
    HostRegistry,
    External,
}

/// Reserved compiled-code artifact descriptor.
///
/// The artifact owns generated-code metadata but not necessarily executable
/// memory lifetime. `CodeOwnership` describes the retention owner, while
/// `finalization_authority` names who may mutate install/destruction state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitCodeArtifact {
    pub id: JitCodeId,
    pub tier: JitType,
    pub origin: CodeOrigin,
    pub ownership: CodeOwnership,
    pub native_code: Option<NativeCodeId>,
    pub machine_code: Option<MachineCodeHandle>,
    pub entrypoint: Entrypoint,
    pub patchpoints: Vec<PatchpointDescriptor>,
    pub dependencies: Vec<WatchpointDependency>,
    pub byproducts: Vec<OpaqueByproductDescriptor>,
    pub disassembly: Option<DisassemblyMetadata>,
    pub liveness: CodeLiveness,
    /// Installation and destruction authority is intentionally separate from
    /// code ownership because C++ finalizers may run on compiler, main, or GC
    /// paths depending on tier and liveness.
    pub finalization_authority: CodeFinalizationAuthority,
}

/// Data needed before the VM may consider a baseline artifact entry-eligible.
///
/// This is still metadata only: it proves that a live generated-code artifact is
/// represented and owned coherently, but it is not a callable function pointer
/// and does not allocate executable memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineAbiProof {
    descriptor: BaselineAbiDescriptor,
}

impl BaselineAbiProof {
    fn new(descriptor: &BaselineAbiDescriptor) -> Result<Self, BaselineAbiValidationError> {
        descriptor.validate()?;
        Ok(Self {
            descriptor: *descriptor,
        })
    }

    #[cfg(test)]
    pub(crate) fn descriptor(&self) -> BaselineAbiDescriptor {
        self.descriptor
    }

    pub(crate) fn proves_descriptor(&self, descriptor: &BaselineAbiDescriptor) -> bool {
        self.descriptor == *descriptor && self.descriptor.validate().is_ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineEntryArtifact {
    pub id: JitCodeId,
    pub tier: JitType,
    pub owner: CodeBlockId,
    pub origin: CodeOrigin,
    pub ownership: CodeOwnership,
    pub native_code: NativeCodeId,
    pub machine_code: MachineCodeHandle,
    pub entrypoint: Entrypoint,
    pub liveness: CodeLiveness,
    pub finalization_authority: CodeFinalizationAuthority,
    pub baseline_abi_proof: BaselineAbiProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineNativeEntryToken {
    pub owner: CodeBlockId,
    pub artifact_id: JitCodeId,
    pub native_symbol: NativeCodeId,
    pub machine_code: MachineCodeHandle,
    pub entrypoint: Entrypoint,
    pub kind: BaselineNativeEntryTokenKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineNativeEntryTokenKind {
    Normal,
    ArityCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineArityCheckNativeEntry {
    Token(BaselineNativeEntryToken),
    Unavailable(BaselineArityCheckUnavailableReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineArityCheckUnavailableReason {
    NotEmitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineNativeEntryCallableKind {
    P6PureBaselineNativeEntryShim,
    P6X86_64EmittedSemanticCAbiEntry,
    P6Arm64EmittedSemanticCAbiEntry,
}

impl BaselineNativeEntryCallableKind {
    pub const fn supported_opcode_subset(self) -> BaselineSupportedOpcodeSubset {
        match self {
            Self::P6PureBaselineNativeEntryShim => {
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary
            }
            Self::P6X86_64EmittedSemanticCAbiEntry => {
                BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse
            }
            Self::P6Arm64EmittedSemanticCAbiEntry => {
                // This reports the shared P6 lowering/proof envelope used at
                // install time. The current ARM64 callable is a narrower return
                // seed; arithmetic in this envelope is intentionally rejected by
                // the ARM64 encoder and falls back to the existing generated
                // baseline/x86_64-semantic artifact path.
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineNativeEntryDescriptor {
    pub owner: CodeBlockId,
    pub artifact_id: JitCodeId,
    pub native_symbol: NativeCodeId,
    pub machine_code: MachineCodeHandle,
    pub machine_range: MachineCodeRange,
    pub entrypoint: Entrypoint,
    pub baseline_abi_proof: BaselineAbiProof,
    pub normal_entry: BaselineNativeEntryToken,
    pub arity_check_entry: BaselineArityCheckNativeEntry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaselineNativeEntryCallableValidationError {
    OwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ArtifactIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    NativeCodeMismatch {
        expected: NativeCodeId,
        actual: NativeCodeId,
    },
    MachineCodeMismatch {
        expected: Box<MachineCodeHandle>,
        actual: Box<MachineCodeHandle>,
    },
    MachineRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    EntrypointMismatch {
        expected: Entrypoint,
        actual: Entrypoint,
    },
    AbiProofMismatch,
    TokenMismatch {
        expected: Box<BaselineNativeEntryToken>,
        actual: Box<BaselineNativeEntryToken>,
    },
    SealMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BaselineNativeEntryCallableSeal {
    owner: CodeBlockId,
    artifact_id: JitCodeId,
    native_symbol: NativeCodeId,
    machine_code: MachineCodeHandle,
    entrypoint: Entrypoint,
}

/// Opaque authority for crossing the native-entry boundary.
///
/// This authority can represent either a Rust shim or an emitted semantic
/// x86_64/ARM64 C-ABI entry. Public native-entry descriptor/token metadata can
/// describe a symbolic entrypoint, but VM enabled execution additionally
/// requires this sealed value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineNativeEntryCallableAuthority {
    kind: BaselineNativeEntryCallableKind,
    descriptor: BaselineNativeEntryDescriptor,
    token: BaselineNativeEntryToken,
    seal: BaselineNativeEntryCallableSeal,
}

impl BaselineNativeEntryCallableAuthority {
    #[allow(dead_code)]
    pub(crate) fn new_p6_pure_baseline_native_entry_shim(
        descriptor: BaselineNativeEntryDescriptor,
    ) -> Self {
        Self::from_descriptor_and_token(
            BaselineNativeEntryCallableKind::P6PureBaselineNativeEntryShim,
            descriptor,
            descriptor.normal_entry,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_p6_x86_64_emitted_semantic_c_abi_entry(
        descriptor: BaselineNativeEntryDescriptor,
    ) -> Self {
        Self::from_descriptor_and_token(
            BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
            descriptor,
            descriptor.normal_entry,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_p6_arm64_emitted_semantic_c_abi_entry(
        descriptor: BaselineNativeEntryDescriptor,
    ) -> Self {
        Self::from_descriptor_and_token(
            BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
            descriptor,
            descriptor.normal_entry,
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_descriptor_and_token(
        kind: BaselineNativeEntryCallableKind,
        descriptor: BaselineNativeEntryDescriptor,
        token: BaselineNativeEntryToken,
    ) -> Self {
        Self::from_descriptor_and_token(kind, descriptor, token)
    }

    #[allow(dead_code)]
    fn from_descriptor_and_token(
        kind: BaselineNativeEntryCallableKind,
        descriptor: BaselineNativeEntryDescriptor,
        token: BaselineNativeEntryToken,
    ) -> Self {
        Self {
            kind,
            descriptor,
            token,
            seal: BaselineNativeEntryCallableSeal {
                owner: descriptor.owner,
                artifact_id: descriptor.artifact_id,
                native_symbol: descriptor.native_symbol,
                machine_code: descriptor.machine_code,
                entrypoint: descriptor.entrypoint,
            },
        }
    }

    pub(crate) const fn kind(self) -> BaselineNativeEntryCallableKind {
        self.kind
    }

    pub(crate) const fn descriptor(self) -> BaselineNativeEntryDescriptor {
        self.descriptor
    }

    pub(crate) const fn token(self) -> BaselineNativeEntryToken {
        self.token
    }

    pub(crate) fn validate_for_descriptor(
        self,
        descriptor: &BaselineNativeEntryDescriptor,
    ) -> Result<(), BaselineNativeEntryCallableValidationError> {
        let actual = self.descriptor;
        if actual.owner != descriptor.owner {
            return Err(BaselineNativeEntryCallableValidationError::OwnerMismatch {
                expected: descriptor.owner,
                actual: actual.owner,
            });
        }
        if actual.artifact_id != descriptor.artifact_id {
            return Err(
                BaselineNativeEntryCallableValidationError::ArtifactIdMismatch {
                    expected: descriptor.artifact_id,
                    actual: actual.artifact_id,
                },
            );
        }
        if actual.native_symbol != descriptor.native_symbol {
            return Err(
                BaselineNativeEntryCallableValidationError::NativeCodeMismatch {
                    expected: descriptor.native_symbol,
                    actual: actual.native_symbol,
                },
            );
        }
        if actual.machine_code != descriptor.machine_code {
            return Err(
                BaselineNativeEntryCallableValidationError::MachineCodeMismatch {
                    expected: Box::new(descriptor.machine_code),
                    actual: Box::new(actual.machine_code),
                },
            );
        }
        if actual.machine_range != descriptor.machine_range {
            return Err(
                BaselineNativeEntryCallableValidationError::MachineRangeMismatch {
                    expected: descriptor.machine_range,
                    actual: actual.machine_range,
                },
            );
        }
        if actual.entrypoint != descriptor.entrypoint {
            return Err(
                BaselineNativeEntryCallableValidationError::EntrypointMismatch {
                    expected: descriptor.entrypoint,
                    actual: actual.entrypoint,
                },
            );
        }
        if actual.baseline_abi_proof != descriptor.baseline_abi_proof {
            return Err(BaselineNativeEntryCallableValidationError::AbiProofMismatch);
        }
        if self.token != descriptor.normal_entry {
            return Err(BaselineNativeEntryCallableValidationError::TokenMismatch {
                expected: Box::new(descriptor.normal_entry),
                actual: Box::new(self.token),
            });
        }
        if self.seal.owner != descriptor.owner
            || self.seal.artifact_id != descriptor.artifact_id
            || self.seal.native_symbol != descriptor.native_symbol
            || self.seal.machine_code != descriptor.machine_code
            || self.seal.entrypoint != descriptor.entrypoint
        {
            return Err(BaselineNativeEntryCallableValidationError::SealMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BaselineGeneratedCodeBodyId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedCodeBody {
    pub id: BaselineGeneratedCodeBodyId,
    pub supported_opcode_subset: Option<BaselineSupportedOpcodeSubset>,
    pub effect_contract: Option<BaselineGeneratedEffectContract>,
}

impl BaselineGeneratedCodeBody {
    pub const fn new(
        id: BaselineGeneratedCodeBodyId,
        supported_opcode_subset: BaselineSupportedOpcodeSubset,
    ) -> Self {
        Self {
            id,
            supported_opcode_subset: Some(supported_opcode_subset),
            effect_contract: Some(supported_opcode_subset.generated_effect_contract()),
        }
    }
}

/// Copyable coverage/effect evidence for a compiled baseline body.
///
/// C++ JSC publishes a `BaselineJITCode` object whose finalized `CodeRef` owns
/// the executable body and entrypoints. Rust keeps this proof projection
/// separate from native-entry callable kind so readiness/admission can validate
/// body coverage without treating a callable-kind table as generated-code proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedCodeBodyCapability {
    pub supported_opcode_subset: BaselineSupportedOpcodeSubset,
    pub effect_contract: BaselineGeneratedEffectContract,
}

impl BaselineGeneratedCodeBodyCapability {
    pub const fn from_supported_opcode_subset(
        supported_opcode_subset: BaselineSupportedOpcodeSubset,
    ) -> Self {
        Self {
            supported_opcode_subset,
            effect_contract: supported_opcode_subset.generated_effect_contract(),
        }
    }

    pub fn from_eligibility_proof(proof: &BaselineBytecodeEligibilityProof) -> Self {
        Self {
            supported_opcode_subset: proof.opcode_subset(),
            effect_contract: proof.generated_effect_contract(),
        }
    }

    pub fn from_body(body: &BaselineGeneratedCodeBody) -> Option<Self> {
        let supported_opcode_subset = body.supported_opcode_subset?;
        let effect_contract = body.effect_contract?;
        if effect_contract != supported_opcode_subset.generated_effect_contract() {
            return None;
        }
        Some(Self {
            supported_opcode_subset,
            effect_contract,
        })
    }

    pub fn matches_eligibility_proof(self, proof: &BaselineBytecodeEligibilityProof) -> bool {
        self == Self::from_eligibility_proof(proof)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedCodeArtifact {
    pub id: JitCodeId,
    pub owner: CodeBlockId,
    pub eligibility_proof: BaselineBytecodeEligibilityProof,
    pub baseline_abi_proof: BaselineAbiProof,
    pub body: BaselineGeneratedCodeBody,
    pub(crate) runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlanMetadata>,
    pub(crate) property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlanMetadata>,
    pub(crate) owner_continuation_map: Option<BaselineGeneratedOwnerContinuationMapMetadata>,
    pub liveness: CodeLiveness,
    pub finalization_authority: CodeFinalizationAuthority,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedCodeMetadataPlans {
    pub(crate) runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlanMetadata>,
    pub(crate) property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlanMetadata>,
    pub(crate) owner_continuation_map: Option<BaselineGeneratedOwnerContinuationMapMetadata>,
}

impl BaselineGeneratedCodeMetadataPlans {
    pub(crate) fn new(
        runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlanMetadata>,
        property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlanMetadata>,
        owner_continuation_map: Option<BaselineGeneratedOwnerContinuationMapMetadata>,
    ) -> Self {
        Self {
            runtime_helper_plan,
            property_handoff_plan,
            owner_continuation_map,
        }
    }
}

impl BaselineGeneratedCodeArtifact {
    pub fn new(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
    ) -> Result<Self, JitCodeValidationError> {
        Self::new_with_abi_descriptor(
            id,
            owner,
            eligibility_proof,
            body,
            liveness,
            finalization_authority,
            &BASELINE_ABI_DESCRIPTOR,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_with_runtime_helper_plan(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        runtime_helper_plan: BaselineGeneratedRuntimeHelperPlanMetadata,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
    ) -> Result<Self, JitCodeValidationError> {
        Self::new_with_abi_descriptor_and_runtime_helper_plan(
            id,
            owner,
            eligibility_proof,
            body,
            Some(runtime_helper_plan),
            liveness,
            finalization_authority,
            &BASELINE_ABI_DESCRIPTOR,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_with_property_handoff_plan(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        property_handoff_plan: BaselineGeneratedPropertyHandoffPlanMetadata,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
    ) -> Result<Self, JitCodeValidationError> {
        Self::new_with_abi_descriptor_and_metadata_plans(
            id,
            owner,
            eligibility_proof,
            body,
            None,
            Some(property_handoff_plan),
            None,
            liveness,
            finalization_authority,
            &BASELINE_ABI_DESCRIPTOR,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_with_runtime_helper_and_property_handoff_plans(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        metadata_plans: BaselineGeneratedCodeMetadataPlans,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
    ) -> Result<Self, JitCodeValidationError> {
        Self::new_with_abi_descriptor_and_metadata_plans(
            id,
            owner,
            eligibility_proof,
            body,
            metadata_plans.runtime_helper_plan,
            metadata_plans.property_handoff_plan,
            metadata_plans.owner_continuation_map,
            liveness,
            finalization_authority,
            &BASELINE_ABI_DESCRIPTOR,
        )
    }

    fn new_with_abi_descriptor(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
        baseline_abi_descriptor: &BaselineAbiDescriptor,
    ) -> Result<Self, JitCodeValidationError> {
        Self::new_with_abi_descriptor_and_runtime_helper_plan(
            id,
            owner,
            eligibility_proof,
            body,
            None,
            liveness,
            finalization_authority,
            baseline_abi_descriptor,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_abi_descriptor_and_runtime_helper_plan(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlanMetadata>,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
        baseline_abi_descriptor: &BaselineAbiDescriptor,
    ) -> Result<Self, JitCodeValidationError> {
        Self::new_with_abi_descriptor_and_metadata_plans(
            id,
            owner,
            eligibility_proof,
            body,
            runtime_helper_plan,
            None,
            None,
            liveness,
            finalization_authority,
            baseline_abi_descriptor,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_abi_descriptor_and_metadata_plans(
        id: JitCodeId,
        owner: CodeBlockId,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        body: BaselineGeneratedCodeBody,
        runtime_helper_plan: Option<BaselineGeneratedRuntimeHelperPlanMetadata>,
        property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlanMetadata>,
        owner_continuation_map: Option<BaselineGeneratedOwnerContinuationMapMetadata>,
        liveness: CodeLiveness,
        finalization_authority: CodeFinalizationAuthority,
        baseline_abi_descriptor: &BaselineAbiDescriptor,
    ) -> Result<Self, JitCodeValidationError> {
        let baseline_abi_proof = BaselineAbiProof::new(baseline_abi_descriptor)
            .map_err(|_| JitCodeValidationError::BaselineGeneratedCodeAbiDescriptorInvalid)?;
        let artifact = Self {
            id,
            owner,
            eligibility_proof,
            baseline_abi_proof,
            body,
            runtime_helper_plan,
            property_handoff_plan,
            owner_continuation_map,
            liveness,
            finalization_authority,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), JitCodeValidationError> {
        if self.liveness != CodeLiveness::Live {
            return Err(JitCodeValidationError::BaselineGeneratedCodeNotLive);
        }
        if self.eligibility_proof.owner() != self.owner {
            return Err(JitCodeValidationError::BaselineGeneratedCodeOwnerMismatch);
        }
        if self.body.supported_opcode_subset != Some(self.eligibility_proof.opcode_subset()) {
            return Err(JitCodeValidationError::BaselineGeneratedCodeOpcodeSubsetMismatch);
        }
        if self.body.effect_contract != Some(self.eligibility_proof.generated_effect_contract()) {
            return Err(JitCodeValidationError::BaselineGeneratedCodeEffectContractMismatch);
        }
        if !self
            .baseline_abi_proof
            .proves_descriptor(&BASELINE_ABI_DESCRIPTOR)
        {
            return Err(JitCodeValidationError::BaselineGeneratedCodeAbiDescriptorInvalid);
        }
        if let Some(runtime_helper_plan) = &self.runtime_helper_plan {
            if runtime_helper_plan.bytecode_snapshot()
                != self.eligibility_proof.bytecode_snapshot_fingerprint()
            {
                return Err(
                    JitCodeValidationError::BaselineGeneratedRuntimeHelperPlanSnapshotMismatch,
                );
            }
        }
        if let Some(property_handoff_plan) = &self.property_handoff_plan {
            if property_handoff_plan.bytecode_snapshot()
                != self.eligibility_proof.bytecode_snapshot_fingerprint()
            {
                return Err(
                    JitCodeValidationError::BaselineGeneratedPropertyHandoffPlanSnapshotMismatch,
                );
            }
        }
        if let Some(owner_continuation_map) = &self.owner_continuation_map {
            if owner_continuation_map.bytecode_snapshot()
                != self.eligibility_proof.bytecode_snapshot_fingerprint()
            {
                return Err(
                    JitCodeValidationError::BaselineGeneratedOwnerContinuationMapSnapshotMismatch,
                );
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn runtime_helper_plan(&self) -> Option<BaselineGeneratedRuntimeHelperPlan<'_>> {
        self.runtime_helper_plan
            .as_ref()
            .map(BaselineGeneratedRuntimeHelperPlanMetadata::borrowed_plan)
    }

    #[allow(dead_code)]
    pub(crate) fn property_handoff_plan(&self) -> Option<BaselineGeneratedPropertyHandoffPlan<'_>> {
        self.property_handoff_plan
            .as_ref()
            .map(BaselineGeneratedPropertyHandoffPlanMetadata::borrowed_plan)
    }

    #[allow(dead_code)]
    pub(crate) fn owner_continuation_map(
        &self,
    ) -> Option<&BaselineGeneratedOwnerContinuationMapMetadata> {
        self.owner_continuation_map.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn has_valid_baseline_abi_proof(&self) -> bool {
        self.baseline_abi_proof
            .proves_descriptor(&BASELINE_ABI_DESCRIPTOR)
    }
}

impl BaselineEntryArtifact {
    pub fn validate_native_entry_descriptor(
        &self,
    ) -> Result<BaselineNativeEntryDescriptor, JitCodeValidationError> {
        if self.tier != JitType::Baseline {
            return Err(JitCodeValidationError::BaselineNativeEntryTierMismatch);
        }
        if self.origin.kind != CodeOriginKind::BaselineCodeBlock {
            return Err(JitCodeValidationError::BaselineNativeEntryOriginMismatch);
        }
        match self.origin.owner {
            Some(origin_owner) if origin_owner == self.owner => {}
            Some(_) => return Err(JitCodeValidationError::BaselineNativeEntryOwnerMismatch),
            None => return Err(JitCodeValidationError::BaselineNativeEntryOwnerMissing),
        }
        if self.ownership != CodeOwnership::CodeBlockOwned {
            return Err(JitCodeValidationError::BaselineNativeEntryOwnershipMismatch);
        }
        if self.liveness != CodeLiveness::Live {
            return Err(JitCodeValidationError::BaselineNativeEntryNotLive);
        }
        self.machine_code
            .validate()
            .map_err(|_| JitCodeValidationError::BaselineNativeEntryMachineCodeInvalid)?;
        if self.machine_code.owner != MachineCodeOwnership::CodeBlock(self.owner) {
            return Err(JitCodeValidationError::BaselineNativeEntryMachineOwnerMismatch);
        }
        if self.machine_code.symbol != Some(self.native_code) {
            return Err(JitCodeValidationError::BaselineNativeEntryNativeSymbolMismatch);
        }
        if self.machine_code.protection != ExecutableMemoryProtection::Executable
            || self.machine_code.lifecycle != ExecutableAllocationLifecycle::LinkedExecutable
        {
            return Err(JitCodeValidationError::BaselineNativeEntryMachineCodeNotExecutable);
        }
        if self.entrypoint.kind != EntrypointKind::GeneratedCode
            || self.entrypoint.abi != EntryAbi::GeneratedCode
            || self.entrypoint.code != Some(self.id)
        {
            return Err(JitCodeValidationError::BaselineNativeEntryEntrypointMismatch);
        }
        if !self.has_valid_baseline_abi_proof() {
            return Err(JitCodeValidationError::BaselineNativeEntryAbiDescriptorInvalid);
        }

        let normal_entry = BaselineNativeEntryToken {
            owner: self.owner,
            artifact_id: self.id,
            native_symbol: self.native_code,
            machine_code: self.machine_code,
            entrypoint: self.entrypoint,
            kind: BaselineNativeEntryTokenKind::Normal,
        };

        Ok(BaselineNativeEntryDescriptor {
            owner: self.owner,
            artifact_id: self.id,
            native_symbol: self.native_code,
            machine_code: self.machine_code,
            machine_range: self.machine_code.range,
            entrypoint: self.entrypoint,
            baseline_abi_proof: self.baseline_abi_proof,
            normal_entry,
            arity_check_entry: BaselineArityCheckNativeEntry::Unavailable(
                BaselineArityCheckUnavailableReason::NotEmitted,
            ),
        })
    }

    pub(crate) fn has_valid_baseline_abi_proof(&self) -> bool {
        self.baseline_abi_proof
            .proves_descriptor(&BASELINE_ABI_DESCRIPTOR)
    }
}

impl JitCodeArtifact {
    pub fn validate(&self) -> Result<(), JitCodeValidationError> {
        if let Some(machine_code) = self.machine_code {
            machine_code
                .validate()
                .map_err(|_| JitCodeValidationError::MachineCodeInvalid)?;
        }
        if self.native_code.is_some() != self.machine_code.is_some() {
            return Err(JitCodeValidationError::NativeAndMachineCodeMismatch);
        }
        if self.entrypoint.code.is_some() && self.entrypoint.code != Some(self.id) {
            return Err(JitCodeValidationError::EntrypointTierMismatch);
        }
        for patchpoint in &self.patchpoints {
            if patchpoint.owner_code.is_some() && patchpoint.owner_code != Some(self.id) {
                return Err(JitCodeValidationError::PatchpointOwnerMismatch);
            }
        }
        if !self.dependencies.is_empty()
            && !self
                .patchpoints
                .iter()
                .any(|patchpoint| patchpoint.boundary.is_some())
        {
            return Err(JitCodeValidationError::DependencyBarrierMissing);
        }
        for byproduct in &self.byproducts {
            if byproduct.owner_code != self.id {
                return Err(JitCodeValidationError::ByproductOwnerMismatch(byproduct.id));
            }
        }
        if matches!(self.liveness, CodeLiveness::Live)
            && self.native_code.is_none()
            && !matches!(self.ownership, CodeOwnership::HostRegistry)
        {
            return Err(JitCodeValidationError::SuccessfulCodeNotLive);
        }

        Ok(())
    }

    pub fn validate_baseline_entry_artifact(
        &self,
        owner: CodeBlockId,
    ) -> Result<BaselineEntryArtifact, JitCodeValidationError> {
        self.validate_baseline_entry_artifact_with_abi_descriptor(owner, &BASELINE_ABI_DESCRIPTOR)
    }

    fn validate_baseline_entry_artifact_with_abi_descriptor(
        &self,
        owner: CodeBlockId,
        baseline_abi_descriptor: &BaselineAbiDescriptor,
    ) -> Result<BaselineEntryArtifact, JitCodeValidationError> {
        self.validate()?;
        let baseline_abi_proof = BaselineAbiProof::new(baseline_abi_descriptor)
            .map_err(|_| JitCodeValidationError::BaselineEntryAbiDescriptorInvalid)?;
        if self.tier != JitType::Baseline {
            return Err(JitCodeValidationError::BaselineEntryTierMismatch);
        }
        if self.origin.kind != CodeOriginKind::BaselineCodeBlock {
            return Err(JitCodeValidationError::BaselineEntryOriginMismatch);
        }
        match self.origin.owner {
            Some(origin_owner) if origin_owner == owner => {}
            Some(_) => return Err(JitCodeValidationError::BaselineEntryOwnerMismatch),
            None => return Err(JitCodeValidationError::BaselineEntryOwnerMissing),
        }
        if self.ownership != CodeOwnership::CodeBlockOwned {
            return Err(JitCodeValidationError::BaselineEntryOwnershipMismatch);
        }
        if self.liveness != CodeLiveness::Live {
            return Err(JitCodeValidationError::BaselineEntryNotLive);
        }
        if self.entrypoint.kind != EntrypointKind::GeneratedCode
            || self.entrypoint.abi != EntryAbi::GeneratedCode
            || self.entrypoint.code != Some(self.id)
        {
            return Err(JitCodeValidationError::BaselineEntryEntrypointMissing);
        }
        let native_code = self
            .native_code
            .ok_or(JitCodeValidationError::BaselineEntryMissingNativeCode)?;
        let machine_code = self
            .machine_code
            .ok_or(JitCodeValidationError::BaselineEntryMissingNativeCode)?;
        if machine_code.owner != MachineCodeOwnership::CodeBlock(owner) {
            return Err(JitCodeValidationError::BaselineEntryMachineOwnerMismatch);
        }
        if machine_code.symbol != Some(native_code) {
            return Err(JitCodeValidationError::BaselineEntryNativeSymbolMismatch);
        }
        if machine_code.protection != ExecutableMemoryProtection::Executable
            || machine_code.lifecycle != ExecutableAllocationLifecycle::LinkedExecutable
        {
            return Err(JitCodeValidationError::BaselineEntryMachineCodeNotExecutable);
        }

        Ok(BaselineEntryArtifact {
            id: self.id,
            tier: self.tier,
            owner,
            origin: self.origin,
            ownership: self.ownership,
            native_code,
            machine_code,
            entrypoint: self.entrypoint,
            liveness: self.liveness,
            finalization_authority: self.finalization_authority,
            baseline_abi_proof,
        })
    }
}

/// Boundary that must be crossed before code can be installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeInstallBarrier {
    OwnerStillLive,
    WatchpointsStillValid,
    StructureEpochUnchanged,
    ExecutableStillMatches,
    WasmInstanceStillLive,
    MainThreadFinalization,
}

/// Invalidation state carried by linked code and stubs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeInvalidationState {
    pub epoch: u64,
    pub reason: Option<CodeInvalidationReason>,
    pub watchpoint_sets: Vec<WatchpointSetId>,
    pub barriers: Vec<CodeInstallBarrier>,
}

impl CodeInvalidationState {
    pub fn validate(&self) -> Result<(), JitCodeValidationError> {
        if !self.watchpoint_sets.is_empty()
            && !self
                .barriers
                .contains(&CodeInstallBarrier::WatchpointsStillValid)
        {
            return Err(JitCodeValidationError::DependencyBarrierMissing);
        }
        if self.epoch > 0 && self.reason.is_none() {
            return Err(JitCodeValidationError::InvalidationReasonMissing);
        }

        Ok(())
    }
}

/// Reason code is no longer installable or executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeInvalidationReason {
    WatchpointFired,
    GeneratedInlineCacheDependencyInvalidated,
    OwnerCodeBlockJettisoned,
    OwnerExecutableReplaced,
    WeakReferenceCleared,
    TierReplacementInstalled,
    CompilationCancelled,
    WasmMemoryModeChanged,
    WasmCalleeReplaced,
}

/// Code replacement edge between tiers or OSR entry artifacts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeReplacement {
    pub old_code: Option<JitCodeId>,
    pub new_code: JitCodeId,
    pub owner: CodeBlockId,
    pub install_epoch: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        BytecodeIndex, BytecodeRootMap, BytecodeRootMapId, BytecodeRootSlotDescriptor,
        BytecodeRootSlotKind, CoreOpcode, VirtualRegister,
    };
    use crate::gc::CellId;
    use crate::jit::plan::{
        BaselineGeneratedOwnerBytecodeLabel, BaselineGeneratedOwnerContinuationMapMetadata,
        BaselineGeneratedRuntimeBoundaryCandidate, BaselineGeneratedRuntimeBoundaryProof,
        BaselineGeneratedRuntimeHelperPlanMetadata, BaselineGeneratedRuntimeHelperProof,
        CompilerSafepointDescriptor, CompilerSafepointId, CompilerSafepointKind,
    };
    use crate::jit::{
        BaselineBytecodeEligibilityRecord, BaselineBytecodeInstruction, BaselineBytecodeRange,
        BaselineExceptionMetadataPresence, BaselineRootMapRequirements, ExecutableAllocationId,
        ExecutableMutationAuthority, JitPlanValidationError, MachineCodeRange, TieringSnapshot,
        TieringTrigger,
    };

    fn owner() -> CodeBlockId {
        CodeBlockId(CellId(1))
    }

    fn baseline_artifact(owner: CodeBlockId) -> JitCodeArtifact {
        let code = JitCodeId(7);
        let native_code = NativeCodeId(11);
        let allocation = ExecutableAllocationId(3);
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

    fn baseline_native_entry_descriptor(owner: CodeBlockId) -> BaselineNativeEntryDescriptor {
        baseline_artifact(owner)
            .validate_baseline_entry_artifact(owner)
            .unwrap()
            .validate_native_entry_descriptor()
            .unwrap()
    }

    fn baseline_generated_code_body() -> BaselineGeneratedCodeBody {
        BaselineGeneratedCodeBody::new(
            BaselineGeneratedCodeBodyId(13),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
        )
    }

    fn baseline_eligibility_proof(owner: CodeBlockId) -> BaselineBytecodeEligibilityProof {
        baseline_eligibility_proof_with_instructions(
            owner,
            BaselineBytecodeRange {
                start: BytecodeIndex::from_offset(0),
                end: BytecodeIndex::from_offset(1),
                instruction_count: 2,
            },
            vec![
                BaselineBytecodeInstruction {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::LoadInt32,
                },
                BaselineBytecodeInstruction {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Return,
                },
            ],
        )
    }

    fn different_baseline_eligibility_proof(
        owner: CodeBlockId,
    ) -> BaselineBytecodeEligibilityProof {
        baseline_eligibility_proof_with_instructions(
            owner,
            BaselineBytecodeRange {
                start: BytecodeIndex::from_offset(0),
                end: BytecodeIndex::from_offset(2),
                instruction_count: 3,
            },
            vec![
                BaselineBytecodeInstruction {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::LoadInt32,
                },
                BaselineBytecodeInstruction {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::LoadInt32,
                },
                BaselineBytecodeInstruction {
                    bytecode_index: BytecodeIndex::from_offset(2),
                    opcode: CoreOpcode::Return,
                },
            ],
        )
    }

    fn baseline_eligibility_proof_with_instructions(
        owner: CodeBlockId,
        bytecode: BaselineBytecodeRange,
        instructions: Vec<BaselineBytecodeInstruction>,
    ) -> BaselineBytecodeEligibilityProof {
        BaselineBytecodeEligibilityRecord {
            owner: Some(owner),
            snapshot: TieringSnapshot {
                owner,
                from_tier: JitType::None,
                to_tier: JitType::Baseline,
                trigger: TieringTrigger::EntryCounter,
                counters: Default::default(),
                osr_entry_bytecode_index: None,
                epoch: 1,
            },
            bytecode,
            opcode_subset: BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            instructions,
            root_map_requirements: BaselineRootMapRequirements::default(),
            exception_metadata: BaselineExceptionMetadataPresence::Present { handler_count: 0 },
        }
        .validate()
        .unwrap()
    }

    fn runtime_helper_metadata_for_proof(
        proof: BaselineBytecodeEligibilityProof,
    ) -> BaselineGeneratedRuntimeHelperPlanMetadata {
        BaselineGeneratedRuntimeHelperPlanMetadata::new(
            proof.bytecode_snapshot_fingerprint(),
            vec![BaselineGeneratedRuntimeHelperProof::new(
                BytecodeIndex::from_offset(0),
                runtime_helper_boundary_proof(proof.owner(), BytecodeIndex::from_offset(0)),
            )],
        )
        .unwrap()
    }

    fn owner_continuation_map_for_proof(
        proof: BaselineBytecodeEligibilityProof,
    ) -> BaselineGeneratedOwnerContinuationMapMetadata {
        let owner = proof.owner();
        BaselineGeneratedOwnerContinuationMapMetadata::new(
            proof.bytecode_snapshot_fingerprint(),
            vec![
                BaselineGeneratedOwnerBytecodeLabel {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::LoadInt32,
                    next_bytecode_index: Some(BytecodeIndex::from_offset(1)),
                },
                BaselineGeneratedOwnerBytecodeLabel {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Return,
                    next_bytecode_index: None,
                },
            ],
            Vec::new(),
        )
        .unwrap()
    }

    fn runtime_helper_boundary_proof(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
    ) -> BaselineGeneratedRuntimeBoundaryProof {
        let root_map = BytecodeRootMapId(42);
        BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::NewObject,
            safepoint: CompilerSafepointDescriptor {
                id: CompilerSafepointId(7),
                owner: Some(owner),
                code: None,
                tier: JitType::Baseline,
                kind: CompilerSafepointKind::Call,
                bytecode_index: Some(bytecode_index),
                root_map: Some(root_map),
                roots: Vec::new(),
                may_call: true,
                may_allocate: true,
            },
            root_map: Some(BytecodeRootMap {
                id: root_map,
                owner: Some(owner),
                bytecode_range_start: bytecode_index,
                bytecode_range_end: bytecode_index,
                slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                    bytecode_index,
                    VirtualRegister::local(0),
                    BytecodeRootSlotKind::VirtualRegister,
                )],
                complete: true,
            }),
            no_gc_exit_reentry: true,
        }
        .validate()
        .unwrap()
    }

    #[test]
    fn live_baseline_artifact_validates_as_entry_eligible_data() {
        let artifact = baseline_artifact(owner());
        let entry = artifact.validate_baseline_entry_artifact(owner()).unwrap();

        assert_eq!(entry.id, artifact.id);
        assert_eq!(entry.tier, JitType::Baseline);
        assert_eq!(entry.owner, owner());
        assert_eq!(entry.liveness, CodeLiveness::Live);
        assert!(entry.has_valid_baseline_abi_proof());
        assert_eq!(
            entry.baseline_abi_proof.descriptor(),
            BASELINE_ABI_DESCRIPTOR
        );
        assert_eq!(
            entry.machine_code.owner,
            MachineCodeOwnership::CodeBlock(owner())
        );
    }

    #[test]
    fn valid_baseline_entry_artifact_describes_native_entry_without_callable_pointer() {
        let artifact = baseline_artifact(owner());
        let entry = artifact.validate_baseline_entry_artifact(owner()).unwrap();
        let descriptor = entry.validate_native_entry_descriptor().unwrap();

        assert_eq!(descriptor.owner, owner());
        assert_eq!(descriptor.artifact_id, artifact.id);
        assert_eq!(descriptor.native_symbol, artifact.native_code.unwrap());
        assert_eq!(descriptor.machine_code, artifact.machine_code.unwrap());
        assert_eq!(
            descriptor.machine_range,
            artifact.machine_code.unwrap().range
        );
        assert_eq!(descriptor.entrypoint, artifact.entrypoint);
        assert_eq!(
            descriptor.normal_entry,
            BaselineNativeEntryToken {
                owner: owner(),
                artifact_id: artifact.id,
                native_symbol: artifact.native_code.unwrap(),
                machine_code: artifact.machine_code.unwrap(),
                entrypoint: artifact.entrypoint,
                kind: BaselineNativeEntryTokenKind::Normal,
            }
        );
        assert_eq!(
            descriptor.arity_check_entry,
            BaselineArityCheckNativeEntry::Unavailable(
                BaselineArityCheckUnavailableReason::NotEmitted
            )
        );
        assert_eq!(
            descriptor.baseline_abi_proof.descriptor(),
            BASELINE_ABI_DESCRIPTOR
        );
    }

    #[test]
    fn descriptor_only_artifact_is_not_baseline_entry_eligible() {
        let mut artifact = baseline_artifact(owner());
        artifact.native_code = None;
        artifact.machine_code = None;
        artifact.liveness = CodeLiveness::Unallocated;

        assert_eq!(
            artifact.validate_baseline_entry_artifact(owner()),
            Err(JitCodeValidationError::BaselineEntryNotLive)
        );
    }

    #[test]
    fn baseline_native_entry_descriptor_rejects_symbol_entrypoint_abi_and_machine_mismatches() {
        let artifact = baseline_artifact(owner());
        let entry = artifact.validate_baseline_entry_artifact(owner()).unwrap();

        let mut symbol_mismatch = entry;
        symbol_mismatch.machine_code.symbol = Some(NativeCodeId(999));
        assert_eq!(
            symbol_mismatch.validate_native_entry_descriptor(),
            Err(JitCodeValidationError::BaselineNativeEntryNativeSymbolMismatch)
        );

        let mut entrypoint_mismatch = entry;
        entrypoint_mismatch.entrypoint.code = None;
        assert_eq!(
            entrypoint_mismatch.validate_native_entry_descriptor(),
            Err(JitCodeValidationError::BaselineNativeEntryEntrypointMismatch)
        );

        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.name = "stale-baseline-first-tier";
        let mut abi_mismatch = entry;
        abi_mismatch.baseline_abi_proof = BaselineAbiProof::new(&descriptor).unwrap();
        assert_eq!(
            abi_mismatch.validate_native_entry_descriptor(),
            Err(JitCodeValidationError::BaselineNativeEntryAbiDescriptorInvalid)
        );

        let mut malformed_machine = entry;
        malformed_machine.machine_code.range.size_bytes = 0;
        assert_eq!(
            malformed_machine.validate_native_entry_descriptor(),
            Err(JitCodeValidationError::BaselineNativeEntryMachineCodeInvalid)
        );
    }

    #[test]
    fn baseline_native_entry_callable_kinds_report_distinct_opcode_subsets() {
        let broad_subset = BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary;

        assert_eq!(
            BaselineNativeEntryCallableKind::P6PureBaselineNativeEntryShim
                .supported_opcode_subset(),
            broad_subset
        );
        assert_eq!(
            BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry
                .supported_opcode_subset(),
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse
        );
        assert_eq!(
            BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry
                .supported_opcode_subset(),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
        );
    }

    #[test]
    fn p6_emitted_semantic_c_abi_callable_constructors_seal_descriptor() {
        let descriptor = baseline_native_entry_descriptor(owner());

        let shim = BaselineNativeEntryCallableAuthority::new_p6_pure_baseline_native_entry_shim(
            descriptor,
        );
        for (emitted, expected_kind) in [
            (
                BaselineNativeEntryCallableAuthority::new_p6_x86_64_emitted_semantic_c_abi_entry(
                    descriptor,
                ),
                BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
            ),
            (
                BaselineNativeEntryCallableAuthority::new_p6_arm64_emitted_semantic_c_abi_entry(
                    descriptor,
                ),
                BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
            ),
        ] {
            assert_eq!(emitted.kind(), expected_kind);
            assert_eq!(emitted.descriptor(), descriptor);
            assert_eq!(emitted.token(), descriptor.normal_entry);
            assert_eq!(emitted.seal, shim.seal);
            assert_eq!(emitted.seal.owner, descriptor.owner);
            assert_eq!(emitted.seal.artifact_id, descriptor.artifact_id);
            assert_eq!(emitted.seal.native_symbol, descriptor.native_symbol);
            assert_eq!(emitted.seal.machine_code, descriptor.machine_code);
            assert_eq!(emitted.seal.entrypoint, descriptor.entrypoint);
            assert_eq!(emitted.validate_for_descriptor(&descriptor), Ok(()));
        }
    }

    #[test]
    fn p6_x86_64_emitted_semantic_c_abi_callable_rejects_forged_descriptor_and_token() {
        let descriptor = baseline_native_entry_descriptor(owner());
        let authority =
            BaselineNativeEntryCallableAuthority::new_p6_x86_64_emitted_semantic_c_abi_entry(
                descriptor,
            );
        let mut forged_descriptor = descriptor;
        forged_descriptor.machine_range.size_bytes += 1;

        assert_eq!(
            authority.validate_for_descriptor(&forged_descriptor),
            Err(
                BaselineNativeEntryCallableValidationError::MachineRangeMismatch {
                    expected: forged_descriptor.machine_range,
                    actual: descriptor.machine_range,
                }
            )
        );

        let mut forged_token = descriptor.normal_entry;
        forged_token.artifact_id = JitCodeId(999);
        let authority_with_forged_token =
            BaselineNativeEntryCallableAuthority::for_test_with_descriptor_and_token(
                BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
                descriptor,
                forged_token,
            );

        assert_eq!(
            authority_with_forged_token.validate_for_descriptor(&descriptor),
            Err(BaselineNativeEntryCallableValidationError::TokenMismatch {
                expected: Box::new(descriptor.normal_entry),
                actual: Box::new(forged_token),
            })
        );
    }

    #[test]
    fn baseline_entry_validation_rejects_invalid_abi_metadata() {
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.entry_abi = EntryAbi::Rust;
        let artifact = baseline_artifact(owner());

        assert_eq!(
            artifact.validate_baseline_entry_artifact_with_abi_descriptor(owner(), &descriptor),
            Err(JitCodeValidationError::BaselineEntryAbiDescriptorInvalid)
        );
    }

    #[test]
    fn baseline_entry_validation_rejects_owner_liveness_and_tier_mismatches() {
        let owner = owner();
        let other_owner = CodeBlockId(CellId(2));
        assert_eq!(
            baseline_artifact(other_owner).validate_baseline_entry_artifact(owner),
            Err(JitCodeValidationError::BaselineEntryOwnerMismatch)
        );

        let mut not_live = baseline_artifact(owner);
        not_live.liveness = CodeLiveness::PendingInvalidation;
        assert_eq!(
            not_live.validate_baseline_entry_artifact(owner),
            Err(JitCodeValidationError::BaselineEntryNotLive)
        );

        let mut wrong_tier = baseline_artifact(owner);
        wrong_tier.tier = JitType::Dfg;
        wrong_tier.origin.kind = CodeOriginKind::DfgReplacement;
        assert_eq!(
            wrong_tier.validate_baseline_entry_artifact(owner),
            Err(JitCodeValidationError::BaselineEntryTierMismatch)
        );
    }

    #[test]
    fn baseline_generated_code_artifact_validates_without_native_code() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let body = baseline_generated_code_body();

        let artifact = BaselineGeneratedCodeArtifact::new(
            JitCodeId(21),
            owner,
            proof,
            body,
            CodeLiveness::Live,
            CodeFinalizationAuthority::CompilerThread,
        )
        .unwrap();

        assert_eq!(artifact.id, JitCodeId(21));
        assert_eq!(artifact.owner, owner);
        assert_eq!(artifact.eligibility_proof.owner(), owner);
        assert_eq!(
            artifact.body.supported_opcode_subset,
            Some(BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic)
        );
        assert!(artifact.runtime_helper_plan().is_none());
        assert_eq!(artifact.liveness, CodeLiveness::Live);
        assert!(artifact.has_valid_baseline_abi_proof());
    }

    #[test]
    fn baseline_generated_code_artifact_owns_matching_runtime_helper_metadata() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let runtime_helper_plan = runtime_helper_metadata_for_proof(proof);

        let artifact = BaselineGeneratedCodeArtifact::new_with_runtime_helper_plan(
            JitCodeId(27),
            owner,
            proof,
            baseline_generated_code_body(),
            runtime_helper_plan,
            CodeLiveness::Live,
            CodeFinalizationAuthority::CompilerThread,
        )
        .unwrap();

        let borrowed = artifact.runtime_helper_plan().unwrap();
        assert_eq!(
            borrowed.bytecode_snapshot,
            proof.bytecode_snapshot_fingerprint()
        );
        assert_eq!(borrowed.proof_count(), 1);
        assert_eq!(
            borrowed.proof_at(0).unwrap().bytecode_index,
            BytecodeIndex::from_offset(0)
        );
    }

    #[test]
    fn baseline_generated_code_artifact_owns_matching_owner_continuation_map() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let owner_continuation_map = owner_continuation_map_for_proof(proof);

        let artifact =
            BaselineGeneratedCodeArtifact::new_with_runtime_helper_and_property_handoff_plans(
                JitCodeId(30),
                owner,
                proof,
                baseline_generated_code_body(),
                BaselineGeneratedCodeMetadataPlans::new(None, None, Some(owner_continuation_map)),
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
            )
            .unwrap();

        let owner_map = artifact
            .owner_continuation_map()
            .expect("owner continuation map");
        assert_eq!(
            owner_map.bytecode_snapshot(),
            proof.bytecode_snapshot_fingerprint()
        );
        assert_eq!(owner_map.label_count(), 2);
        assert_eq!(owner_map.call_site_count(), 0);
    }

    #[test]
    fn baseline_generated_code_artifact_rejects_owner_continuation_map_snapshot_mismatch() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let stale_map =
            owner_continuation_map_for_proof(different_baseline_eligibility_proof(owner));

        assert_eq!(
            BaselineGeneratedCodeArtifact::new_with_runtime_helper_and_property_handoff_plans(
                JitCodeId(31),
                owner,
                proof,
                baseline_generated_code_body(),
                BaselineGeneratedCodeMetadataPlans::new(None, None, Some(stale_map)),
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
            ),
            Err(JitCodeValidationError::BaselineGeneratedOwnerContinuationMapSnapshotMismatch)
        );
    }

    #[test]
    fn baseline_runtime_helper_metadata_rejects_missing_no_gc_exit_reentry() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let bytecode_index = BytecodeIndex::from_offset(0);
        let mut helper_proof = runtime_helper_boundary_proof(owner, bytecode_index);
        helper_proof.no_gc_exit_reentry = false;

        assert_eq!(
            BaselineGeneratedRuntimeHelperPlanMetadata::new(
                proof.bytecode_snapshot_fingerprint(),
                vec![BaselineGeneratedRuntimeHelperProof::new(
                    bytecode_index,
                    helper_proof,
                )],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMissingNoGcExitReentry {
                    bytecode_index,
                    opcode: CoreOpcode::NewObject,
                }
            )
        );
    }

    #[test]
    fn baseline_runtime_helper_metadata_rejects_missing_root_map() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let bytecode_index = BytecodeIndex::from_offset(0);
        let mut helper_proof = runtime_helper_boundary_proof(owner, bytecode_index);
        helper_proof.root_map = None;

        assert_eq!(
            BaselineGeneratedRuntimeHelperPlanMetadata::new(
                proof.bytecode_snapshot_fingerprint(),
                vec![BaselineGeneratedRuntimeHelperProof::new(
                    bytecode_index,
                    helper_proof,
                )],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMissingRootMap {
                    bytecode_index,
                    opcode: CoreOpcode::NewObject,
                    safepoint: CompilerSafepointId(7),
                }
            )
        );
    }

    #[test]
    fn baseline_generated_code_rejects_stale_runtime_helper_metadata_snapshot() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let stale_plan =
            runtime_helper_metadata_for_proof(different_baseline_eligibility_proof(owner));

        assert_eq!(
            BaselineGeneratedCodeArtifact::new_with_runtime_helper_plan(
                JitCodeId(28),
                owner,
                proof,
                baseline_generated_code_body(),
                stale_plan,
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
            ),
            Err(JitCodeValidationError::BaselineGeneratedRuntimeHelperPlanSnapshotMismatch)
        );

        let runtime_helper_plan = runtime_helper_metadata_for_proof(proof);
        let mut artifact = BaselineGeneratedCodeArtifact::new_with_runtime_helper_plan(
            JitCodeId(29),
            owner,
            proof,
            baseline_generated_code_body(),
            runtime_helper_plan,
            CodeLiveness::Live,
            CodeFinalizationAuthority::CompilerThread,
        )
        .unwrap();
        artifact.runtime_helper_plan = Some(runtime_helper_metadata_for_proof(
            different_baseline_eligibility_proof(owner),
        ));

        assert_eq!(
            artifact.validate(),
            Err(JitCodeValidationError::BaselineGeneratedRuntimeHelperPlanSnapshotMismatch)
        );
    }

    #[test]
    fn baseline_generated_code_rejects_owner_mismatch() {
        let proof = baseline_eligibility_proof(owner());
        let other_owner = CodeBlockId(CellId(2));

        assert_eq!(
            BaselineGeneratedCodeArtifact::new(
                JitCodeId(22),
                other_owner,
                proof,
                baseline_generated_code_body(),
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
            ),
            Err(JitCodeValidationError::BaselineGeneratedCodeOwnerMismatch)
        );
    }

    #[test]
    fn baseline_generated_code_rejects_subset_body_mismatch() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let body = BaselineGeneratedCodeBody {
            id: BaselineGeneratedCodeBodyId(23),
            supported_opcode_subset: None,
            effect_contract: Some(
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
                    .generated_effect_contract(),
            ),
        };

        assert_eq!(
            BaselineGeneratedCodeArtifact::new(
                JitCodeId(23),
                owner,
                proof,
                body,
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
            ),
            Err(JitCodeValidationError::BaselineGeneratedCodeOpcodeSubsetMismatch)
        );
    }

    #[test]
    fn baseline_generated_code_rejects_effect_contract_mismatch() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let mut body = baseline_generated_code_body();
        body.effect_contract = None;

        assert_eq!(
            BaselineGeneratedCodeArtifact::new(
                JitCodeId(26),
                owner,
                proof,
                body,
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
            ),
            Err(JitCodeValidationError::BaselineGeneratedCodeEffectContractMismatch)
        );
    }

    #[test]
    fn baseline_generated_code_rejects_non_live_liveness() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);

        assert_eq!(
            BaselineGeneratedCodeArtifact::new(
                JitCodeId(24),
                owner,
                proof,
                baseline_generated_code_body(),
                CodeLiveness::PendingInvalidation,
                CodeFinalizationAuthority::CompilerThread,
            ),
            Err(JitCodeValidationError::BaselineGeneratedCodeNotLive)
        );
    }

    #[test]
    fn baseline_generated_code_validates_current_abi_proof() {
        let owner = owner();
        let proof = baseline_eligibility_proof(owner);
        let mut descriptor = BASELINE_ABI_DESCRIPTOR;
        descriptor.entry_abi = EntryAbi::Rust;

        assert_eq!(
            BaselineGeneratedCodeArtifact::new_with_abi_descriptor(
                JitCodeId(25),
                owner,
                proof,
                baseline_generated_code_body(),
                CodeLiveness::Live,
                CodeFinalizationAuthority::CompilerThread,
                &descriptor,
            ),
            Err(JitCodeValidationError::BaselineGeneratedCodeAbiDescriptorInvalid)
        );
    }
}
