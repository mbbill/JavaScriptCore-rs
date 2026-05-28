//! Future concurrent compilation plan contracts.
//!
//! Plans may eventually hold weak or externally traced references to code
//! blocks, runtime objects, profiles, and watchpoints. This skeleton keeps those
//! references opaque so neighboring modules can define ownership first.

use crate::bytecode::{
    BytecodeIndex, BytecodeRootMap, BytecodeRootMapId, BytecodeRootMapValidationError,
    BytecodeRootSlotDescriptor, BytecodeRootSlotKind, BytecodeRootSlotStorage, Checkpoint,
    CodeBlock, CoreOpcode, DecodedInstruction, DecodedInstructionSource,
    InlineCacheMutationAuthority, InlineCacheState as BytecodeInlineCacheState,
    InstructionDecodeError, Opcode, Operand, OperandAccessError, OperandWidth, PropertyAccessType,
    PropertyCacheKey, PropertyCacheKind, PropertyInlineCache, PropertyInlineCacheDispatch,
    RuntimeSlot, ValueProfileBucketKind, ValueProfileEmissionPolicy,
    ValueProfileJitStorageGeneration, VirtualRegister,
};
use crate::gc::{
    CellId, HeapId, RootId, RootKind, RootRecord, RootSetMutationAuthority, RootSetSemanticError,
    TargetedRootRecord, TargetedRootSet,
};
use crate::jit::semantics::EffectSummary;
use crate::jit::{
    CodeInstallBarrier, CodeInvalidationReason, InlineCacheBarrierMetadata,
    InlineCacheBarrierTarget, InlineCacheFallbackSemantics, InlineCacheKind,
    InlineCacheMissHandoffDescriptor, InlineCacheMissKind, InlineCacheSlot, InlineCacheSlotId,
    InlineCacheState as JitInlineCacheState, InlineCacheValidationError, JitCodeArtifact,
    JitCodeId, JitType, TieringSnapshot, TieringTrigger, WatchpointDependency,
};
use crate::runtime::{CodeBlockId, ExecutableId};
use crate::strings::{AtomId, Identifier, PropertyKey};

/// Stable identity for a deferred compilation plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilationPlanId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JitCompilationKeyId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JitWorklistId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerSafepointId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerSafepointKind {
    Call,
    SlowPath,
    OsrExit,
    ExceptionCheck,
    LoopBackedge,
    WasmBoundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerRootSlotLocation {
    VirtualRegister(VirtualRegister),
    StackSlot(i32),
    MachineRegister(u16),
    ConstantPool(u32),
    MetadataSlot(u32),
    InlineCacheSlot(u32),
    ValueProfileSlot(u32),
    CallSite(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerRootSlotDescriptor {
    pub bytecode_index: Option<BytecodeIndex>,
    pub location: CompilerRootSlotLocation,
    pub slot_kind: BytecodeRootSlotKind,
    pub root_kind: RootKind,
    pub mutation_authority: RootSetMutationAuthority,
    pub precise: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerSafepointRootTarget {
    Known(CellId),
    NoTarget,
    Unknown,
}

impl CompilerSafepointRootTarget {
    pub const fn known(target: CellId) -> Self {
        Self::Known(target)
    }

    pub const fn no_target() -> Self {
        Self::NoTarget
    }

    pub const fn unknown() -> Self {
        Self::Unknown
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerSafepointRootBinding {
    pub slot_index: usize,
    pub target: CompilerSafepointRootTarget,
}

impl CompilerSafepointRootBinding {
    pub const fn targeted(slot_index: usize, target: CellId) -> Self {
        Self {
            slot_index,
            target: CompilerSafepointRootTarget::known(target),
        }
    }

    pub const fn no_target(slot_index: usize) -> Self {
        Self {
            slot_index,
            target: CompilerSafepointRootTarget::no_target(),
        }
    }

    pub const fn unknown(slot_index: usize) -> Self {
        Self {
            slot_index,
            target: CompilerSafepointRootTarget::unknown(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSafepointDescriptor {
    pub id: CompilerSafepointId,
    pub owner: Option<CodeBlockId>,
    pub code: Option<JitCodeId>,
    pub tier: JitType,
    pub kind: CompilerSafepointKind,
    pub bytecode_index: Option<BytecodeIndex>,
    pub root_map: Option<BytecodeRootMapId>,
    pub roots: Vec<CompilerRootSlotDescriptor>,
    pub may_call: bool,
    pub may_allocate: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSafepointTargetedRootPlan {
    pub safepoint: CompilerSafepointId,
    pub heap: HeapId,
    pub roots: Vec<TargetedRootRecord>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineSupportedOpcodeSubset {
    P6ConstantsMovesReturnInt32Arithmetic,
    P8aConstantsMovesReturnInt32ArithmeticBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitAndOr,
    P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality,
    P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational,
    P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality,
    P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational,
    P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber,
    P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish,
    P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse,
    P6ConstantsMovesReturnInt32ArithmeticBitwise,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid,
    P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineOpcodeRejectionReason {
    Call,
    AllocationOrObject,
    PropertyAccess,
    Exception,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineGeneratedEffectRejectionReason {
    MayAllocate,
    MayCallRuntime,
    MayCallJs,
    MayThrow,
    WritesHeap,
    TouchesGcRoots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineOpcodeEffect {
    pub opcode: CoreOpcode,
    pub summary: EffectSummary,
    pub may_call_runtime: bool,
    pub touches_gc_roots: bool,
    pub records_write_barrier_handoff: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedEffectContract {
    pub opcode_subset: BaselineSupportedOpcodeSubset,
    pub summary: EffectSummary,
    pub may_call_runtime: bool,
    pub touches_gc_roots: bool,
    pub records_write_barrier_handoff: bool,
}

impl BaselineGeneratedEffectContract {
    pub const fn permits_no_heap_allocation_no_runtime_call(self) -> bool {
        !self.summary.allocates && !self.may_call_runtime && !self.summary.may_call_js
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineGeneratedRuntimeBoundaryCategory {
    NoJsCallHeapRuntimeHelper,
    ExceptionThrow,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineGeneratedRuntimeBoundaryRejectionReason {
    PropertyAccess,
    JsCallOrConstructor,
    FunctionOrConstructor,
    Exception,
    PowNumber,
    Unsupported,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedRuntimeBoundaryEffects {
    pub calls_runtime_helper: bool,
    pub allocates: bool,
    pub may_throw: bool,
    pub writes_heap: bool,
    pub touches_gc_roots: bool,
    pub records_write_barrier_handoff: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedRuntimeBoundaryRequirements {
    pub complete_safepoint_root_map: bool,
    pub no_gc_exit_reentry: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedRuntimeBoundaryContract {
    pub opcode: CoreOpcode,
    pub category: BaselineGeneratedRuntimeBoundaryCategory,
    pub summary: EffectSummary,
    pub effects: BaselineGeneratedRuntimeBoundaryEffects,
    pub requirements: BaselineGeneratedRuntimeBoundaryRequirements,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedRuntimeBoundaryCandidate {
    pub opcode: CoreOpcode,
    pub safepoint: CompilerSafepointDescriptor,
    pub root_map: Option<BytecodeRootMap>,
    pub no_gc_exit_reentry: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedRuntimeBoundaryProof {
    pub contract: BaselineGeneratedRuntimeBoundaryContract,
    pub safepoint: CompilerSafepointId,
    pub root_map: Option<BytecodeRootMapId>,
    pub root_count: usize,
    pub no_gc_exit_reentry: bool,
    pub may_throw: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaselineGeneratedRuntimeBoundaryValidationError {
    RejectedOpcode {
        opcode: CoreOpcode,
        reason: BaselineGeneratedRuntimeBoundaryRejectionReason,
    },
    MissingNoGcExitReentry {
        opcode: CoreOpcode,
    },
    SafepointMissingAllocationFlag {
        opcode: CoreOpcode,
        safepoint: CompilerSafepointId,
    },
    Safepoint(JitPlanValidationError),
}

#[allow(dead_code)]
impl BaselineGeneratedRuntimeBoundaryCandidate {
    pub fn validate(
        &self,
    ) -> Result<
        BaselineGeneratedRuntimeBoundaryProof,
        BaselineGeneratedRuntimeBoundaryValidationError,
    > {
        let contract =
            baseline_generated_runtime_boundary_contract(self.opcode).map_err(|reason| {
                BaselineGeneratedRuntimeBoundaryValidationError::RejectedOpcode {
                    opcode: self.opcode,
                    reason,
                }
            })?;

        if contract.requirements.no_gc_exit_reentry && !self.no_gc_exit_reentry {
            return Err(
                BaselineGeneratedRuntimeBoundaryValidationError::MissingNoGcExitReentry {
                    opcode: self.opcode,
                },
            );
        }
        if contract.effects.calls_runtime_helper && !self.safepoint.may_call {
            return Err(BaselineGeneratedRuntimeBoundaryValidationError::Safepoint(
                JitPlanValidationError::SafepointKindCallMismatch(self.safepoint.id),
            ));
        }
        if contract.effects.allocates && !self.safepoint.may_allocate {
            return Err(
                BaselineGeneratedRuntimeBoundaryValidationError::SafepointMissingAllocationFlag {
                    opcode: self.opcode,
                    safepoint: self.safepoint.id,
                },
            );
        }

        let (root_map, root_count) = if contract.requirements.complete_safepoint_root_map {
            let root_map = self.root_map.as_ref().ok_or(
                BaselineGeneratedRuntimeBoundaryValidationError::Safepoint(
                    JitPlanValidationError::SafepointMissingRootMap(self.safepoint.id),
                ),
            )?;
            if !root_map.complete {
                return Err(BaselineGeneratedRuntimeBoundaryValidationError::Safepoint(
                    JitPlanValidationError::SafepointIncompleteRootMap {
                        safepoint: self.safepoint.id,
                        root_map: root_map.id,
                    },
                ));
            }
            let resolved = self
                .safepoint
                .resolve_root_map(root_map)
                .map_err(BaselineGeneratedRuntimeBoundaryValidationError::Safepoint)?;
            (Some(root_map.id), resolved.roots.len())
        } else {
            self.safepoint
                .validate()
                .map_err(BaselineGeneratedRuntimeBoundaryValidationError::Safepoint)?;
            (self.safepoint.root_map, self.safepoint.roots.len())
        };

        Ok(BaselineGeneratedRuntimeBoundaryProof {
            contract,
            safepoint: self.safepoint.id,
            root_map,
            root_count,
            no_gc_exit_reentry: self.no_gc_exit_reentry,
            may_throw: contract.effects.may_throw,
        })
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineBytecodeRange {
    pub start: BytecodeIndex,
    pub end: BytecodeIndex,
    pub instruction_count: u32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineBytecodeInstruction {
    pub bytecode_index: BytecodeIndex,
    pub opcode: CoreOpcode,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineMixedBytecodeSiteKind {
    Generated,
    RuntimeHelper,
    JsCallHandoff,
    PropertyHandoff,
    InterpreterFallback,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineMixedBytecodeSite {
    pub bytecode_index: BytecodeIndex,
    pub opcode: CoreOpcode,
    pub kind: BaselineMixedBytecodeSiteKind,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineBytecodeSnapshotFingerprint {
    instruction_count: u32,
    instruction_stream_hash: u128,
    side_table_hash: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineBytecodeProofBinding {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode: BaselineBytecodeRange,
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BaselineBytecodeProofBindingError {
    OwnerWitnessMismatch {
        owner_witness: CodeBlockId,
        proof_owner: CodeBlockId,
    },
    ProofSnapshotOwnerMismatch {
        proof_owner: CodeBlockId,
        snapshot_owner: CodeBlockId,
    },
    ProofSnapshotTierMismatch {
        from_tier: JitType,
        to_tier: JitType,
    },
    CodeBlockSnapshotInvalid {
        error: JitPlanValidationError,
    },
    CodeBlockSnapshotMismatch {
        owner: CodeBlockId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedRuntimeHelperProof {
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) proof: BaselineGeneratedRuntimeBoundaryProof,
}

impl BaselineGeneratedRuntimeHelperProof {
    #[allow(dead_code)]
    pub(crate) const fn new(
        bytecode_index: BytecodeIndex,
        proof: BaselineGeneratedRuntimeBoundaryProof,
    ) -> Self {
        Self {
            bytecode_index,
            proof,
        }
    }
}

// P9 encodes the retained JS-call native-exit site index in a u32 payload.
// This must not be tied to the separate inline argument-register array size.
pub(crate) const BASELINE_GENERATED_JS_CALL_NATIVE_EXIT_PLAN_SITE_CAPACITY: usize =
    u32::MAX as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedRuntimeHelperPlan<'proof> {
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    proofs: &'proof [BaselineGeneratedRuntimeHelperProof],
}

impl<'proof> BaselineGeneratedRuntimeHelperPlan<'proof> {
    #[allow(dead_code)]
    pub(crate) const fn new(
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        proofs: &'proof [BaselineGeneratedRuntimeHelperProof],
    ) -> Self {
        Self {
            bytecode_snapshot,
            proofs,
        }
    }

    #[cfg(test)]
    pub(crate) fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    #[cfg(test)]
    pub(crate) fn proof_at(&self, index: usize) -> Option<&BaselineGeneratedRuntimeHelperProof> {
        self.proofs.get(index)
    }

    pub(crate) fn proof_for_bytecode_index(
        self,
        bytecode_index: BytecodeIndex,
    ) -> Result<Option<&'proof BaselineGeneratedRuntimeBoundaryProof>, ()> {
        let mut matching = None;
        let mut ambiguous = false;
        for proof in self.proofs {
            if proof.bytecode_index == bytecode_index {
                if matching.is_some() {
                    ambiguous = true;
                    break;
                }
                matching = Some(&proof.proof);
            }
        }
        if ambiguous {
            Err(())
        } else {
            Ok(matching)
        }
    }
}

/// Owned generated-code metadata for runtime-helper handoff proofs.
///
/// The table is sorted by bytecode index and rejects duplicate indices at
/// construction, so a generated artifact can own proof data while executor
/// entrypoints borrow a plan view without cloning proofs.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedRuntimeHelperPlanMetadata {
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    proofs: Vec<BaselineGeneratedRuntimeHelperProof>,
}

#[allow(dead_code)]
impl BaselineGeneratedRuntimeHelperPlanMetadata {
    pub(crate) fn new(
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        mut proofs: Vec<BaselineGeneratedRuntimeHelperProof>,
    ) -> Result<Self, JitPlanValidationError> {
        validate_baseline_generated_runtime_helper_proofs(&mut proofs)?;
        Ok(Self {
            bytecode_snapshot,
            proofs,
        })
    }

    pub(crate) fn from_code_block_snapshot(
        code_block: &CodeBlock,
        proofs: Vec<BaselineGeneratedRuntimeHelperProof>,
    ) -> Result<Self, JitPlanValidationError> {
        Self::new(
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?,
            proofs,
        )
    }

    pub(crate) const fn bytecode_snapshot(&self) -> BaselineBytecodeSnapshotFingerprint {
        self.bytecode_snapshot
    }

    pub(crate) fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    pub(crate) fn proof_at(&self, index: usize) -> Option<&BaselineGeneratedRuntimeHelperProof> {
        self.proofs.get(index)
    }

    pub(crate) fn borrowed_plan(&self) -> BaselineGeneratedRuntimeHelperPlan<'_> {
        BaselineGeneratedRuntimeHelperPlan::new(self.bytecode_snapshot, &self.proofs)
    }

    pub(crate) fn proof_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&BaselineGeneratedRuntimeBoundaryProof> {
        self.proofs
            .binary_search_by_key(&bytecode_index, |proof| proof.bytecode_index)
            .ok()
            .map(|index| &self.proofs[index].proof)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedRuntimeHelperPlanDerivation {
    pub(crate) metadata: Option<BaselineGeneratedRuntimeHelperPlanMetadata>,
    pub(crate) safepoints: Vec<CompilerSafepointDescriptor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsCallNativeExitSite {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) destination: VirtualRegister,
    pub(crate) callee: VirtualRegister,
    pub(crate) this_register: Option<VirtualRegister>,
    pub(crate) provided_argument_count: u32,
    pub(crate) argument_registers: Vec<VirtualRegister>,
    pub(crate) resume_bytecode_index: Option<BytecodeIndex>,
    pub(crate) requires_no_gc_exit_reentry: bool,
    pub(crate) may_throw: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsCallNativeExitPlanMetadata {
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    sites: Vec<BaselineGeneratedJsCallNativeExitSite>,
}

impl BaselineGeneratedJsCallNativeExitPlanMetadata {
    pub(crate) fn new(
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        mut sites: Vec<BaselineGeneratedJsCallNativeExitSite>,
    ) -> Result<Self, JitPlanValidationError> {
        validate_baseline_generated_js_call_native_exit_sites(&mut sites)?;
        Ok(Self {
            bytecode_snapshot,
            sites,
        })
    }

    pub(crate) fn from_code_block_snapshot(
        code_block: &CodeBlock,
        sites: Vec<BaselineGeneratedJsCallNativeExitSite>,
    ) -> Result<Self, JitPlanValidationError> {
        Self::new(
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?,
            sites,
        )
    }

    pub(crate) const fn bytecode_snapshot(&self) -> BaselineBytecodeSnapshotFingerprint {
        self.bytecode_snapshot
    }

    pub(crate) fn site_count(&self) -> usize {
        self.sites.len()
    }

    pub(crate) fn site_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&BaselineGeneratedJsCallNativeExitSite> {
        self.sites
            .binary_search_by_key(&bytecode_index, |site| site.bytecode_index)
            .ok()
            .and_then(|index| self.sites.get(index))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedJsCallNativeExitPlanDerivation {
    pub(crate) metadata: Option<BaselineGeneratedJsCallNativeExitPlanMetadata>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedOwnerBytecodeLabel {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) next_bytecode_index: Option<BytecodeIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BaselineGeneratedOwnerContinuationKind {
    Call,
    Construct,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedOwnerCallResultProfileSite {
    pub(crate) profile_slot: RuntimeSlot,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) checkpoint: Checkpoint,
    pub(crate) bucket_kind: ValueProfileBucketKind,
    pub(crate) storage_generation: ValueProfileJitStorageGeneration,
    pub(crate) value_profile_offset: u32,
    pub(crate) metadata_table_displacement: i32,
    pub(crate) metadata_table_base_address: usize,
    pub(crate) raw_bucket_address: usize,
    pub(crate) raw_bucket_bytes: u32,
    pub(crate) emission_policy: ValueProfileEmissionPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedOwnerContinuationSite {
    pub(crate) owner: CodeBlockId,
    pub(crate) call_bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) destination: VirtualRegister,
    pub(crate) argument_count_including_this: u32,
    pub(crate) resume_bytecode_index: Option<BytecodeIndex>,
    pub(crate) kind: BaselineGeneratedOwnerContinuationKind,
    pub(crate) result_profile: Option<BaselineGeneratedOwnerCallResultProfileSite>,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedOwnerContinuationMapMetadata {
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    labels: Vec<BaselineGeneratedOwnerBytecodeLabel>,
    call_sites: Vec<BaselineGeneratedOwnerContinuationSite>,
}

impl std::fmt::Debug for BaselineGeneratedOwnerContinuationMapMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BaselineGeneratedOwnerContinuationMapMetadata")
            .field("bytecode_snapshot", &self.bytecode_snapshot)
            .field("label_count", &self.labels.len())
            .field("call_site_count", &self.call_sites.len())
            .finish()
    }
}

#[allow(dead_code)]
impl BaselineGeneratedOwnerContinuationMapMetadata {
    pub(crate) fn new(
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        mut labels: Vec<BaselineGeneratedOwnerBytecodeLabel>,
        mut call_sites: Vec<BaselineGeneratedOwnerContinuationSite>,
    ) -> Result<Self, JitPlanValidationError> {
        validate_baseline_generated_owner_continuation_labels(&mut labels)?;
        validate_baseline_generated_owner_continuation_sites(&labels, &mut call_sites)?;
        Ok(Self {
            bytecode_snapshot,
            labels,
            call_sites,
        })
    }

    pub(crate) fn from_code_block_snapshot(
        code_block: &CodeBlock,
        labels: Vec<BaselineGeneratedOwnerBytecodeLabel>,
        call_sites: Vec<BaselineGeneratedOwnerContinuationSite>,
    ) -> Result<Self, JitPlanValidationError> {
        Self::new(
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?,
            labels,
            call_sites,
        )
    }

    pub(crate) const fn bytecode_snapshot(&self) -> BaselineBytecodeSnapshotFingerprint {
        self.bytecode_snapshot
    }

    pub(crate) fn label_count(&self) -> usize {
        self.labels.len()
    }

    pub(crate) fn call_site_count(&self) -> usize {
        self.call_sites.len()
    }

    pub(crate) fn label_at(&self, index: usize) -> Option<&BaselineGeneratedOwnerBytecodeLabel> {
        self.labels.get(index)
    }

    pub(crate) fn call_site_at(
        &self,
        index: usize,
    ) -> Option<&BaselineGeneratedOwnerContinuationSite> {
        self.call_sites.get(index)
    }

    pub(crate) fn label_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&BaselineGeneratedOwnerBytecodeLabel> {
        self.labels
            .binary_search_by_key(&bytecode_index, |label| label.bytecode_index)
            .ok()
            .and_then(|index| self.labels.get(index))
    }

    pub(crate) fn call_site_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&BaselineGeneratedOwnerContinuationSite> {
        self.call_sites
            .binary_search_by_key(&bytecode_index, |site| site.call_bytecode_index)
            .ok()
            .and_then(|index| self.call_sites.get(index))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedOwnerContinuationMapDerivation {
    pub(crate) metadata: Option<BaselineGeneratedOwnerContinuationMapMetadata>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineGeneratedPropertyHandoffSite {
    pub(crate) owner: CodeBlockId,
    pub(crate) slot: InlineCacheSlotId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) cache_kind: InlineCacheKind,
    pub(crate) access: PropertyAccessType,
    pub(crate) property_cache_kind: PropertyCacheKind,
    pub(crate) property_key: PropertyCacheKey,
    pub(crate) fallback: InlineCacheFallbackSemantics,
    pub(crate) cold_miss_handoff: InlineCacheMissHandoffDescriptor,
    pub(crate) requires_no_gc_exit_reentry: bool,
    pub(crate) may_throw: bool,
}

impl BaselineGeneratedPropertyHandoffSite {
    pub(crate) const fn get_by_name_property_load(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_key: PropertyKey,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::GetByName,
            cache_kind: InlineCacheKind::PropertyLoad,
            access: PropertyAccessType::GetById,
            property_cache_kind: PropertyCacheKind::GetById,
            property_key: PropertyCacheKey::Key(property_key),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::PropertyLoad,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn get_global_object_property_load(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_key: PropertyKey,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::GetGlobalObjectProperty,
            cache_kind: InlineCacheKind::PropertyLoad,
            access: PropertyAccessType::GetById,
            property_cache_kind: PropertyCacheKind::GetById,
            property_key: PropertyCacheKey::Key(property_key),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::PropertyLoad,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn get_length_property_load(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_key: PropertyKey,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::GetLength,
            cache_kind: InlineCacheKind::PropertyLoad,
            access: PropertyAccessType::GetById,
            property_cache_kind: PropertyCacheKind::GetById,
            property_key: PropertyCacheKey::Key(property_key),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::PropertyLoad,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn put_by_name_property_store(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_key: PropertyKey,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::PutByName,
            cache_kind: InlineCacheKind::PropertyStore,
            access: PropertyAccessType::PutByIdSloppy,
            property_cache_kind: PropertyCacheKind::PutById,
            property_key: PropertyCacheKey::Key(property_key),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::PropertyStore,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn put_global_object_property_store(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_key: PropertyKey,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::PutGlobalObjectProperty,
            cache_kind: InlineCacheKind::PropertyStore,
            access: PropertyAccessType::PutByIdSloppy,
            property_cache_kind: PropertyCacheKind::PutById,
            property_key: PropertyCacheKey::Key(property_key),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::PropertyStore,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn get_by_value_element_load(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_register: VirtualRegister,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::GetByValue,
            cache_kind: InlineCacheKind::ElementLoad,
            access: PropertyAccessType::GetByVal,
            property_cache_kind: PropertyCacheKind::GetByVal,
            property_key: PropertyCacheKey::RuntimeValue(property_register),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::ElementLoad,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn put_by_value_element_store(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_register: VirtualRegister,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::PutByValue,
            cache_kind: InlineCacheKind::ElementStore,
            access: PropertyAccessType::PutByValSloppy,
            property_cache_kind: PropertyCacheKind::PutByVal,
            property_key: PropertyCacheKey::RuntimeValue(property_register),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::ElementStore,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn in_by_id_has(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_key: PropertyKey,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::InById,
            cache_kind: InlineCacheKind::HasProperty,
            access: PropertyAccessType::InById,
            property_cache_kind: PropertyCacheKind::InById,
            property_key: PropertyCacheKey::Key(property_key),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::HasProperty,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    pub(crate) const fn in_by_value_has(
        owner: CodeBlockId,
        slot: InlineCacheSlotId,
        bytecode_index: BytecodeIndex,
        property_register: VirtualRegister,
    ) -> Self {
        Self {
            owner,
            slot,
            bytecode_index,
            opcode: CoreOpcode::InByVal,
            cache_kind: InlineCacheKind::HasProperty,
            access: PropertyAccessType::InByVal,
            property_cache_kind: PropertyCacheKind::InByVal,
            property_key: PropertyCacheKey::RuntimeValue(property_register),
            fallback: InlineCacheFallbackSemantics::SlowPathLookup,
            cold_miss_handoff: InlineCacheMissHandoffDescriptor {
                owner,
                slot,
                bytecode_index: bytecode_index.offset(),
                cache_kind: InlineCacheKind::HasProperty,
                miss_kind: InlineCacheMissKind::Cold,
                fallback: InlineCacheFallbackSemantics::SlowPathLookup,
                boundary: None,
                call_link: None,
                preserves_operand_registers: true,
            },
            requires_no_gc_exit_reentry: true,
            may_throw: true,
        }
    }

    fn with_cold_miss_handoff(
        mut self,
        cold_miss_handoff: InlineCacheMissHandoffDescriptor,
    ) -> Self {
        self.cold_miss_handoff = cold_miss_handoff;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyHandoffPlan<'site> {
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    sites: &'site [BaselineGeneratedPropertyHandoffSite],
}

impl<'site> BaselineGeneratedPropertyHandoffPlan<'site> {
    #[allow(dead_code)]
    pub(crate) const fn new(
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        sites: &'site [BaselineGeneratedPropertyHandoffSite],
    ) -> Self {
        Self {
            bytecode_snapshot,
            sites,
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn site_count(&self) -> usize {
        self.sites.len()
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn site_at(&self, index: usize) -> Option<&BaselineGeneratedPropertyHandoffSite> {
        self.sites.get(index)
    }

    pub(crate) fn site_for_bytecode_index(
        self,
        bytecode_index: BytecodeIndex,
    ) -> Result<Option<&'site BaselineGeneratedPropertyHandoffSite>, ()> {
        let mut matching = None;
        let mut ambiguous = false;
        for site in self.sites {
            if site.bytecode_index == bytecode_index {
                if matching.is_some() {
                    ambiguous = true;
                    break;
                }
                matching = Some(site);
            }
        }
        if ambiguous {
            Err(())
        } else {
            Ok(matching)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyHandoffPlanMetadata {
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    sites: Vec<BaselineGeneratedPropertyHandoffSite>,
}

impl BaselineGeneratedPropertyHandoffPlanMetadata {
    pub(crate) fn new(
        bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
        mut sites: Vec<BaselineGeneratedPropertyHandoffSite>,
    ) -> Result<Self, JitPlanValidationError> {
        validate_baseline_generated_property_handoff_sites(&mut sites)?;
        Ok(Self {
            bytecode_snapshot,
            sites,
        })
    }

    pub(crate) fn from_code_block_snapshot(
        code_block: &CodeBlock,
        sites: Vec<BaselineGeneratedPropertyHandoffSite>,
    ) -> Result<Self, JitPlanValidationError> {
        Self::new(
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?,
            sites,
        )
    }

    pub(crate) const fn bytecode_snapshot(&self) -> BaselineBytecodeSnapshotFingerprint {
        self.bytecode_snapshot
    }

    pub(crate) fn site_count(&self) -> usize {
        self.sites.len()
    }

    pub(crate) fn site_at(&self, index: usize) -> Option<&BaselineGeneratedPropertyHandoffSite> {
        self.sites.get(index)
    }

    pub(crate) fn borrowed_plan(&self) -> BaselineGeneratedPropertyHandoffPlan<'_> {
        BaselineGeneratedPropertyHandoffPlan::new(self.bytecode_snapshot, &self.sites)
    }

    pub(crate) fn site_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&BaselineGeneratedPropertyHandoffSite> {
        self.sites
            .binary_search_by_key(&bytecode_index, |site| site.bytecode_index)
            .ok()
            .and_then(|index| self.sites.get(index))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedPropertyHandoffPlanDerivation {
    pub(crate) metadata: Option<BaselineGeneratedPropertyHandoffPlanMetadata>,
}

pub(crate) fn derive_baseline_generated_runtime_helper_plan_from_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
) -> Result<BaselineGeneratedRuntimeHelperPlanDerivation, JitPlanValidationError> {
    let mut proofs = Vec::new();
    let mut safepoints = Vec::new();

    for (ordinal, decoded) in code_block
        .unlinked()
        .instructions()
        .decoded_instructions()
        .enumerate()
    {
        let decoded = decoded.map_err(|error| {
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            }
        })?;
        let Some(opcode) = CoreOpcode::from_opcode(decoded.opcode) else {
            continue;
        };
        let Some(helper) = BaselineGeneratedRuntimeHelperDescriptor::for_opcode(opcode) else {
            continue;
        };

        let safepoint_id = CompilerSafepointId((proofs.len() as u32).saturating_add(1));
        let (proof, safepoint) = derive_baseline_generated_runtime_helper_site(
            code_block,
            owner,
            decoded,
            helper,
            safepoint_id,
        )?;
        proofs.push(BaselineGeneratedRuntimeHelperProof::new(
            decoded.bytecode_index,
            proof,
        ));
        safepoints.push(safepoint);
    }

    let metadata = if proofs.is_empty() {
        None
    } else {
        Some(
            BaselineGeneratedRuntimeHelperPlanMetadata::from_code_block_snapshot(
                code_block, proofs,
            )?,
        )
    };

    Ok(BaselineGeneratedRuntimeHelperPlanDerivation {
        metadata,
        safepoints,
    })
}

pub(crate) fn validate_baseline_generated_runtime_helper_plan_against_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
) -> Result<BaselineGeneratedRuntimeHelperPlanDerivation, JitPlanValidationError> {
    let derivation =
        derive_baseline_generated_runtime_helper_plan_from_code_block(code_block, owner)?;
    if derivation.metadata.as_ref() != Some(runtime_helper_plan) {
        return Err(baseline_generated_runtime_helper_plan_mismatch_error(
            code_block,
            derivation.metadata.as_ref(),
            runtime_helper_plan,
        ));
    }
    Ok(derivation)
}

pub(crate) fn baseline_generated_runtime_helper_plan_is_native_exit_eligible(
    runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
) -> bool {
    runtime_helper_plan.proof_count() != 0
        && (0..runtime_helper_plan.proof_count()).all(|index| {
            runtime_helper_plan
                .proof_at(index)
                .map(|proof| {
                    baseline_runtime_helper_opcode_is_native_exit_eligible(
                        proof.proof.contract.opcode,
                    )
                })
                .unwrap_or(false)
        })
}

const fn baseline_runtime_helper_opcode_is_native_exit_eligible(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::NewObject
            | CoreOpcode::NewArray
            | CoreOpcode::LoadString
            | CoreOpcode::LoadBigInt
            | CoreOpcode::LoadCapture
            | CoreOpcode::NewClosureCell
            | CoreOpcode::GetClosureCell
            | CoreOpcode::PutClosureCell
            | CoreOpcode::ArrayAppend
            | CoreOpcode::TypeOf
            | CoreOpcode::Throw
    )
}

pub(crate) fn derive_baseline_generated_property_handoff_plan_from_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
) -> Result<BaselineGeneratedPropertyHandoffPlanDerivation, JitPlanValidationError> {
    derive_baseline_generated_property_handoff_plan_from_code_block_with_cache_validation(
        code_block,
        owner,
        PropertyHandoffBytecodeCacheValidation::ColdInstall,
    )
}

pub(crate) fn derive_baseline_generated_property_handoff_plan_from_current_code_block_metadata(
    code_block: &CodeBlock,
    owner: CodeBlockId,
) -> Result<BaselineGeneratedPropertyHandoffPlanDerivation, JitPlanValidationError> {
    derive_baseline_generated_property_handoff_plan_from_code_block_with_cache_validation(
        code_block,
        owner,
        PropertyHandoffBytecodeCacheValidation::CurrentMetadata,
    )
}

fn derive_baseline_generated_property_handoff_plan_from_code_block_with_cache_validation(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    cache_validation: PropertyHandoffBytecodeCacheValidation,
) -> Result<BaselineGeneratedPropertyHandoffPlanDerivation, JitPlanValidationError> {
    let mut sites = Vec::new();

    for (ordinal, decoded) in code_block
        .unlinked()
        .instructions()
        .decoded_instructions()
        .enumerate()
    {
        let decoded = decoded.map_err(|error| {
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            }
        })?;
        if !matches!(
            CoreOpcode::from_opcode(decoded.opcode),
            Some(
                CoreOpcode::GetByName
                    | CoreOpcode::GetGlobalObjectProperty
                    | CoreOpcode::GetLength
                    | CoreOpcode::PutByName
                    | CoreOpcode::PutGlobalObjectProperty
                    | CoreOpcode::GetByValue
                    | CoreOpcode::PutByValue
                    | CoreOpcode::InById
                    | CoreOpcode::InByVal
            )
        ) {
            continue;
        }

        let site = derive_baseline_generated_property_handoff_site_with_cache_validation(
            code_block,
            owner,
            decoded,
            cache_validation,
        )?;
        sites.push(site);
    }

    let metadata = if sites.is_empty() {
        None
    } else {
        Some(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                code_block, sites,
            )?,
        )
    };

    Ok(BaselineGeneratedPropertyHandoffPlanDerivation { metadata })
}

pub(crate) fn derive_baseline_generated_js_call_native_exit_plan_from_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
) -> Result<BaselineGeneratedJsCallNativeExitPlanDerivation, JitPlanValidationError> {
    let mut sites = Vec::new();

    for (ordinal, decoded) in code_block
        .unlinked()
        .instructions()
        .decoded_instructions()
        .enumerate()
    {
        let decoded = decoded.map_err(|error| {
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            }
        })?;
        if !matches!(
            CoreOpcode::from_opcode(decoded.opcode),
            Some(CoreOpcode::Call | CoreOpcode::CallWithThis | CoreOpcode::Construct)
        ) {
            continue;
        }

        sites.push(derive_baseline_generated_js_call_native_exit_site(
            code_block, owner, decoded,
        )?);
    }

    let metadata = if sites.is_empty() {
        None
    } else {
        Some(
            BaselineGeneratedJsCallNativeExitPlanMetadata::from_code_block_snapshot(
                code_block, sites,
            )?,
        )
    };

    Ok(BaselineGeneratedJsCallNativeExitPlanDerivation { metadata })
}

pub(crate) fn derive_baseline_generated_owner_continuation_map_from_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
) -> Result<BaselineGeneratedOwnerContinuationMapDerivation, JitPlanValidationError> {
    let decoded = collect_decoded_instructions_for_owner_continuation_map(code_block)?;
    let mut labels = Vec::with_capacity(decoded.len());
    let mut call_sites = Vec::new();

    for (index, instruction) in decoded.iter().copied().enumerate() {
        let Some(opcode) = CoreOpcode::from_opcode(instruction.opcode) else {
            continue;
        };
        let next_bytecode_index = decoded
            .get(index.saturating_add(1))
            .map(|next| next.bytecode_index);
        labels.push(BaselineGeneratedOwnerBytecodeLabel {
            owner,
            bytecode_index: instruction.bytecode_index,
            opcode,
            next_bytecode_index,
        });

        if !baseline_opcode_is_generated_js_call_handoff(opcode) {
            continue;
        }
        let native_exit_site =
            derive_baseline_generated_js_call_native_exit_site(code_block, owner, instruction)?;
        call_sites.push(BaselineGeneratedOwnerContinuationSite {
            owner,
            call_bytecode_index: native_exit_site.bytecode_index,
            opcode,
            destination: native_exit_site.destination,
            argument_count_including_this: native_exit_site
                .provided_argument_count
                .saturating_add(1),
            resume_bytecode_index: native_exit_site.resume_bytecode_index,
            kind: owner_continuation_kind_for_opcode(opcode).ok_or(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapUnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode,
                },
            )?,
            result_profile: baseline_generated_owner_call_result_profile_site(
                code_block,
                instruction.bytecode_index,
                opcode,
            )?,
        });
    }

    let metadata = if labels.is_empty() {
        None
    } else {
        Some(
            BaselineGeneratedOwnerContinuationMapMetadata::from_code_block_snapshot(
                code_block, labels, call_sites,
            )?,
        )
    };

    Ok(BaselineGeneratedOwnerContinuationMapDerivation { metadata })
}

#[allow(dead_code)]
pub(crate) fn validate_baseline_generated_owner_continuation_map_against_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    owner_continuation_map: &BaselineGeneratedOwnerContinuationMapMetadata,
) -> Result<BaselineGeneratedOwnerContinuationMapDerivation, JitPlanValidationError> {
    let derivation =
        derive_baseline_generated_owner_continuation_map_from_code_block(code_block, owner)?;
    if derivation.metadata.as_ref() != Some(owner_continuation_map) {
        return Err(baseline_generated_owner_continuation_map_mismatch_error(
            code_block,
            derivation.metadata.as_ref(),
            owner_continuation_map,
        ));
    }
    Ok(derivation)
}

pub(crate) fn validate_baseline_generated_js_call_native_exit_site_against_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    site: &BaselineGeneratedJsCallNativeExitSite,
) -> Result<(), JitPlanValidationError> {
    let instruction = code_block
        .decoded_instruction_at(site.bytecode_index)
        .map_err(
            |error| JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: site.bytecode_index,
                error,
            },
        )?;
    let derived =
        derive_baseline_generated_js_call_native_exit_site(code_block, owner, instruction)?;
    if &derived != site {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanCodeBlockDerivationMismatch {
                expected_site_count: 1,
                actual_site_count: 1,
                first_mismatch: Some(0),
                bytecode_snapshot_matches: Some(true),
            },
        );
    }
    Ok(())
}

pub(crate) fn validate_baseline_generated_property_handoff_plan_against_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    property_handoff_plan: &BaselineGeneratedPropertyHandoffPlanMetadata,
) -> Result<BaselineGeneratedPropertyHandoffPlanDerivation, JitPlanValidationError> {
    let derivation =
        derive_baseline_generated_property_handoff_plan_from_code_block(code_block, owner)?;
    if derivation.metadata.as_ref() != Some(property_handoff_plan) {
        return Err(baseline_generated_property_handoff_plan_mismatch_error(
            code_block,
            derivation.metadata.as_ref(),
            property_handoff_plan,
        ));
    }
    Ok(derivation)
}

pub(crate) fn validate_baseline_generated_property_handoff_site_against_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Result<(), JitPlanValidationError> {
    let instruction = code_block
        .decoded_instruction_at(site.bytecode_index)
        .map_err(
            |error| JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: site.bytecode_index,
                error,
            },
        )?;
    let derived = derive_baseline_generated_property_handoff_site(code_block, owner, instruction)?;
    if &derived != site {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
                expected_site_count: 1,
                actual_site_count: 1,
                first_mismatch: Some(0),
                bytecode_snapshot_matches: Some(true),
            },
        );
    }
    Ok(())
}

fn baseline_generated_property_handoff_plan_mismatch_error(
    code_block: &CodeBlock,
    expected: Option<&BaselineGeneratedPropertyHandoffPlanMetadata>,
    actual: &BaselineGeneratedPropertyHandoffPlanMetadata,
) -> JitPlanValidationError {
    let expected_site_count = expected.map_or(0, |metadata| metadata.site_count());
    let actual_site_count = actual.site_count();
    let first_mismatch = expected.and_then(|metadata| {
        let shared_count = metadata.site_count().min(actual.site_count());
        (0..shared_count).find(|index| metadata.site_at(*index) != actual.site_at(*index))
    });
    let expected_snapshot = match expected {
        Some(metadata) => Some(metadata.bytecode_snapshot()),
        None => baseline_bytecode_snapshot_fingerprint_from_code_block(code_block).ok(),
    };

    JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
        expected_site_count,
        actual_site_count,
        first_mismatch,
        bytecode_snapshot_matches: expected_snapshot
            .map(|expected_snapshot| expected_snapshot == actual.bytecode_snapshot()),
    }
}

#[allow(dead_code)]
fn baseline_generated_owner_continuation_map_mismatch_error(
    code_block: &CodeBlock,
    expected: Option<&BaselineGeneratedOwnerContinuationMapMetadata>,
    actual: &BaselineGeneratedOwnerContinuationMapMetadata,
) -> JitPlanValidationError {
    let expected_label_count = expected.map_or(0, |metadata| metadata.label_count());
    let actual_label_count = actual.label_count();
    let first_label_mismatch = expected.and_then(|metadata| {
        let shared_count = metadata.label_count().min(actual.label_count());
        (0..shared_count).find(|index| metadata.label_at(*index) != actual.label_at(*index))
    });
    let expected_site_count = expected.map_or(0, |metadata| metadata.call_site_count());
    let actual_site_count = actual.call_site_count();
    let first_site_mismatch = expected.and_then(|metadata| {
        let shared_count = metadata.call_site_count().min(actual.call_site_count());
        (0..shared_count).find(|index| metadata.call_site_at(*index) != actual.call_site_at(*index))
    });
    let expected_snapshot = match expected {
        Some(metadata) => Some(metadata.bytecode_snapshot()),
        None => baseline_bytecode_snapshot_fingerprint_from_code_block(code_block).ok(),
    };

    JitPlanValidationError::BaselineGeneratedOwnerContinuationMapCodeBlockDerivationMismatch {
        expected_label_count,
        actual_label_count,
        first_label_mismatch,
        expected_site_count,
        actual_site_count,
        first_site_mismatch,
        bytecode_snapshot_matches: expected_snapshot
            .map(|expected_snapshot| expected_snapshot == actual.bytecode_snapshot()),
    }
}

fn collect_decoded_instructions_for_owner_continuation_map(
    code_block: &CodeBlock,
) -> Result<Vec<DecodedInstruction<'_>>, JitPlanValidationError> {
    let mut decoded = Vec::new();
    for (ordinal, instruction) in code_block
        .unlinked()
        .instructions()
        .decoded_instructions()
        .enumerate()
    {
        decoded.push(instruction.map_err(|error| {
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            }
        })?);
    }
    Ok(decoded)
}

const fn owner_continuation_kind_for_opcode(
    opcode: CoreOpcode,
) -> Option<BaselineGeneratedOwnerContinuationKind> {
    match opcode {
        CoreOpcode::Call | CoreOpcode::CallWithThis => {
            Some(BaselineGeneratedOwnerContinuationKind::Call)
        }
        CoreOpcode::Construct => Some(BaselineGeneratedOwnerContinuationKind::Construct),
        _ => None,
    }
}

fn baseline_generated_owner_call_result_profile_site(
    code_block: &CodeBlock,
    bytecode_index: BytecodeIndex,
    opcode: CoreOpcode,
) -> Result<Option<BaselineGeneratedOwnerCallResultProfileSite>, JitPlanValidationError> {
    if !matches!(opcode, CoreOpcode::Call | CoreOpcode::CallWithThis) {
        return Ok(None);
    }

    let target = code_block
        .side_tables()
        .value_profiles
        .jit_store_target(
            bytecode_index,
            crate::bytecode::Checkpoint::NONE,
            ValueProfileBucketKind::Sample,
        )
        .map_err(|_| {
            JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMalformedSite {
                bytecode_index,
            }
        })?;
    Ok(Some(BaselineGeneratedOwnerCallResultProfileSite {
        profile_slot: target.binding.profile_slot,
        bytecode_index: target.binding.bytecode_index,
        checkpoint: target.binding.checkpoint,
        bucket_kind: target.binding.kind,
        storage_generation: target.binding.storage_generation,
        value_profile_offset: target.binding.value_profile_offset,
        metadata_table_displacement: target.binding.metadata_table_displacement,
        metadata_table_base_address: target.metadata_table_base_address,
        raw_bucket_address: target.raw_bucket_address,
        raw_bucket_bytes: target.raw_bucket_bytes,
        emission_policy: target.binding.emission_policy,
    }))
}

fn derive_baseline_generated_js_call_native_exit_site(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    instruction: DecodedInstruction<'_>,
) -> Result<BaselineGeneratedJsCallNativeExitSite, JitPlanValidationError> {
    let bytecode_index = instruction.bytecode_index;
    if !bytecode_index.is_valid() {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanInvalidBytecodeIndex {
                bytecode_index,
            },
        );
    }
    let opcode = CoreOpcode::from_opcode(instruction.opcode).ok_or(
        JitPlanValidationError::BaselineEligibilityUnsupportedNonCoreOpcode {
            bytecode_index,
            opcode: instruction.opcode,
            reason: BaselineOpcodeRejectionReason::Call,
        },
    )?;
    if !baseline_opcode_is_generated_js_call_handoff(opcode) {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanUnsupportedOpcode {
                bytecode_index,
                opcode,
            },
        );
    }

    let destination = js_call_native_exit_register_operand(instruction, opcode, 0)?;
    let callee = js_call_native_exit_register_operand(instruction, opcode, 1)?;
    let (this_register, argument_count_operand, first_argument_operand) = match opcode {
        CoreOpcode::Call | CoreOpcode::Construct => (None, 2usize, 3usize),
        CoreOpcode::CallWithThis => (
            Some(js_call_native_exit_register_operand(
                instruction,
                opcode,
                2,
            )?),
            3usize,
            4usize,
        ),
        _ => unreachable!(),
    };
    let provided_argument_count = js_call_native_exit_unsigned_immediate_operand(
        instruction,
        opcode,
        argument_count_operand,
    )?;
    let expected_operand_count = first_argument_operand
        .checked_add(usize::try_from(provided_argument_count).unwrap_or(usize::MAX))
        .ok_or(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanOperandCountMismatch {
                bytecode_index,
                opcode,
                expected: usize::MAX,
                actual: instruction.operands.len(),
            },
        )?;
    if instruction.operands.len() != expected_operand_count {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanOperandCountMismatch {
                bytecode_index,
                opcode,
                expected: expected_operand_count,
                actual: instruction.operands.len(),
            },
        );
    }

    let mut argument_registers =
        Vec::with_capacity(usize::try_from(provided_argument_count).unwrap_or(usize::MAX));
    for argument_index in 0..provided_argument_count {
        let operand_index = first_argument_operand
            .saturating_add(usize::try_from(argument_index).unwrap_or(usize::MAX));
        argument_registers.push(js_call_native_exit_register_operand(
            instruction,
            opcode,
            operand_index,
        )?);
    }

    Ok(BaselineGeneratedJsCallNativeExitSite {
        owner,
        bytecode_index,
        opcode,
        destination,
        callee,
        this_register,
        provided_argument_count,
        argument_registers,
        resume_bytecode_index: next_baseline_generated_site_bytecode_index(
            code_block,
            bytecode_index,
        )?,
        requires_no_gc_exit_reentry: true,
        may_throw: true,
    })
}

fn js_call_native_exit_register_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    operand_index: usize,
) -> Result<VirtualRegister, JitPlanValidationError> {
    instruction
        .register_operand(operand_index)
        .map_err(|error| {
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanOperandShape {
                bytecode_index: instruction.bytecode_index,
                opcode,
                operand_index,
                error,
            }
        })
}

fn js_call_native_exit_unsigned_immediate_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    operand_index: usize,
) -> Result<u32, JitPlanValidationError> {
    instruction
        .unsigned_immediate_operand(operand_index)
        .map_err(|error| {
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanOperandShape {
                bytecode_index: instruction.bytecode_index,
                opcode,
                operand_index,
                error,
            }
        })
}

fn next_baseline_generated_site_bytecode_index(
    code_block: &CodeBlock,
    bytecode_index: BytecodeIndex,
) -> Result<Option<BytecodeIndex>, JitPlanValidationError> {
    let next = BytecodeIndex::from_offset(bytecode_index.offset().saturating_add(1));
    match code_block.decoded_instruction_at(next) {
        Ok(instruction) => Ok(Some(instruction.bytecode_index)),
        Err(InstructionDecodeError::MissingInstruction { .. }) => Ok(None),
        Err(error) => Err(
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: next,
                error,
            },
        ),
    }
}

fn baseline_generated_runtime_helper_plan_mismatch_error(
    code_block: &CodeBlock,
    expected: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    actual: &BaselineGeneratedRuntimeHelperPlanMetadata,
) -> JitPlanValidationError {
    let expected_proof_count = expected.map_or(0, |metadata| metadata.proof_count());
    let actual_proof_count = actual.proof_count();
    let first_mismatch = expected.and_then(|metadata| {
        let shared_count = metadata.proof_count().min(actual.proof_count());
        (0..shared_count).find(|index| metadata.proof_at(*index) != actual.proof_at(*index))
    });
    let expected_snapshot = match expected {
        Some(metadata) => Some(metadata.bytecode_snapshot()),
        None => baseline_bytecode_snapshot_fingerprint_from_code_block(code_block).ok(),
    };

    JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanCodeBlockDerivationMismatch {
        expected_proof_count,
        actual_proof_count,
        first_mismatch,
        bytecode_snapshot_matches: expected_snapshot
            .map(|expected_snapshot| expected_snapshot == actual.bytecode_snapshot()),
    }
}

fn derive_baseline_generated_property_handoff_site(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    instruction: DecodedInstruction<'_>,
) -> Result<BaselineGeneratedPropertyHandoffSite, JitPlanValidationError> {
    derive_baseline_generated_property_handoff_site_with_cache_validation(
        code_block,
        owner,
        instruction,
        PropertyHandoffBytecodeCacheValidation::ColdInstall,
    )
}

pub(crate) fn validate_baseline_generated_property_handoff_site_against_current_code_block(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Result<(), JitPlanValidationError> {
    let instruction = code_block
        .decoded_instruction_at(site.bytecode_index)
        .map_err(|_| {
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanInvalidBytecodeIndex {
                bytecode_index: site.bytecode_index,
            }
        })?;
    let derived = derive_baseline_generated_property_handoff_site_with_cache_validation(
        code_block,
        owner,
        instruction,
        PropertyHandoffBytecodeCacheValidation::CurrentMetadata,
    )?;
    if derived != *site {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
                expected_site_count: 1,
                actual_site_count: 1,
                first_mismatch: Some(0),
                bytecode_snapshot_matches: baseline_bytecode_snapshot_fingerprint_from_code_block(
                    code_block,
                )
                .ok()
                .map(|_| true),
            },
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PropertyHandoffBytecodeCacheValidation {
    ColdInstall,
    CurrentMetadata,
}

fn derive_baseline_generated_property_handoff_site_with_cache_validation(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    instruction: DecodedInstruction<'_>,
    cache_validation: PropertyHandoffBytecodeCacheValidation,
) -> Result<BaselineGeneratedPropertyHandoffSite, JitPlanValidationError> {
    let bytecode_index = instruction.bytecode_index;
    if !bytecode_index.is_valid() {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanInvalidBytecodeIndex {
                bytecode_index,
            },
        );
    }
    let opcode = CoreOpcode::from_opcode(instruction.opcode).ok_or(
        JitPlanValidationError::BaselineEligibilityUnsupportedNonCoreOpcode {
            bytecode_index,
            opcode: instruction.opcode,
            reason: BaselineOpcodeRejectionReason::PropertyAccess,
        },
    )?;
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
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanUnsupportedOpcode {
                bytecode_index,
                opcode,
            },
        );
    }

    let base = match opcode {
        CoreOpcode::GetByName
        | CoreOpcode::GetLength
        | CoreOpcode::GetByValue
        | CoreOpcode::InById
        | CoreOpcode::InByVal => Some(instruction.register_operand(1).map_err(|_| {
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissingBaseRegister {
                bytecode_index,
                opcode,
            }
        })?),
        CoreOpcode::GetGlobalObjectProperty | CoreOpcode::PutGlobalObjectProperty => None,
        CoreOpcode::PutByName | CoreOpcode::PutByValue => {
            Some(instruction.register_operand(0).map_err(|_| {
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissingBaseRegister {
                    bytecode_index,
                    opcode,
                }
            })?)
        }
        _ => unreachable!(),
    };
    let property_key = property_cache_key_from_instruction(instruction, opcode)?;
    let (slot, cache) = unique_property_access_slot_for_bytecode_index(code_block, bytecode_index)?;
    validate_property_inline_cache(
        cache,
        bytecode_index,
        opcode,
        base,
        property_key,
        cache_validation,
    )?;

    let cache_kind = property_handoff_cache_kind_for_opcode(opcode);
    let cold_miss_handoff =
        derive_property_miss_handoff_descriptor(owner, slot, bytecode_index, cache_kind)?;

    let site = match opcode {
        CoreOpcode::GetByName => BaselineGeneratedPropertyHandoffSite::get_by_name_property_load(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::Key(property_key) => property_key,
                _ => unreachable!(),
            },
        ),
        CoreOpcode::GetGlobalObjectProperty => {
            BaselineGeneratedPropertyHandoffSite::get_global_object_property_load(
                owner,
                slot,
                bytecode_index,
                match property_key {
                    PropertyCacheKey::Key(property_key) => property_key,
                    _ => unreachable!(),
                },
            )
        }
        CoreOpcode::GetLength => BaselineGeneratedPropertyHandoffSite::get_length_property_load(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::Key(property_key) => property_key,
                _ => unreachable!(),
            },
        ),
        CoreOpcode::PutByName => BaselineGeneratedPropertyHandoffSite::put_by_name_property_store(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::Key(property_key) => property_key,
                _ => unreachable!(),
            },
        ),
        CoreOpcode::PutGlobalObjectProperty => {
            BaselineGeneratedPropertyHandoffSite::put_global_object_property_store(
                owner,
                slot,
                bytecode_index,
                match property_key {
                    PropertyCacheKey::Key(property_key) => property_key,
                    _ => unreachable!(),
                },
            )
        }
        CoreOpcode::GetByValue => BaselineGeneratedPropertyHandoffSite::get_by_value_element_load(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::RuntimeValue(property_register) => property_register,
                _ => unreachable!(),
            },
        ),
        CoreOpcode::PutByValue => BaselineGeneratedPropertyHandoffSite::put_by_value_element_store(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::RuntimeValue(property_register) => property_register,
                _ => unreachable!(),
            },
        ),
        CoreOpcode::InById => BaselineGeneratedPropertyHandoffSite::in_by_id_has(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::Key(property_key) => property_key,
                _ => unreachable!(),
            },
        ),
        CoreOpcode::InByVal => BaselineGeneratedPropertyHandoffSite::in_by_value_has(
            owner,
            slot,
            bytecode_index,
            match property_key {
                PropertyCacheKey::RuntimeValue(property_register) => property_register,
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    Ok(site.with_cold_miss_handoff(cold_miss_handoff))
}

fn derive_property_miss_handoff_descriptor(
    owner: CodeBlockId,
    slot: InlineCacheSlotId,
    bytecode_index: BytecodeIndex,
    cache_kind: InlineCacheKind,
) -> Result<InlineCacheMissHandoffDescriptor, JitPlanValidationError> {
    let mut builder = InlineCacheSlot::builder(slot, cache_kind)
        .owner(owner)
        .bytecode_index(bytecode_index.offset())
        .state(JitInlineCacheState::ColdSlowPath);
    if matches!(
        cache_kind,
        InlineCacheKind::PropertyStore | InlineCacheKind::ElementStore
    ) {
        builder = builder.barrier_metadata(InlineCacheBarrierMetadata::store_value(
            InlineCacheBarrierTarget::StoredValue,
        ));
    }
    let ic_slot = builder.build().map_err(|error| {
        JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffInvalid {
            bytecode_index,
            error,
        }
    })?;
    InlineCacheMissHandoffDescriptor::from_slot(&ic_slot, InlineCacheMissKind::Cold, None).map_err(
        |error| JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffInvalid {
            bytecode_index,
            error,
        },
    )
}

fn unique_property_access_slot_for_bytecode_index(
    code_block: &CodeBlock,
    bytecode_index: BytecodeIndex,
) -> Result<(InlineCacheSlotId, &PropertyInlineCache), JitPlanValidationError> {
    let mut matching = code_block
        .side_tables()
        .inline_caches
        .property_accesses
        .iter()
        .enumerate()
        .filter(|(_, cache)| cache.bytecode_index == bytecode_index);
    let Some((slot, cache)) = matching.next() else {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissingBytecodeCache {
                bytecode_index,
            },
        );
    };
    if matching.next().is_some() {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanDuplicateBytecodeCache {
                bytecode_index,
            },
        );
    }
    Ok((InlineCacheSlotId(slot as u32), cache))
}

fn property_cache_key_from_instruction(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
) -> Result<PropertyCacheKey, JitPlanValidationError> {
    let bytecode_index = instruction.bytecode_index;
    let property_operand_index = match opcode {
        CoreOpcode::GetByName => 2,
        CoreOpcode::GetGlobalObjectProperty => 1,
        CoreOpcode::GetLength => 2,
        CoreOpcode::PutByName => 1,
        CoreOpcode::PutGlobalObjectProperty => 0,
        CoreOpcode::GetByValue => 2,
        CoreOpcode::PutByValue => 1,
        CoreOpcode::InById | CoreOpcode::InByVal => 2,
        _ => {
            return Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanUnsupportedOpcode {
                    bytecode_index,
                    opcode,
                },
            )
        }
    };
    let property_operand = instruction.operand(property_operand_index).map_err(|_| {
        JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissingPropertyIdentifier {
            bytecode_index,
            opcode,
        }
    })?;
    match (opcode, property_operand) {
        (
            CoreOpcode::GetByName
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::GetLength
            | CoreOpcode::PutByName
            | CoreOpcode::PutGlobalObjectProperty,
            Operand::IdentifierIndex(identifier_index),
        ) => Ok(PropertyCacheKey::Key(PropertyKey::from_identifier(
            Identifier::from_atom(AtomId::from_table_slot(identifier_index)),
        ))),
        (CoreOpcode::InById, Operand::IdentifierIndex(identifier_index)) => {
            Ok(PropertyCacheKey::Key(PropertyKey::from_identifier(
                Identifier::from_atom(AtomId::from_table_slot(identifier_index)),
            )))
        }
        (
            CoreOpcode::GetByValue | CoreOpcode::PutByValue | CoreOpcode::InByVal,
            Operand::Register(register),
        ) => Ok(PropertyCacheKey::RuntimeValue(register)),
        _ => Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissingPropertyIdentifier {
                bytecode_index,
                opcode,
            },
        ),
    }
}

fn validate_property_inline_cache(
    cache: &PropertyInlineCache,
    bytecode_index: BytecodeIndex,
    opcode: CoreOpcode,
    base: Option<VirtualRegister>,
    property_key: PropertyCacheKey,
    cache_validation: PropertyHandoffBytecodeCacheValidation,
) -> Result<(), JitPlanValidationError> {
    let expected = property_handoff_shape_for_opcode(opcode)?;
    if cache.bytecode_index != bytecode_index {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanBytecodeIndexMismatch {
                expected: bytecode_index,
                actual: cache.bytecode_index,
            },
        );
    }
    if cache.access != expected.access {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanAccessMismatch {
                bytecode_index,
                expected: expected.access,
                actual: cache.access,
            },
        );
    }
    if cache.kind != expected.property_cache_kind {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanPropertyCacheKindMismatch {
                bytecode_index,
                expected: expected.property_cache_kind,
                actual: cache.kind,
            },
        );
    }
    if cache_validation == PropertyHandoffBytecodeCacheValidation::ColdInstall {
        if cache.dispatch != PropertyInlineCacheDispatch::Unlinked
            || cache.mutation_authority != InlineCacheMutationAuthority::LinkedCodeBlock
        {
            return Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMalformedBytecodeCache {
                    bytecode_index,
                },
            );
        }
        if cache.state != BytecodeInlineCacheState::Unset {
            return Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanBytecodeCacheStateMismatch {
                    bytecode_index,
                    expected: BytecodeInlineCacheState::Unset,
                    actual: cache.state,
                },
            );
        }
    }
    if cache.base != base {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanBaseMismatch {
                bytecode_index,
                expected: base.unwrap_or(VirtualRegister::INVALID),
                actual: cache.base,
            },
        );
    }
    if cache.property != property_key {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanPropertyKeyMismatch {
                bytecode_index,
                expected: property_key,
                actual: cache.property,
            },
        );
    }
    if cache.get_by_id.is_some() != expected.has_get_by_id
        || cache.put_by_id.is_some() != expected.has_put_by_id
    {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMalformedBytecodeCache {
                bytecode_index,
            },
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BaselineGeneratedPropertyHandoffShape {
    cache_kind: InlineCacheKind,
    access: PropertyAccessType,
    property_cache_kind: PropertyCacheKind,
    has_get_by_id: bool,
    has_put_by_id: bool,
}

fn property_handoff_shape_for_opcode(
    opcode: CoreOpcode,
) -> Result<BaselineGeneratedPropertyHandoffShape, JitPlanValidationError> {
    match opcode {
        CoreOpcode::GetByName | CoreOpcode::GetGlobalObjectProperty | CoreOpcode::GetLength => {
            Ok(BaselineGeneratedPropertyHandoffShape {
                cache_kind: InlineCacheKind::PropertyLoad,
                access: PropertyAccessType::GetById,
                property_cache_kind: PropertyCacheKind::GetById,
                has_get_by_id: true,
                has_put_by_id: false,
            })
        }
        CoreOpcode::PutByName => Ok(BaselineGeneratedPropertyHandoffShape {
            cache_kind: InlineCacheKind::PropertyStore,
            access: PropertyAccessType::PutByIdSloppy,
            property_cache_kind: PropertyCacheKind::PutById,
            has_get_by_id: false,
            has_put_by_id: true,
        }),
        CoreOpcode::PutGlobalObjectProperty => Ok(BaselineGeneratedPropertyHandoffShape {
            cache_kind: InlineCacheKind::PropertyStore,
            access: PropertyAccessType::PutByIdSloppy,
            property_cache_kind: PropertyCacheKind::PutById,
            has_get_by_id: false,
            has_put_by_id: true,
        }),
        CoreOpcode::GetByValue => Ok(BaselineGeneratedPropertyHandoffShape {
            cache_kind: InlineCacheKind::ElementLoad,
            access: PropertyAccessType::GetByVal,
            property_cache_kind: PropertyCacheKind::GetByVal,
            has_get_by_id: false,
            has_put_by_id: false,
        }),
        CoreOpcode::PutByValue => Ok(BaselineGeneratedPropertyHandoffShape {
            cache_kind: InlineCacheKind::ElementStore,
            access: PropertyAccessType::PutByValSloppy,
            property_cache_kind: PropertyCacheKind::PutByVal,
            has_get_by_id: false,
            has_put_by_id: false,
        }),
        CoreOpcode::InById => Ok(BaselineGeneratedPropertyHandoffShape {
            cache_kind: InlineCacheKind::HasProperty,
            access: PropertyAccessType::InById,
            property_cache_kind: PropertyCacheKind::InById,
            has_get_by_id: false,
            has_put_by_id: false,
        }),
        CoreOpcode::InByVal => Ok(BaselineGeneratedPropertyHandoffShape {
            cache_kind: InlineCacheKind::HasProperty,
            access: PropertyAccessType::InByVal,
            property_cache_kind: PropertyCacheKind::InByVal,
            has_get_by_id: false,
            has_put_by_id: false,
        }),
        _ => Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanUnsupportedOpcode {
                bytecode_index: BytecodeIndex::INVALID,
                opcode,
            },
        ),
    }
}

fn property_handoff_cache_kind_for_opcode(opcode: CoreOpcode) -> InlineCacheKind {
    match opcode {
        CoreOpcode::GetByName | CoreOpcode::GetGlobalObjectProperty | CoreOpcode::GetLength => {
            InlineCacheKind::PropertyLoad
        }
        CoreOpcode::PutByName | CoreOpcode::PutGlobalObjectProperty => {
            InlineCacheKind::PropertyStore
        }
        CoreOpcode::GetByValue => InlineCacheKind::ElementLoad,
        CoreOpcode::PutByValue => InlineCacheKind::ElementStore,
        CoreOpcode::InById | CoreOpcode::InByVal => InlineCacheKind::HasProperty,
        _ => unreachable!(),
    }
}

fn derive_baseline_generated_runtime_helper_site(
    code_block: &CodeBlock,
    owner: CodeBlockId,
    instruction: DecodedInstruction<'_>,
    helper: BaselineGeneratedRuntimeHelperDescriptor,
    safepoint_id: CompilerSafepointId,
) -> Result<
    (
        BaselineGeneratedRuntimeBoundaryProof,
        CompilerSafepointDescriptor,
    ),
    JitPlanValidationError,
> {
    let bytecode_index = instruction.bytecode_index;
    let opcode = helper.opcode();
    let required_operands = helper.required_operands(code_block, instruction)?;
    let root_map = select_baseline_generated_runtime_helper_root_map(
        code_block,
        bytecode_index,
        opcode,
        safepoint_id,
    )?;
    let contract = baseline_generated_runtime_boundary_contract(opcode).map_err(|_| {
        JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationRejectedOpcode {
            bytecode_index,
            opcode,
        }
    })?;
    let safepoint = CompilerSafepointDescriptor {
        id: safepoint_id,
        owner: Some(owner),
        code: None,
        tier: JitType::Baseline,
        kind: CompilerSafepointKind::Call,
        bytecode_index: Some(bytecode_index),
        root_map: root_map.as_ref().map(|root_map| root_map.id),
        roots: Vec::new(),
        may_call: true,
        may_allocate: contract.effects.allocates,
    };
    let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
        opcode,
        safepoint: safepoint.clone(),
        root_map,
        no_gc_exit_reentry: true,
    };
    let proof = candidate.validate().map_err(|error| {
        runtime_boundary_validation_error_to_jit_plan_error(error, bytecode_index, opcode)
    })?;
    let root_map =
        candidate
            .root_map
            .as_ref()
            .ok_or(JitPlanValidationError::SafepointMissingRootMap(
                safepoint_id,
            ))?;
    if let Some(destination) = required_operands.destination {
        if !helper.root_map_contains_register_slot(root_map, bytecode_index, destination) {
            return Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationSlot {
                    bytecode_index,
                    opcode,
                    root_map: root_map.id,
                    register: destination,
                },
            );
        }
    }
    for source in required_operands.sources {
        if !helper.root_map_contains_register_slot(root_map, bytecode_index, source) {
            return Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceSlot {
                    bytecode_index,
                    opcode,
                    root_map: root_map.id,
                    register: source,
                },
            );
        }
    }

    Ok((proof, safepoint))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaselineGeneratedRuntimeHelperOperandRole {
    Destination,
    Source,
    StringLiteral,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BaselineGeneratedRuntimeHelperOperandDescriptor {
    role: BaselineGeneratedRuntimeHelperOperandRole,
    operand_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BaselineGeneratedRuntimeHelperRequiredOperands {
    destination: Option<VirtualRegister>,
    sources: Vec<VirtualRegister>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BaselineGeneratedRuntimeHelperDescriptor {
    opcode: CoreOpcode,
    operands: &'static [BaselineGeneratedRuntimeHelperOperandDescriptor],
}

const DESTINATION_OPERAND_0: BaselineGeneratedRuntimeHelperOperandDescriptor =
    BaselineGeneratedRuntimeHelperOperandDescriptor {
        role: BaselineGeneratedRuntimeHelperOperandRole::Destination,
        operand_index: 0,
    };
const SOURCE_OPERAND_1: BaselineGeneratedRuntimeHelperOperandDescriptor =
    BaselineGeneratedRuntimeHelperOperandDescriptor {
        role: BaselineGeneratedRuntimeHelperOperandRole::Source,
        operand_index: 1,
    };
const SOURCE_OPERAND_0: BaselineGeneratedRuntimeHelperOperandDescriptor =
    BaselineGeneratedRuntimeHelperOperandDescriptor {
        role: BaselineGeneratedRuntimeHelperOperandRole::Source,
        operand_index: 0,
    };
const STRING_LITERAL_OPERAND_1: BaselineGeneratedRuntimeHelperOperandDescriptor =
    BaselineGeneratedRuntimeHelperOperandDescriptor {
        role: BaselineGeneratedRuntimeHelperOperandRole::StringLiteral,
        operand_index: 1,
    };
const DESTINATION_ONLY_RUNTIME_HELPER_OPERANDS: [BaselineGeneratedRuntimeHelperOperandDescriptor;
    1] = [DESTINATION_OPERAND_0];
const DESTINATION_SOURCE_RUNTIME_HELPER_OPERANDS:
    [BaselineGeneratedRuntimeHelperOperandDescriptor; 2] =
    [DESTINATION_OPERAND_0, SOURCE_OPERAND_1];
const DESTINATION_STRING_LITERAL_RUNTIME_HELPER_OPERANDS:
    [BaselineGeneratedRuntimeHelperOperandDescriptor; 2] =
    [DESTINATION_OPERAND_0, STRING_LITERAL_OPERAND_1];
const SOURCE_ONLY_RUNTIME_HELPER_OPERANDS: [BaselineGeneratedRuntimeHelperOperandDescriptor; 1] =
    [SOURCE_OPERAND_1];
const THROW_RUNTIME_HELPER_OPERANDS: [BaselineGeneratedRuntimeHelperOperandDescriptor; 1] =
    [SOURCE_OPERAND_0];
const SOURCE_SOURCE_RUNTIME_HELPER_OPERANDS: [BaselineGeneratedRuntimeHelperOperandDescriptor; 2] =
    [SOURCE_OPERAND_0, SOURCE_OPERAND_1];

impl BaselineGeneratedRuntimeHelperDescriptor {
    const fn for_opcode(opcode: CoreOpcode) -> Option<Self> {
        match opcode {
            CoreOpcode::LoadString | CoreOpcode::LoadBigInt => Some(Self {
                opcode,
                operands: &DESTINATION_STRING_LITERAL_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::NewObject | CoreOpcode::NewArray => Some(Self {
                opcode,
                operands: &DESTINATION_ONLY_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::LoadCapture => Some(Self {
                opcode,
                operands: &DESTINATION_ONLY_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::NewClosureCell | CoreOpcode::GetClosureCell => Some(Self {
                opcode,
                operands: &DESTINATION_SOURCE_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::PutClosureCell => Some(Self {
                opcode,
                operands: &SOURCE_SOURCE_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::ArrayAppend => Some(Self {
                opcode,
                operands: &SOURCE_SOURCE_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::LoadFunction => Some(Self {
                opcode,
                operands: &DESTINATION_ONLY_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::InitializeGlobalLexical => Some(Self {
                opcode,
                operands: &SOURCE_ONLY_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::TypeOf => Some(Self {
                opcode,
                operands: &DESTINATION_SOURCE_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::ForInKeys => Some(Self {
                opcode,
                operands: &DESTINATION_SOURCE_RUNTIME_HELPER_OPERANDS,
            }),
            CoreOpcode::Throw => Some(Self {
                opcode,
                operands: &THROW_RUNTIME_HELPER_OPERANDS,
            }),
            _ => None,
        }
    }

    const fn opcode(self) -> CoreOpcode {
        self.opcode
    }

    fn required_operands(
        self,
        code_block: &CodeBlock,
        instruction: DecodedInstruction<'_>,
    ) -> Result<BaselineGeneratedRuntimeHelperRequiredOperands, JitPlanValidationError> {
        let mut destination = None;
        let mut sources = Vec::new();
        for operand in self.operands {
            match operand.role {
                BaselineGeneratedRuntimeHelperOperandRole::Destination => {
                    let register = instruction.register_operand(operand.operand_index).map_err(
                        |_| {
                            JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
                                bytecode_index: instruction.bytecode_index,
                                opcode: self.opcode,
                            }
                        },
                    )?;
                    destination = Some(register);
                }
                BaselineGeneratedRuntimeHelperOperandRole::Source => {
                    let register = instruction.register_operand(operand.operand_index).map_err(
                        |_| {
                            JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceRegister {
                                bytecode_index: instruction.bytecode_index,
                                opcode: self.opcode,
                            }
                        },
                    )?;
                    push_unique_runtime_helper_source(&mut sources, register);
                }
                BaselineGeneratedRuntimeHelperOperandRole::StringLiteral => {
                    let key = match instruction.operand(operand.operand_index) {
                        Ok(Operand::IdentifierIndex(key)) => key,
                        Ok(_) | Err(_) => {
                            return Err(
                                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingStringLiteralOperand {
                                    bytecode_index: instruction.bytecode_index,
                                    opcode: self.opcode,
                                },
                            );
                        }
                    };
                    if code_block.string_literal(key).is_none() {
                        return Err(
                            JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingCodeBlockStringLiteral {
                                bytecode_index: instruction.bytecode_index,
                                opcode: self.opcode,
                                identifier_index: key,
                            },
                        );
                    }
                }
            }
        }
        if self.opcode == CoreOpcode::LoadFunction {
            let capture_count = match instruction.operand(2) {
                Ok(Operand::UnsignedImmediate(count)) => count,
                Ok(_) | Err(_) => {
                    return Err(
                        JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceRegister {
                            bytecode_index: instruction.bytecode_index,
                            opcode: self.opcode,
                        },
                    );
                }
            };
            for capture_index in 0..capture_count {
                let operand_index = usize::try_from(capture_index)
                    .unwrap_or(usize::MAX)
                    .saturating_add(3);
                let register = instruction.register_operand(operand_index).map_err(|_| {
                    JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceRegister {
                        bytecode_index: instruction.bytecode_index,
                        opcode: self.opcode,
                    }
                })?;
                push_unique_runtime_helper_source(&mut sources, register);
            }
        }
        Ok(BaselineGeneratedRuntimeHelperRequiredOperands {
            destination: if self.operands.iter().any(|operand| {
                matches!(
                    operand.role,
                    BaselineGeneratedRuntimeHelperOperandRole::Destination
                )
            }) {
                Some(destination.ok_or(
                    JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
                        bytecode_index: instruction.bytecode_index,
                        opcode: self.opcode,
                    },
                )?)
            } else {
                destination
            },
            sources,
        })
    }

    fn root_map_contains_register_slot(
        self,
        root_map: &BytecodeRootMap,
        bytecode_index: BytecodeIndex,
        required_register: VirtualRegister,
    ) -> bool {
        root_map.slots.iter().any(|slot| {
            slot.bytecode_index == bytecode_index
                && matches!(
                    slot.storage,
                    BytecodeRootSlotStorage::Register(register) if register == required_register
                )
        })
    }
}

fn push_unique_runtime_helper_source(
    sources: &mut Vec<VirtualRegister>,
    register: VirtualRegister,
) {
    if !sources.contains(&register) {
        sources.push(register);
    }
}

fn select_baseline_generated_runtime_helper_root_map(
    code_block: &CodeBlock,
    bytecode_index: BytecodeIndex,
    opcode: CoreOpcode,
    safepoint: CompilerSafepointId,
) -> Result<Option<BytecodeRootMap>, JitPlanValidationError> {
    let root_maps = &code_block.side_tables().root_maps;
    let mut covering = root_maps.iter().filter(|root_map| {
        bytecode_index >= root_map.bytecode_range_start
            && bytecode_index <= root_map.bytecode_range_end
    });
    let first = covering.next();
    if covering.next().is_some() {
        return Err(
            JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationAmbiguousRootMaps {
                bytecode_index,
                opcode,
            },
        );
    }

    if let Some(root_map) = first {
        return Ok(Some(root_map.clone()));
    }

    Err(JitPlanValidationError::SafepointMissingRootMap(safepoint))
}

fn runtime_boundary_validation_error_to_jit_plan_error(
    error: BaselineGeneratedRuntimeBoundaryValidationError,
    bytecode_index: BytecodeIndex,
    opcode: CoreOpcode,
) -> JitPlanValidationError {
    match error {
        BaselineGeneratedRuntimeBoundaryValidationError::Safepoint(error) => error,
        BaselineGeneratedRuntimeBoundaryValidationError::RejectedOpcode { .. } => {
            JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationRejectedOpcode {
                bytecode_index,
                opcode,
            }
        }
        BaselineGeneratedRuntimeBoundaryValidationError::MissingNoGcExitReentry { .. } => {
            JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMissingNoGcExitReentry {
                bytecode_index,
                opcode,
            }
        }
        BaselineGeneratedRuntimeBoundaryValidationError::SafepointMissingAllocationFlag {
            safepoint,
            ..
        } => {
            JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingAllocationFlag {
                bytecode_index,
                opcode,
                safepoint,
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BaselineExceptionMetadataPresence {
    #[default]
    Missing,
    Present {
        handler_count: u32,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BaselineRootMapRequirements {
    pub root_maps: Vec<BytecodeRootMap>,
    pub safepoints: Vec<CompilerSafepointDescriptor>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineRootMapRequirementsProof {
    pub root_map_count: usize,
    pub safepoint_count: usize,
    pub complete_safepoint_root_map_count: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineBytecodeEligibilityRecord {
    pub owner: Option<CodeBlockId>,
    pub snapshot: TieringSnapshot,
    pub bytecode: BaselineBytecodeRange,
    pub opcode_subset: BaselineSupportedOpcodeSubset,
    pub instructions: Vec<BaselineBytecodeInstruction>,
    pub root_map_requirements: BaselineRootMapRequirements,
    pub exception_metadata: BaselineExceptionMetadataPresence,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineBytecodeEligibilityProof {
    owner: CodeBlockId,
    snapshot: TieringSnapshot,
    bytecode: BaselineBytecodeRange,
    opcode_subset: BaselineSupportedOpcodeSubset,
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    generated_effect_contract: BaselineGeneratedEffectContract,
    root_map_requirements: BaselineRootMapRequirementsProof,
    exception_metadata: BaselineExceptionMetadataPresence,
}

impl BaselineBytecodeEligibilityProof {
    pub const fn owner(&self) -> CodeBlockId {
        self.owner
    }

    pub const fn snapshot(&self) -> TieringSnapshot {
        self.snapshot
    }

    pub const fn bytecode(&self) -> BaselineBytecodeRange {
        self.bytecode
    }

    pub const fn opcode_subset(&self) -> BaselineSupportedOpcodeSubset {
        self.opcode_subset
    }

    pub(crate) const fn bytecode_snapshot_fingerprint(
        &self,
    ) -> BaselineBytecodeSnapshotFingerprint {
        self.bytecode_snapshot
    }

    pub const fn generated_effect_contract(&self) -> BaselineGeneratedEffectContract {
        self.generated_effect_contract
    }

    pub const fn root_map_requirements(&self) -> BaselineRootMapRequirementsProof {
        self.root_map_requirements
    }

    pub const fn exception_metadata(&self) -> BaselineExceptionMetadataPresence {
        self.exception_metadata
    }

    pub(crate) fn fingerprint_code_block_snapshot(
        code_block: &CodeBlock,
    ) -> Result<BaselineBytecodeSnapshotFingerprint, JitPlanValidationError> {
        baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)
    }
}

pub(crate) fn bind_baseline_bytecode_proof_owner(
    owner_witness: CodeBlockId,
    proof: &BaselineBytecodeEligibilityProof,
) -> Result<BaselineBytecodeProofBinding, BaselineBytecodeProofBindingError> {
    let proof_owner = proof.owner();
    if owner_witness != proof_owner {
        return Err(BaselineBytecodeProofBindingError::OwnerWitnessMismatch {
            owner_witness,
            proof_owner,
        });
    }

    let snapshot = proof.snapshot();
    if snapshot.owner != proof_owner {
        return Err(
            BaselineBytecodeProofBindingError::ProofSnapshotOwnerMismatch {
                proof_owner,
                snapshot_owner: snapshot.owner,
            },
        );
    }

    if snapshot.to_tier != JitType::Baseline {
        return Err(
            BaselineBytecodeProofBindingError::ProofSnapshotTierMismatch {
                from_tier: snapshot.from_tier,
                to_tier: snapshot.to_tier,
            },
        );
    }

    Ok(BaselineBytecodeProofBinding {
        owner: proof_owner,
        bytecode: proof.bytecode(),
        bytecode_snapshot: proof.bytecode_snapshot_fingerprint(),
    })
}

pub(crate) fn validate_baseline_bytecode_proof_code_block_snapshot(
    code_block: &CodeBlock,
    binding: &BaselineBytecodeProofBinding,
) -> Result<(), BaselineBytecodeProofBindingError> {
    let actual = baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)
        .map_err(|error| BaselineBytecodeProofBindingError::CodeBlockSnapshotInvalid { error })?;
    if actual != binding.bytecode_snapshot {
        return Err(
            BaselineBytecodeProofBindingError::CodeBlockSnapshotMismatch {
                owner: binding.owner,
            },
        );
    }

    Ok(())
}

#[allow(dead_code)]
impl BaselineBytecodeEligibilityRecord {
    pub fn from_code_block_snapshot(
        code_block: &CodeBlock,
        owner: CodeBlockId,
        snapshot: TieringSnapshot,
        opcode_subset: BaselineSupportedOpcodeSubset,
        safepoints: Vec<CompilerSafepointDescriptor>,
    ) -> Result<Self, JitPlanValidationError> {
        let instructions = collect_baseline_bytecode_instructions(
            code_block.unlinked().instructions().decoded_instructions(),
        )?;
        let bytecode = baseline_bytecode_range_from_instructions(&instructions);

        Ok(Self {
            owner: Some(owner),
            snapshot,
            bytecode,
            opcode_subset,
            instructions,
            root_map_requirements: BaselineRootMapRequirements {
                root_maps: code_block.side_tables().root_maps.clone(),
                safepoints,
            },
            exception_metadata: baseline_exception_metadata_from_code_block(code_block),
        })
    }

    pub fn proof_from_code_block_snapshot(
        code_block: &CodeBlock,
        owner: CodeBlockId,
        snapshot: TieringSnapshot,
        opcode_subset: BaselineSupportedOpcodeSubset,
        safepoints: Vec<CompilerSafepointDescriptor>,
    ) -> Result<BaselineBytecodeEligibilityProof, JitPlanValidationError> {
        let record =
            Self::from_code_block_snapshot(code_block, owner, snapshot, opcode_subset, safepoints)?;
        let mut proof = record.validate()?;
        proof.bytecode_snapshot =
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?;
        Ok(proof)
    }

    pub(crate) fn proof_from_code_block_snapshot_with_runtime_helpers(
        code_block: &CodeBlock,
        owner: CodeBlockId,
        snapshot: TieringSnapshot,
        opcode_subset: BaselineSupportedOpcodeSubset,
        safepoints: Vec<CompilerSafepointDescriptor>,
        runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
    ) -> Result<BaselineBytecodeEligibilityProof, JitPlanValidationError> {
        let record =
            Self::from_code_block_snapshot(code_block, owner, snapshot, opcode_subset, safepoints)?;
        let mut proof = record.validate_with_runtime_helpers(runtime_helper_plan)?;
        proof.bytecode_snapshot =
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?;
        Ok(proof)
    }

    pub(crate) fn proof_from_code_block_snapshot_for_mixed_vm_install(
        code_block: &CodeBlock,
        owner: CodeBlockId,
        snapshot: TieringSnapshot,
        opcode_subset: BaselineSupportedOpcodeSubset,
        safepoints: Vec<CompilerSafepointDescriptor>,
        runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    ) -> Result<BaselineBytecodeEligibilityProof, JitPlanValidationError> {
        let record =
            Self::from_code_block_snapshot(code_block, owner, snapshot, opcode_subset, safepoints)?;
        let mut proof = record.validate_mixed_for_vm_install(runtime_helper_plan)?;
        proof.bytecode_snapshot =
            baseline_bytecode_snapshot_fingerprint_from_code_block(code_block)?;
        Ok(proof)
    }

    pub fn validate(&self) -> Result<BaselineBytecodeEligibilityProof, JitPlanValidationError> {
        let owner = self.validated_owner()?;
        self.validate_snapshot(owner)?;
        self.validate_bytecode()?;
        let generated_effect_contract = self.validate_generated_effect_contract()?;
        self.validate_exception_metadata()?;
        let root_map_proof = self.root_map_requirements.validate(owner, self.bytecode)?;

        Ok(BaselineBytecodeEligibilityProof {
            owner,
            snapshot: self.snapshot,
            bytecode: self.bytecode,
            opcode_subset: self.opcode_subset,
            bytecode_snapshot: baseline_bytecode_snapshot_fingerprint_from_record(self),
            generated_effect_contract,
            root_map_requirements: root_map_proof,
            exception_metadata: self.exception_metadata,
        })
    }

    fn validate_with_runtime_helpers(
        &self,
        runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
    ) -> Result<BaselineBytecodeEligibilityProof, JitPlanValidationError> {
        let owner = self.validated_owner()?;
        self.validate_snapshot(owner)?;
        self.validate_bytecode_with_runtime_helpers(runtime_helper_plan)?;
        let generated_effect_contract =
            self.validate_generated_effect_contract_with_runtime_helpers(runtime_helper_plan)?;
        self.validate_exception_metadata()?;
        let root_map_proof = self.root_map_requirements.validate(owner, self.bytecode)?;

        Ok(BaselineBytecodeEligibilityProof {
            owner,
            snapshot: self.snapshot,
            bytecode: self.bytecode,
            opcode_subset: self.opcode_subset,
            bytecode_snapshot: baseline_bytecode_snapshot_fingerprint_from_record(self),
            generated_effect_contract,
            root_map_requirements: root_map_proof,
            exception_metadata: self.exception_metadata,
        })
    }

    fn validate_mixed_for_vm_install(
        &self,
        runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    ) -> Result<BaselineBytecodeEligibilityProof, JitPlanValidationError> {
        let owner = self.validated_owner()?;
        self.validate_snapshot(owner)?;
        let sites = self.validate_mixed_bytecode_sites(runtime_helper_plan)?;
        let generated_effect_contract =
            self.validate_generated_effect_contract_for_sites(&sites)?;
        self.validate_exception_metadata()?;
        let root_map_proof = self.root_map_requirements.validate(owner, self.bytecode)?;

        Ok(BaselineBytecodeEligibilityProof {
            owner,
            snapshot: self.snapshot,
            bytecode: self.bytecode,
            opcode_subset: self.opcode_subset,
            bytecode_snapshot: baseline_bytecode_snapshot_fingerprint_from_record(self),
            generated_effect_contract,
            root_map_requirements: root_map_proof,
            exception_metadata: self.exception_metadata,
        })
    }

    fn validated_owner(&self) -> Result<CodeBlockId, JitPlanValidationError> {
        let owner = self
            .owner
            .ok_or(JitPlanValidationError::BaselineEligibilityMissingOwner)?;
        if owner == CodeBlockId::default() {
            return Err(JitPlanValidationError::BaselineEligibilityDefaultOwner);
        }
        Ok(owner)
    }

    fn validate_snapshot(&self, owner: CodeBlockId) -> Result<(), JitPlanValidationError> {
        if self.snapshot.owner != owner {
            return Err(
                JitPlanValidationError::BaselineEligibilitySnapshotOwnerMismatch {
                    owner,
                    snapshot_owner: self.snapshot.owner,
                },
            );
        }
        if self.snapshot.to_tier != JitType::Baseline {
            return Err(
                JitPlanValidationError::BaselineEligibilitySnapshotTierMismatch {
                    from_tier: self.snapshot.from_tier,
                    to_tier: self.snapshot.to_tier,
                },
            );
        }
        Ok(())
    }

    fn validate_bytecode_shape(&self) -> Result<(), JitPlanValidationError> {
        self.bytecode.validate()?;
        if self.instructions.is_empty() {
            return Err(JitPlanValidationError::BaselineEligibilityEmptyBytecode);
        }
        if self.instructions.len() as u64 != u64::from(self.bytecode.instruction_count) {
            return Err(
                JitPlanValidationError::BaselineEligibilityInstructionCountMismatch {
                    expected: self.bytecode.instruction_count,
                    actual: self.instructions.len(),
                },
            );
        }

        Ok(())
    }

    fn validate_bytecode_instruction_order(
        &self,
        bytecode_index: BytecodeIndex,
        previous_index: &mut Option<BytecodeIndex>,
    ) -> Result<(), JitPlanValidationError> {
        if !bytecode_index.is_valid() {
            return Err(
                JitPlanValidationError::BaselineEligibilityInvalidBytecodeIndex(bytecode_index),
            );
        }
        if !self.bytecode.contains(bytecode_index) {
            return Err(
                JitPlanValidationError::BaselineEligibilityBytecodeIndexOutOfRange {
                    bytecode_index,
                    start: self.bytecode.start,
                    end: self.bytecode.end,
                },
            );
        }
        if let Some(previous) = *previous_index {
            if bytecode_index <= previous {
                return Err(
                    JitPlanValidationError::BaselineEligibilityBytecodeIndexNotIncreasing {
                        previous,
                        current: bytecode_index,
                    },
                );
            }
        }
        *previous_index = Some(bytecode_index);
        Ok(())
    }

    fn validate_bytecode(&self) -> Result<(), JitPlanValidationError> {
        self.validate_bytecode_shape()?;
        let mut previous_index = None;
        for instruction in &self.instructions {
            let bytecode_index = instruction.bytecode_index;
            self.validate_bytecode_instruction_order(bytecode_index, &mut previous_index)?;

            if let Some(reason) = self.opcode_subset.rejection_reason(instruction.opcode) {
                return Err(
                    JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                        bytecode_index,
                        opcode: instruction.opcode,
                        reason,
                    },
                );
            }
        }

        Ok(())
    }

    fn validate_bytecode_with_runtime_helpers(
        &self,
        runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
    ) -> Result<(), JitPlanValidationError> {
        self.validate_bytecode_shape()?;
        let mut previous_index = None;
        for instruction in &self.instructions {
            self.validate_bytecode_instruction_order(
                instruction.bytecode_index,
                &mut previous_index,
            )?;

            if self
                .opcode_subset
                .rejection_reason(instruction.opcode)
                .is_none()
            {
                continue;
            }

            match runtime_helper_plan.proof_for_bytecode_index(instruction.bytecode_index) {
                Some(proof) if proof.contract.opcode == instruction.opcode => {}
                Some(proof) => {
                    return Err(
                        JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanOpcodeMismatch {
                            bytecode_index: instruction.bytecode_index,
                            instruction: instruction.opcode,
                            proof: proof.contract.opcode,
                        },
                    );
                }
                None => {
                    let reason = self
                        .opcode_subset
                        .rejection_reason(instruction.opcode)
                        .unwrap_or(BaselineOpcodeRejectionReason::Unsupported);
                    return Err(
                        JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                            bytecode_index: instruction.bytecode_index,
                            opcode: instruction.opcode,
                            reason,
                        },
                    );
                }
            }
        }

        Ok(())
    }

    fn validate_mixed_bytecode_sites(
        &self,
        runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    ) -> Result<Vec<BaselineMixedBytecodeSite>, JitPlanValidationError> {
        self.validate_bytecode_shape()?;
        let mut previous_index = None;
        let mut sites = Vec::with_capacity(self.instructions.len());
        for instruction in &self.instructions {
            self.validate_bytecode_instruction_order(
                instruction.bytecode_index,
                &mut previous_index,
            )?;
            let kind = self.classify_mixed_bytecode_site(*instruction, runtime_helper_plan)?;
            sites.push(BaselineMixedBytecodeSite {
                bytecode_index: instruction.bytecode_index,
                opcode: instruction.opcode,
                kind,
            });
        }
        if !sites.iter().any(|site| {
            matches!(
                site.kind,
                BaselineMixedBytecodeSiteKind::Generated
                    | BaselineMixedBytecodeSiteKind::RuntimeHelper
                    | BaselineMixedBytecodeSiteKind::JsCallHandoff
            )
        }) {
            return Err(JitPlanValidationError::BaselineEligibilityNoGeneratedOrRuntimeHelperSites);
        }

        Ok(sites)
    }

    fn classify_mixed_bytecode_site(
        &self,
        instruction: BaselineBytecodeInstruction,
        runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    ) -> Result<BaselineMixedBytecodeSiteKind, JitPlanValidationError> {
        if self.opcode_subset.supports(instruction.opcode) {
            return Ok(BaselineMixedBytecodeSiteKind::Generated);
        }

        if baseline_opcode_is_generated_js_call_handoff(instruction.opcode) {
            return Ok(BaselineMixedBytecodeSiteKind::JsCallHandoff);
        }

        if baseline_opcode_is_generated_property_handoff(instruction.opcode) {
            return Ok(BaselineMixedBytecodeSiteKind::PropertyHandoff);
        }

        if let Some(proof) = runtime_helper_plan
            .and_then(|plan| plan.proof_for_bytecode_index(instruction.bytecode_index))
        {
            if proof.contract.opcode != instruction.opcode {
                return Err(
                    JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanOpcodeMismatch {
                        bytecode_index: instruction.bytecode_index,
                        instruction: instruction.opcode,
                        proof: proof.contract.opcode,
                    },
                );
            }
            return Ok(BaselineMixedBytecodeSiteKind::RuntimeHelper);
        }

        if baseline_opcode_is_no_js_call_heap_runtime_helper(instruction.opcode) {
            return Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    reason: self
                        .opcode_subset
                        .rejection_reason(instruction.opcode)
                        .unwrap_or(BaselineOpcodeRejectionReason::Unsupported),
                },
            );
        }

        Ok(BaselineMixedBytecodeSiteKind::InterpreterFallback)
    }

    fn validate_generated_effect_contract(
        &self,
    ) -> Result<BaselineGeneratedEffectContract, JitPlanValidationError> {
        for instruction in &self.instructions {
            let effect = baseline_opcode_effect(instruction.opcode);
            if let Some(reason) = baseline_generated_effect_rejection_reason(effect) {
                return Err(
                    JitPlanValidationError::BaselineEligibilityGeneratedEffectUnsupported {
                        bytecode_index: instruction.bytecode_index,
                        opcode: instruction.opcode,
                        reason,
                    },
                );
            }
        }

        let contract = self.opcode_subset.generated_effect_contract();
        if !contract.permits_no_heap_allocation_no_runtime_call() {
            return Err(
                JitPlanValidationError::BaselineEligibilityGeneratedEffectContractInvalid {
                    opcode_subset: self.opcode_subset,
                },
            );
        }
        Ok(contract)
    }

    fn validate_generated_effect_contract_for_sites(
        &self,
        sites: &[BaselineMixedBytecodeSite],
    ) -> Result<BaselineGeneratedEffectContract, JitPlanValidationError> {
        for site in sites {
            if site.kind != BaselineMixedBytecodeSiteKind::Generated {
                continue;
            }

            let effect = baseline_opcode_effect(site.opcode);
            if let Some(reason) = baseline_generated_effect_rejection_reason(effect) {
                return Err(
                    JitPlanValidationError::BaselineEligibilityGeneratedEffectUnsupported {
                        bytecode_index: site.bytecode_index,
                        opcode: site.opcode,
                        reason,
                    },
                );
            }
        }

        let contract = self.opcode_subset.generated_effect_contract();
        if !contract.permits_no_heap_allocation_no_runtime_call() {
            return Err(
                JitPlanValidationError::BaselineEligibilityGeneratedEffectContractInvalid {
                    opcode_subset: self.opcode_subset,
                },
            );
        }
        Ok(contract)
    }

    fn validate_generated_effect_contract_with_runtime_helpers(
        &self,
        runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
    ) -> Result<BaselineGeneratedEffectContract, JitPlanValidationError> {
        for instruction in &self.instructions {
            if runtime_helper_plan
                .proof_for_bytecode_index(instruction.bytecode_index)
                .is_some()
            {
                continue;
            }

            let effect = baseline_opcode_effect(instruction.opcode);
            if let Some(reason) = baseline_generated_effect_rejection_reason(effect) {
                return Err(
                    JitPlanValidationError::BaselineEligibilityGeneratedEffectUnsupported {
                        bytecode_index: instruction.bytecode_index,
                        opcode: instruction.opcode,
                        reason,
                    },
                );
            }
        }

        let contract = self.opcode_subset.generated_effect_contract();
        if !contract.permits_no_heap_allocation_no_runtime_call() {
            return Err(
                JitPlanValidationError::BaselineEligibilityGeneratedEffectContractInvalid {
                    opcode_subset: self.opcode_subset,
                },
            );
        }
        Ok(contract)
    }

    fn validate_exception_metadata(&self) -> Result<(), JitPlanValidationError> {
        match self.exception_metadata {
            BaselineExceptionMetadataPresence::Missing => {
                Err(JitPlanValidationError::BaselineEligibilityMissingExceptionMetadata)
            }
            BaselineExceptionMetadataPresence::Present { handler_count: 0 } => Ok(()),
            BaselineExceptionMetadataPresence::Present { handler_count } => Err(
                JitPlanValidationError::BaselineEligibilityExceptionHandlersUnsupported {
                    handler_count,
                },
            ),
        }
    }
}

#[allow(dead_code)]
impl BaselineBytecodeRange {
    pub fn validate(&self) -> Result<(), JitPlanValidationError> {
        if self.instruction_count == 0 {
            return Err(JitPlanValidationError::BaselineEligibilityEmptyBytecode);
        }
        if !self.start.is_valid() || !self.end.is_valid() || self.start > self.end {
            return Err(
                JitPlanValidationError::BaselineEligibilityInvalidBytecodeRange {
                    start: self.start,
                    end: self.end,
                },
            );
        }
        Ok(())
    }

    pub fn contains(&self, bytecode_index: BytecodeIndex) -> bool {
        bytecode_index >= self.start && bytecode_index <= self.end
    }
}

#[allow(dead_code)]
impl BaselineSupportedOpcodeSubset {
    pub const fn generated_effect_contract(self) -> BaselineGeneratedEffectContract {
        match self {
            Self::P6ConstantsMovesReturnInt32Arithmetic
            | Self::P8aConstantsMovesReturnInt32ArithmeticBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOr
            | Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality
            | Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational
            | Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality
            | Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational
            | Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber
            | Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish
            | Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwise
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid
            | Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary => {
                BaselineGeneratedEffectContract {
                    opcode_subset: self,
                    summary: EffectSummary {
                        reads_heap: false,
                        writes_heap: false,
                        allocates: false,
                        may_call_js: false,
                        may_throw: false,
                        may_exit: true,
                        terminates: true,
                        reads_local_state: true,
                        writes_local_state: true,
                        reads_pinned: false,
                        writes_pinned: false,
                        fence: false,
                    },
                    may_call_runtime: false,
                    touches_gc_roots: false,
                    records_write_barrier_handoff: true,
                }
            }
        }
    }

    pub const fn rejection_reason(
        self,
        opcode: CoreOpcode,
    ) -> Option<BaselineOpcodeRejectionReason> {
        if self.supports(opcode) {
            return None;
        }
        Some(baseline_opcode_rejection_reason(opcode))
    }

    pub const fn supports(self, opcode: CoreOpcode) -> bool {
        match self {
            Self::P6ConstantsMovesReturnInt32Arithmetic => matches!(
                opcode,
                CoreOpcode::LoopHint
                    | CoreOpcode::LoadUndefined
                    | CoreOpcode::LoadNull
                    | CoreOpcode::LoadBool
                    | CoreOpcode::LoadInt32
                    | CoreOpcode::LoadCallee
                    | CoreOpcode::Move
                    | CoreOpcode::Return
                    | CoreOpcode::AddInt32
                    | CoreOpcode::SubInt32
                    | CoreOpcode::MulInt32
            ),
            Self::P8aConstantsMovesReturnInt32ArithmeticBranchNullish => {
                Self::P6ConstantsMovesReturnInt32Arithmetic.supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBranchNullish.supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOr => {
                Self::P6ConstantsMovesReturnInt32Arithmetic.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::BitOrInt32
                            | CoreOpcode::BitAndInt32
                            | CoreOpcode::BitXorInt32
                            | CoreOpcode::RightShiftInt32
                    )
            }
            Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOr.supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish.supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOr.supports(opcode)
                    || matches!(opcode, CoreOpcode::Equal | CoreOpcode::NotEqual)
            }
            Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality.supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::LessThanInt32
                            | CoreOpcode::LessEqualInt32
                            | CoreOpcode::GreaterThanInt32
                            | CoreOpcode::GreaterEqualInt32
                    )
            }
            Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOr.supports(opcode)
                    || matches!(opcode, CoreOpcode::Equal | CoreOpcode::NotEqual)
            }
            Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality
                    .supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::LessThanInt32
                            | CoreOpcode::LessEqualInt32
                            | CoreOpcode::GreaterThanInt32
                            | CoreOpcode::GreaterEqualInt32
                    )
            }
            Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullish => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullish
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::ToNumber)
            }
            Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse => {
                Self::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish
                    .supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwise => {
                Self::P6ConstantsMovesReturnInt32Arithmetic.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::BitNotInt32
                            | CoreOpcode::BitOrInt32
                            | CoreOpcode::BitXorInt32
                            | CoreOpcode::BitAndInt32
                            | CoreOpcode::LeftShiftInt32
                            | CoreOpcode::RightShiftInt32
                            | CoreOpcode::UnsignedRightShiftInt32
                    )
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwise.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::LessThanInt32
                            | CoreOpcode::LessEqualInt32
                            | CoreOpcode::GreaterThanInt32
                            | CoreOpcode::GreaterEqualInt32
                    )
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational.supports(opcode)
                    || matches!(opcode, CoreOpcode::Jump | CoreOpcode::JumpIfNotNullish)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps.supports(opcode)
                    || matches!(opcode, CoreOpcode::JumpIfFalse)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::StrictEqual
                            | CoreOpcode::StrictNotEqual
                            | CoreOpcode::LogicalNot
                    )
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::LoadDouble
                            | CoreOpcode::NegateNumber
                            | CoreOpcode::DivNumber
                            | CoreOpcode::ModNumber
                    )
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber.supports(opcode)
                    || matches!(opcode, CoreOpcode::ToNumber | CoreOpcode::Void)
            }
            Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary => {
                Self::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid.supports(opcode)
                    || matches!(
                        opcode,
                        CoreOpcode::Equal | CoreOpcode::NotEqual | CoreOpcode::LoadCallee
                    )
            }
        }
    }
}

#[allow(dead_code)]
impl BaselineRootMapRequirements {
    pub fn validate(
        &self,
        owner: CodeBlockId,
        bytecode: BaselineBytecodeRange,
    ) -> Result<BaselineRootMapRequirementsProof, JitPlanValidationError> {
        for root_map in &self.root_maps {
            root_map.validate().map_err(|error| {
                JitPlanValidationError::BaselineEligibilityRootMapInvalid {
                    root_map: root_map.id,
                    error,
                }
            })?;
            if root_map.owner != Some(owner) {
                return Err(
                    JitPlanValidationError::BaselineEligibilityRootMapOwnerMismatch {
                        root_map: root_map.id,
                        expected: owner,
                        actual: root_map.owner,
                    },
                );
            }
            if !bytecode.contains(root_map.bytecode_range_start)
                || !bytecode.contains(root_map.bytecode_range_end)
            {
                return Err(
                    JitPlanValidationError::BaselineEligibilityRootMapBytecodeRangeMismatch {
                        root_map: root_map.id,
                        start: root_map.bytecode_range_start,
                        end: root_map.bytecode_range_end,
                    },
                );
            }
        }

        let mut complete_safepoint_root_map_count = 0;
        for safepoint in &self.safepoints {
            validate_baseline_eligibility_safepoint(owner, bytecode, safepoint)?;
            if let Some(root_map_id) = safepoint.root_map {
                let root_map = self
                    .root_maps
                    .iter()
                    .find(|root_map| root_map.id == root_map_id)
                    .ok_or(
                        JitPlanValidationError::BaselineEligibilityMissingSafepointRootMap {
                            safepoint: safepoint.id,
                            root_map: root_map_id,
                        },
                    )?;
                safepoint.resolve_root_map(root_map)?;
                if root_map.complete {
                    complete_safepoint_root_map_count += 1;
                }
            } else {
                safepoint.validate()?;
            }
        }

        Ok(BaselineRootMapRequirementsProof {
            root_map_count: self.root_maps.len(),
            safepoint_count: self.safepoints.len(),
            complete_safepoint_root_map_count,
        })
    }
}

impl CompilerSafepointDescriptor {
    pub fn validate(&self) -> Result<(), JitPlanValidationError> {
        if self.id.0 == 0 {
            return Err(JitPlanValidationError::ZeroSafepointId);
        }
        if matches!(
            self.kind,
            CompilerSafepointKind::Call
                | CompilerSafepointKind::SlowPath
                | CompilerSafepointKind::WasmBoundary
        ) && !self.may_call
        {
            return Err(JitPlanValidationError::SafepointKindCallMismatch(self.id));
        }
        if self.roots.is_empty() && self.root_map.is_none() {
            return Err(JitPlanValidationError::SafepointMissingRootMap(self.id));
        }
        if self.requires_complete_root_map() && self.root_map.is_none() {
            return Err(JitPlanValidationError::SafepointMissingRootMap(self.id));
        }
        for (index, root) in self.roots.iter().enumerate() {
            if let Some(bytecode_index) = root.bytecode_index {
                if !bytecode_index.is_valid() {
                    return Err(JitPlanValidationError::SafepointInvalidBytecodeIndex(
                        self.id,
                    ));
                }
            }
            validate_compiler_root_slot_location(self.id, root)?;
            if !compiler_root_authority_is_valid(root.root_kind, root.mutation_authority) {
                return Err(JitPlanValidationError::SafepointRootAuthorityMismatch {
                    safepoint: self.id,
                    root_kind: root.root_kind,
                    authority: root.mutation_authority,
                });
            }
            if compiler_root_is_conservative(root) && root.precise {
                return Err(JitPlanValidationError::SafepointPreciseConservativeRoot {
                    safepoint: self.id,
                    slot_index: index,
                });
            }
            if self.roots[..index].iter().any(|previous| {
                previous.bytecode_index == root.bytecode_index && previous.location == root.location
            }) {
                return Err(JitPlanValidationError::SafepointDuplicateRootSlot {
                    safepoint: self.id,
                    slot_index: index,
                    location: root.location,
                });
            }
        }
        Ok(())
    }

    pub fn resolve_root_map(
        &self,
        root_map: &BytecodeRootMap,
    ) -> Result<Self, JitPlanValidationError> {
        self.validate_root_map_reference(root_map)?;

        let roots = lower_bytecode_root_map_slots(self.id, root_map.id, &root_map.slots)?;
        let resolved = Self {
            root_map: Some(root_map.id),
            roots,
            ..self.clone()
        };
        resolved.validate()?;
        Ok(resolved)
    }

    fn validate_root_map_reference(
        &self,
        root_map: &BytecodeRootMap,
    ) -> Result<(), JitPlanValidationError> {
        if self.id.0 == 0 {
            return Err(JitPlanValidationError::ZeroSafepointId);
        }
        if matches!(
            self.kind,
            CompilerSafepointKind::Call
                | CompilerSafepointKind::SlowPath
                | CompilerSafepointKind::WasmBoundary
        ) && !self.may_call
        {
            return Err(JitPlanValidationError::SafepointKindCallMismatch(self.id));
        }

        root_map
            .validate()
            .map_err(|error| JitPlanValidationError::SafepointRootMapInvalid {
                safepoint: self.id,
                root_map: root_map.id,
                error,
            })?;

        if self.root_map != Some(root_map.id) {
            return Err(JitPlanValidationError::SafepointRootMapMismatch {
                safepoint: self.id,
                expected: self.root_map,
                actual: root_map.id,
            });
        }
        if self.requires_complete_root_map() && !root_map.complete {
            return Err(JitPlanValidationError::SafepointIncompleteRootMap {
                safepoint: self.id,
                root_map: root_map.id,
            });
        }
        if self.owner != root_map.owner {
            return Err(JitPlanValidationError::SafepointRootMapOwnerMismatch {
                safepoint: self.id,
                safepoint_owner: self.owner,
                root_map_owner: root_map.owner,
            });
        }
        if let Some(bytecode_index) = self.bytecode_index {
            if !bytecode_index.is_valid() {
                return Err(JitPlanValidationError::SafepointInvalidBytecodeIndex(
                    self.id,
                ));
            }
            if bytecode_index < root_map.bytecode_range_start
                || bytecode_index > root_map.bytecode_range_end
            {
                return Err(JitPlanValidationError::SafepointRootMapBytecodeMismatch {
                    safepoint: self.id,
                    root_map: root_map.id,
                    bytecode_index,
                });
            }
        }

        Ok(())
    }

    fn requires_complete_root_map(&self) -> bool {
        self.tier == JitType::Baseline && (self.may_call || self.may_allocate)
    }

    pub fn targeted_root_plan(
        &self,
        heap: HeapId,
        bindings: &[CompilerSafepointRootBinding],
    ) -> Result<CompilerSafepointTargetedRootPlan, JitPlanValidationError> {
        self.validate()?;
        if self.requires_complete_root_map() {
            if let Some(root_map) = self.root_map {
                if self.roots.is_empty() {
                    return Err(JitPlanValidationError::SafepointUnresolvedRootMap {
                        safepoint: self.id,
                        root_map,
                    });
                }
            }
        }

        let mut seen_slots = Vec::new();
        let mut roots = TargetedRootSet::default();
        for binding in bindings {
            if seen_slots.contains(&binding.slot_index) {
                return Err(JitPlanValidationError::SafepointDuplicateRootSlotBinding {
                    safepoint: self.id,
                    slot_index: binding.slot_index,
                });
            }
            seen_slots.push(binding.slot_index);

            let slot = self.roots.get(binding.slot_index).ok_or(
                JitPlanValidationError::SafepointUnknownRootSlot {
                    safepoint: self.id,
                    slot_index: binding.slot_index,
                },
            )?;

            let target = match binding.target {
                CompilerSafepointRootTarget::Known(target) => target,
                CompilerSafepointRootTarget::NoTarget | CompilerSafepointRootTarget::Unknown => {
                    continue
                }
            };

            if compiler_root_is_conservative(slot) {
                return Err(
                    JitPlanValidationError::SafepointConservativeRootCannotBeTargeted {
                        safepoint: self.id,
                        slot_index: binding.slot_index,
                    },
                );
            }
            if !compiler_root_authority_is_valid(slot.root_kind, slot.mutation_authority) {
                return Err(JitPlanValidationError::SafepointRootAuthorityMismatch {
                    safepoint: self.id,
                    root_kind: slot.root_kind,
                    authority: slot.mutation_authority,
                });
            }

            let record = TargetedRootRecord {
                root: RootRecord {
                    id: compiler_safepoint_root_id(self.id, binding.slot_index),
                    kind: slot.root_kind,
                    heap,
                },
                target,
            };
            roots
                .register(heap, record, slot.mutation_authority)
                .map_err(
                    |error| JitPlanValidationError::SafepointTargetedRootSetInvalid {
                        safepoint: self.id,
                        error,
                    },
                )?;
        }

        Ok(CompilerSafepointTargetedRootPlan {
            safepoint: self.id,
            heap,
            roots: roots.records().to_vec(),
        })
    }
}

/// Lifecycle state for future JIT compilation jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationPlanState {
    Queued,
    Preparing,
    Compiling,
    Finalizing,
    ReadyToInstall,
    Installed,
    Cancelled,
    Failed,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitPlanStage {
    Preparing,
    Compiling,
    Compiled,
    Finalizing,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitPlanTier {
    Baseline,
    Dfg,
    Ftl,
}

/// Result category from a future compilation attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationOutcome {
    Deferred,
    Successful,
    InvalidatedBeforeInstall,
    Failed,
    Cancelled,
}

/// Compilation mode requested from the future worklist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationMode {
    Baseline,
    Dfg,
    Ftl,
    OsrEntry,
    InlineCacheStub,
    WasmFunction,
    WasmBridge,
}

/// Why a compilation request exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationRequestKind {
    TierUp(TieringTrigger),
    OsrEntry,
    RecompileAfterInvalidation,
    InlineCacheRegeneration,
    WasmCompilation,
    HostBridge,
}

/// Cancellation reason for queued or in-flight compilation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationCancellation {
    OwnerDied,
    WatchpointInvalidated,
    SupersededByNewerPlan,
    RuntimeShuttingDown,
    PolicyDisabled,
    ExplicitRequest,
}

/// Request payload submitted to a future worklist.
///
/// Owner and executable IDs are borrowed runtime identities. The worklist owns
/// scheduling and cancellation state; it must revalidate these IDs through the
/// host before publication or installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationRequest {
    pub id: CompilationPlanId,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub mode: CompilationMode,
    pub requested_tier: JitType,
    pub kind: CompilationRequestKind,
    pub key: Option<JitCompilationKeyId>,
    pub tiering_snapshot: Option<TieringSnapshot>,
    pub install_barriers: Vec<CodeInstallBarrier>,
    pub dependencies: Vec<WatchpointDependency>,
    pub priority: CompilationPriority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JitPlanValidationError {
    EmptyName,
    EmptyProvenance,
    DuplicateDescriptorName(&'static str),
    EmptyRequestKinds(&'static str),
    EmptyInstallBarriers(&'static str),
    TierModeMismatch(&'static str),
    RequestTierMismatch,
    RequestKindMismatch,
    SnapshotOwnerMismatch,
    SnapshotTierMismatch,
    DependencyWithoutBarrier,
    ProductOutcomeMismatch,
    ProductInvalidationMismatch,
    BaselineEligibilityMissingOwner,
    BaselineEligibilityDefaultOwner,
    BaselineEligibilitySnapshotOwnerMismatch {
        owner: CodeBlockId,
        snapshot_owner: CodeBlockId,
    },
    BaselineEligibilitySnapshotTierMismatch {
        from_tier: JitType,
        to_tier: JitType,
    },
    BaselineEligibilityEmptyBytecode,
    BaselineEligibilityInvalidBytecodeRange {
        start: BytecodeIndex,
        end: BytecodeIndex,
    },
    BaselineEligibilityInvalidBytecodeIndex(BytecodeIndex),
    BaselineEligibilityInstructionCountMismatch {
        expected: u32,
        actual: usize,
    },
    BaselineEligibilityBytecodeIndexOutOfRange {
        bytecode_index: BytecodeIndex,
        start: BytecodeIndex,
        end: BytecodeIndex,
    },
    BaselineEligibilityBytecodeIndexNotIncreasing {
        previous: BytecodeIndex,
        current: BytecodeIndex,
    },
    BaselineEligibilityUnsupportedOpcode {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        reason: BaselineOpcodeRejectionReason,
    },
    BaselineEligibilityUnsupportedNonCoreOpcode {
        bytecode_index: BytecodeIndex,
        opcode: Opcode,
        reason: BaselineOpcodeRejectionReason,
    },
    BaselineEligibilityNoGeneratedOrRuntimeHelperSites,
    BaselineEligibilityGeneratedEffectUnsupported {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        reason: BaselineGeneratedEffectRejectionReason,
    },
    BaselineEligibilityGeneratedEffectContractInvalid {
        opcode_subset: BaselineSupportedOpcodeSubset,
    },
    BaselineGeneratedRuntimeHelperPlanInvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedRuntimeHelperPlanDuplicateProof {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedRuntimeHelperPlanContractDoesNotCallRuntimeHelper {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperPlanContractDoesNotTouchGcRoots {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperPlanMayThrowMismatch {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        proof_may_throw: bool,
        contract_may_throw: bool,
    },
    BaselineGeneratedRuntimeHelperPlanMissingNoGcExitReentry {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperPlanMissingRootMap {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        safepoint: CompilerSafepointId,
    },
    BaselineGeneratedRuntimeHelperPlanOpcodeMismatch {
        bytecode_index: BytecodeIndex,
        instruction: CoreOpcode,
        proof: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperPlanCodeBlockDerivationMismatch {
        expected_proof_count: usize,
        actual_proof_count: usize,
        first_mismatch: Option<usize>,
        bytecode_snapshot_matches: Option<bool>,
    },
    BaselineGeneratedPropertyHandoffPlanInvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanDuplicateSite {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanDuplicateBytecodeCache {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanUnsupportedOpcode {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedPropertyHandoffPlanCacheKindMismatch {
        bytecode_index: BytecodeIndex,
        expected: InlineCacheKind,
        actual: InlineCacheKind,
    },
    BaselineGeneratedPropertyHandoffPlanAccessMismatch {
        bytecode_index: BytecodeIndex,
        expected: PropertyAccessType,
        actual: PropertyAccessType,
    },
    BaselineGeneratedPropertyHandoffPlanPropertyCacheKindMismatch {
        bytecode_index: BytecodeIndex,
        expected: PropertyCacheKind,
        actual: PropertyCacheKind,
    },
    BaselineGeneratedPropertyHandoffPlanFallbackMismatch {
        bytecode_index: BytecodeIndex,
        expected: InlineCacheFallbackSemantics,
        actual: InlineCacheFallbackSemantics,
    },
    BaselineGeneratedPropertyHandoffPlanMissingBaseRegister {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedPropertyHandoffPlanMissingPropertyIdentifier {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedPropertyHandoffPlanBytecodeIndexMismatch {
        expected: BytecodeIndex,
        actual: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanBaseMismatch {
        bytecode_index: BytecodeIndex,
        expected: VirtualRegister,
        actual: Option<VirtualRegister>,
    },
    BaselineGeneratedPropertyHandoffPlanPropertyKeyMismatch {
        bytecode_index: BytecodeIndex,
        expected: PropertyCacheKey,
        actual: PropertyCacheKey,
    },
    BaselineGeneratedPropertyHandoffPlanMissingBytecodeCache {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanBytecodeCacheStateMismatch {
        bytecode_index: BytecodeIndex,
        expected: BytecodeInlineCacheState,
        actual: BytecodeInlineCacheState,
    },
    BaselineGeneratedPropertyHandoffPlanMalformedBytecodeCache {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffOwnerMismatch {
        bytecode_index: BytecodeIndex,
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffSlotMismatch {
        bytecode_index: BytecodeIndex,
        expected: InlineCacheSlotId,
        actual: InlineCacheSlotId,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffBytecodeIndexMismatch {
        bytecode_index: BytecodeIndex,
        expected: u32,
        actual: u32,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffCacheKindMismatch {
        bytecode_index: BytecodeIndex,
        expected: InlineCacheKind,
        actual: InlineCacheKind,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffMissKindMismatch {
        bytecode_index: BytecodeIndex,
        expected: InlineCacheMissKind,
        actual: InlineCacheMissKind,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffFallbackMismatch {
        bytecode_index: BytecodeIndex,
        expected: InlineCacheFallbackSemantics,
        actual: InlineCacheFallbackSemantics,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffBoundaryMismatch {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffCallLinkMismatch {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffPreservesOperandRegistersMismatch {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanMissHandoffInvalid {
        bytecode_index: BytecodeIndex,
        error: InlineCacheValidationError,
    },
    BaselineGeneratedPropertyHandoffPlanMalformedSite {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
        expected_site_count: usize,
        actual_site_count: usize,
        first_mismatch: Option<usize>,
        bytecode_snapshot_matches: Option<bool>,
    },
    BaselineGeneratedJsCallNativeExitPlanInvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedJsCallNativeExitPlanDuplicateSite {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedJsCallNativeExitPlanTooManySites {
        capacity: usize,
        actual: usize,
    },
    BaselineGeneratedJsCallNativeExitPlanUnsupportedOpcode {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedJsCallNativeExitPlanOperandShape {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        operand_index: usize,
        error: OperandAccessError,
    },
    BaselineGeneratedJsCallNativeExitPlanOperandCountMismatch {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        expected: usize,
        actual: usize,
    },
    BaselineGeneratedJsCallNativeExitPlanMalformedSite {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedJsCallNativeExitPlanCodeBlockDerivationMismatch {
        expected_site_count: usize,
        actual_site_count: usize,
        first_mismatch: Option<usize>,
        bytecode_snapshot_matches: Option<bool>,
    },
    BaselineGeneratedOwnerContinuationMapInvalidBytecodeIndex {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedOwnerContinuationMapDuplicateLabel {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedOwnerContinuationMapDuplicateSite {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedOwnerContinuationMapMissingLabel {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedOwnerContinuationMapUnsupportedOpcode {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedOwnerContinuationMapMalformedLabel {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedOwnerContinuationMapMalformedSite {
        bytecode_index: BytecodeIndex,
    },
    BaselineGeneratedOwnerContinuationMapCodeBlockDerivationMismatch {
        expected_label_count: usize,
        actual_label_count: usize,
        first_label_mismatch: Option<usize>,
        expected_site_count: usize,
        actual_site_count: usize,
        first_site_mismatch: Option<usize>,
        bytecode_snapshot_matches: Option<bool>,
    },
    BaselineGeneratedRuntimeHelperDerivationRejectedOpcode {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingAllocationFlag {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        safepoint: CompilerSafepointId,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingSourceRegister {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingStringLiteralOperand {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingCodeBlockStringLiteral {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        identifier_index: u32,
    },
    BaselineGeneratedRuntimeHelperDerivationAmbiguousRootMaps {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingDestinationSlot {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        root_map: BytecodeRootMapId,
        register: VirtualRegister,
    },
    BaselineGeneratedRuntimeHelperDerivationMissingSourceSlot {
        bytecode_index: BytecodeIndex,
        opcode: CoreOpcode,
        root_map: BytecodeRootMapId,
        register: VirtualRegister,
    },
    BaselineEligibilityInstructionDecodeFailed {
        bytecode_index: BytecodeIndex,
        error: InstructionDecodeError,
    },
    BaselineEligibilityMissingExceptionMetadata,
    BaselineEligibilityExceptionHandlersUnsupported {
        handler_count: u32,
    },
    BaselineEligibilityRootMapInvalid {
        root_map: BytecodeRootMapId,
        error: BytecodeRootMapValidationError,
    },
    BaselineEligibilityRootMapOwnerMismatch {
        root_map: BytecodeRootMapId,
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    BaselineEligibilityRootMapBytecodeRangeMismatch {
        root_map: BytecodeRootMapId,
        start: BytecodeIndex,
        end: BytecodeIndex,
    },
    BaselineEligibilitySafepointOwnerMismatch {
        safepoint: CompilerSafepointId,
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    BaselineEligibilitySafepointMissingBytecodeIndex(CompilerSafepointId),
    BaselineEligibilitySafepointBytecodeIndexOutOfRange {
        safepoint: CompilerSafepointId,
        bytecode_index: BytecodeIndex,
        start: BytecodeIndex,
        end: BytecodeIndex,
    },
    BaselineEligibilityMissingSafepointRootMap {
        safepoint: CompilerSafepointId,
        root_map: BytecodeRootMapId,
    },
    ZeroSafepointId,
    SafepointMissingRootMap(CompilerSafepointId),
    SafepointKindCallMismatch(CompilerSafepointId),
    SafepointInvalidBytecodeIndex(CompilerSafepointId),
    SafepointRootMapInvalid {
        safepoint: CompilerSafepointId,
        root_map: BytecodeRootMapId,
        error: BytecodeRootMapValidationError,
    },
    SafepointRootMapMismatch {
        safepoint: CompilerSafepointId,
        expected: Option<BytecodeRootMapId>,
        actual: BytecodeRootMapId,
    },
    SafepointIncompleteRootMap {
        safepoint: CompilerSafepointId,
        root_map: BytecodeRootMapId,
    },
    SafepointUnresolvedRootMap {
        safepoint: CompilerSafepointId,
        root_map: BytecodeRootMapId,
    },
    SafepointRootMapOwnerMismatch {
        safepoint: CompilerSafepointId,
        safepoint_owner: Option<CodeBlockId>,
        root_map_owner: Option<CodeBlockId>,
    },
    SafepointRootMapBytecodeMismatch {
        safepoint: CompilerSafepointId,
        root_map: BytecodeRootMapId,
        bytecode_index: BytecodeIndex,
    },
    SafepointDuplicateRootSlot {
        safepoint: CompilerSafepointId,
        slot_index: usize,
        location: CompilerRootSlotLocation,
    },
    SafepointInvalidVirtualRegister {
        safepoint: CompilerSafepointId,
        register: VirtualRegister,
    },
    SafepointPreciseConservativeRoot {
        safepoint: CompilerSafepointId,
        slot_index: usize,
    },
    SafepointConservativeRootCannotBeTargeted {
        safepoint: CompilerSafepointId,
        slot_index: usize,
    },
    SafepointRootAuthorityMismatch {
        safepoint: CompilerSafepointId,
        root_kind: RootKind,
        authority: RootSetMutationAuthority,
    },
    SafepointUnknownRootSlot {
        safepoint: CompilerSafepointId,
        slot_index: usize,
    },
    SafepointDuplicateRootSlotBinding {
        safepoint: CompilerSafepointId,
        slot_index: usize,
    },
    SafepointTargetedRootSetInvalid {
        safepoint: CompilerSafepointId,
        error: RootSetSemanticError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationRequestBuilder {
    request: CompilationRequest,
}

impl CompilationRequestBuilder {
    pub fn new(id: CompilationPlanId, mode: CompilationMode, kind: CompilationRequestKind) -> Self {
        Self {
            request: CompilationRequest {
                id,
                owner: None,
                executable: None,
                mode,
                requested_tier: tier_for_mode(mode),
                kind,
                key: None,
                tiering_snapshot: None,
                install_barriers: Vec::new(),
                dependencies: Vec::new(),
                priority: CompilationPriority::Background,
            },
        }
    }

    pub fn owner(mut self, owner: CodeBlockId) -> Self {
        self.request.owner = Some(owner);
        self
    }

    pub fn executable(mut self, executable: ExecutableId) -> Self {
        self.request.executable = Some(executable);
        self
    }

    pub fn requested_tier(mut self, tier: JitType) -> Self {
        self.request.requested_tier = tier;
        self
    }

    pub fn key(mut self, key: JitCompilationKeyId) -> Self {
        self.request.key = Some(key);
        self
    }

    pub fn tiering_snapshot(mut self, snapshot: TieringSnapshot) -> Self {
        self.request.tiering_snapshot = Some(snapshot);
        self
    }

    pub fn install_barrier(mut self, barrier: CodeInstallBarrier) -> Self {
        self.request.install_barriers.push(barrier);
        self
    }

    pub fn dependency(mut self, dependency: WatchpointDependency) -> Self {
        self.request.dependencies.push(dependency);
        self
    }

    pub fn priority(mut self, priority: CompilationPriority) -> Self {
        self.request.priority = priority;
        self
    }

    pub fn build(self) -> Result<CompilationRequest, JitPlanValidationError> {
        self.request.validate()?;
        Ok(self.request)
    }
}

impl CompilationRequest {
    pub fn builder(
        id: CompilationPlanId,
        mode: CompilationMode,
        kind: CompilationRequestKind,
    ) -> CompilationRequestBuilder {
        CompilationRequestBuilder::new(id, mode, kind)
    }

    pub fn validate(&self) -> Result<(), JitPlanValidationError> {
        if self.requested_tier != tier_for_mode(self.mode) {
            return Err(JitPlanValidationError::RequestTierMismatch);
        }

        if !request_kind_matches_mode(self.kind, self.mode) {
            return Err(JitPlanValidationError::RequestKindMismatch);
        }

        if let Some(snapshot) = self.tiering_snapshot {
            if self.owner != Some(snapshot.owner) {
                return Err(JitPlanValidationError::SnapshotOwnerMismatch);
            }

            if self.requested_tier != snapshot.to_tier {
                return Err(JitPlanValidationError::SnapshotTierMismatch);
            }
        }

        if !self.dependencies.is_empty()
            && !self
                .install_barriers
                .contains(&CodeInstallBarrier::WatchpointsStillValid)
        {
            return Err(JitPlanValidationError::DependencyWithoutBarrier);
        }

        Ok(())
    }
}

/// Returns a deterministic dependency-respecting order for data-only requests.
///
/// Tier prerequisites are ordered before their consumers: baseline requests come
/// before optimizing tiers, OSR is grouped with DFG, and FTL is last. Within the
/// same tier group, request priority and stable plan identity decide order.
pub fn order_compilation_requests(
    requests: &[CompilationRequest],
) -> Result<Vec<CompilationPlanId>, JitPlanValidationError> {
    for request in requests {
        request.validate()?;
    }

    let mut ordered: Vec<&CompilationRequest> = requests.iter().collect();
    ordered.sort_by_key(|request| {
        (
            compilation_mode_dependency_rank(request.mode),
            compilation_priority_rank(request.priority),
            request.id,
        )
    });

    Ok(ordered.into_iter().map(|request| request.id).collect())
}

const fn compilation_mode_dependency_rank(mode: CompilationMode) -> u8 {
    match mode {
        CompilationMode::Baseline => 0,
        CompilationMode::InlineCacheStub => 1,
        CompilationMode::Dfg | CompilationMode::OsrEntry => 2,
        CompilationMode::Ftl => 3,
        CompilationMode::WasmFunction | CompilationMode::WasmBridge => 4,
    }
}

const fn compilation_priority_rank(priority: CompilationPriority) -> u8 {
    match priority {
        CompilationPriority::HotLoop => 0,
        CompilationPriority::Interactive => 1,
        CompilationPriority::WasmStreaming => 2,
        CompilationPriority::Background => 3,
        CompilationPriority::Maintenance => 4,
    }
}

/// Selects install barriers required by a request from the static plan schema.
pub fn install_barriers_for_request(
    request: &CompilationRequest,
    registry: JitPlanDescriptorRegistry,
) -> Result<&'static [CodeInstallBarrier], JitPlanValidationError> {
    request.validate()?;
    registry.validate()?;

    registry
        .descriptors()
        .iter()
        .find(|descriptor| {
            descriptor.mode == request.mode && descriptor.request_kinds.contains(&request.kind)
        })
        .map(|descriptor| descriptor.install_barriers)
        .ok_or(JitPlanValidationError::RequestKindMismatch)
}

/// Scheduling priority without selecting a concrete queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationPriority {
    Interactive,
    HotLoop,
    Background,
    WasmStreaming,
    Maintenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitWorklistPlanState {
    NotKnown,
    Compiling,
    Compiled,
    Ready,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitWorklistThreadState {
    Idle,
    Running,
    Suspended,
    TemporaryStopRequested,
    ShuttingDown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitWorklistDescriptor {
    pub id: JitWorklistId,
    pub active_threads: u32,
    pub total_load: u32,
    pub queued_baseline: Vec<CompilationPlanId>,
    pub queued_dfg: Vec<CompilationPlanId>,
    pub queued_ftl: Vec<CompilationPlanId>,
    pub ready_plans: Vec<CompilationPlanId>,
    /// The worklist owns queue membership and ready-plan publication. VM and
    /// GC code may inspect or cancel through explicit host callbacks only.
    pub thread_state: JitWorklistThreadState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitPlanLifecycle {
    pub plan: CompilationPlanId,
    pub tier: JitPlanTier,
    pub stage: JitPlanStage,
    pub worklist_state: JitWorklistPlanState,
    pub code_size_bytes: Option<usize>,
    pub main_thread_finalization_required: bool,
    pub safepoint_keeps_dependencies_live: bool,
}

/// Finalization artifact returned to the main thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationProduct {
    pub plan: CompilationPlanId,
    pub outcome: CompilationOutcome,
    pub code: Option<JitCodeArtifact>,
    pub replacement_for: Option<JitCodeId>,
    pub invalidation: Option<CodeInvalidationReason>,
}

impl CompilationProduct {
    pub fn validate(&self) -> Result<(), JitPlanValidationError> {
        match self.outcome {
            CompilationOutcome::Successful => {
                if self.code.is_none() {
                    return Err(JitPlanValidationError::ProductOutcomeMismatch);
                }
                if self.invalidation.is_some() {
                    return Err(JitPlanValidationError::ProductInvalidationMismatch);
                }
            }
            CompilationOutcome::InvalidatedBeforeInstall | CompilationOutcome::Failed => {
                if self.invalidation.is_none() {
                    return Err(JitPlanValidationError::ProductInvalidationMismatch);
                }
            }
            CompilationOutcome::Deferred | CompilationOutcome::Cancelled => {
                if self.code.is_some() {
                    return Err(JitPlanValidationError::ProductOutcomeMismatch);
                }
            }
        }

        Ok(())
    }
}

/// Host callbacks required by future JIT plan execution and installation.
pub trait JitPlanHost {
    fn can_compile_concurrently(&self) -> bool;
    fn trace_plan_edges(&mut self, plan: CompilationPlanId);
    fn invalidate_plan(&mut self, plan: CompilationPlanId);
    fn plan_owner_is_live(&self, owner: CodeBlockId) -> bool;
    fn install_compilation_product(&mut self, product: CompilationProduct) -> CompilationOutcome;
}

/// Abstract compilation job without implementation strategy.
pub trait CompilationPlan {
    fn id(&self) -> CompilationPlanId;
    fn request(&self) -> &CompilationRequest;
    fn requested_tier(&self) -> JitType;
    fn state(&self) -> CompilationPlanState;
    fn watchpoint_dependencies(&self) -> &[WatchpointDependency];
    fn product(&self) -> Option<&CompilationProduct>;
    fn cancel(&mut self, host: &mut dyn JitPlanHost);
}

/// Component that owns a published immutable JIT plan schema.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum JitPlanSchemaOwner {
    #[default]
    JitPlanRegistry,
    BaselineJit,
    DfgJit,
    FtlJit,
    InlineCacheRegistry,
}

/// Authority allowed to replace the static plan registry snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum JitPlanRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

/// Provenance for a static JIT plan schema entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct JitPlanSchemaProvenance {
    pub source: &'static str,
    pub generated_by: &'static str,
}

/// Immutable shape for a family of future compilation plans.
///
/// This table records plan identity, ownership, and required side-data classes.
/// Runtime scheduling, cancellation, compilation, and installation stay outside
/// this schema layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticJitPlanDescriptor {
    pub name: &'static str,
    pub tier: JitPlanTier,
    pub mode: CompilationMode,
    pub request_kinds: &'static [CompilationRequestKind],
    pub install_barriers: &'static [CodeInstallBarrier],
    pub owner: JitPlanSchemaOwner,
    pub mutation_authority: JitPlanRegistryMutationAuthority,
    pub provenance: JitPlanSchemaProvenance,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JitPlanDescriptorRegistry {
    pub descriptors: &'static [StaticJitPlanDescriptor],
}

impl JitPlanDescriptorRegistry {
    pub const fn new(descriptors: &'static [StaticJitPlanDescriptor]) -> Self {
        Self { descriptors }
    }

    pub const fn descriptors(self) -> &'static [StaticJitPlanDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_name(self, name: &str) -> Option<&'static StaticJitPlanDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> Result<(), JitPlanValidationError> {
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            descriptor.validate()?;

            if self.descriptors[index + 1..]
                .iter()
                .any(|other| other.name == descriptor.name)
            {
                return Err(JitPlanValidationError::DuplicateDescriptorName(
                    descriptor.name,
                ));
            }
        }

        Ok(())
    }
}

impl StaticJitPlanDescriptor {
    pub fn validate(&self) -> Result<(), JitPlanValidationError> {
        if self.name.is_empty() {
            return Err(JitPlanValidationError::EmptyName);
        }
        if self.provenance.source.is_empty() || self.provenance.generated_by.is_empty() {
            return Err(JitPlanValidationError::EmptyProvenance);
        }
        if self.request_kinds.is_empty() {
            return Err(JitPlanValidationError::EmptyRequestKinds(self.name));
        }
        if self.install_barriers.is_empty() {
            return Err(JitPlanValidationError::EmptyInstallBarriers(self.name));
        }
        if !tier_matches_mode(self.tier, self.mode) {
            return Err(JitPlanValidationError::TierModeMismatch(self.name));
        }

        Ok(())
    }
}

const fn tier_for_mode(mode: CompilationMode) -> JitType {
    match mode {
        CompilationMode::Baseline | CompilationMode::InlineCacheStub => JitType::Baseline,
        CompilationMode::Dfg | CompilationMode::OsrEntry => JitType::Dfg,
        CompilationMode::Ftl => JitType::Ftl,
        CompilationMode::WasmFunction | CompilationMode::WasmBridge => JitType::WasmOmg,
    }
}

#[allow(dead_code)]
fn validate_baseline_eligibility_safepoint(
    owner: CodeBlockId,
    bytecode: BaselineBytecodeRange,
    safepoint: &CompilerSafepointDescriptor,
) -> Result<(), JitPlanValidationError> {
    if safepoint.owner != Some(owner) {
        return Err(
            JitPlanValidationError::BaselineEligibilitySafepointOwnerMismatch {
                safepoint: safepoint.id,
                expected: owner,
                actual: safepoint.owner,
            },
        );
    }

    let bytecode_index = safepoint.bytecode_index.ok_or(
        JitPlanValidationError::BaselineEligibilitySafepointMissingBytecodeIndex(safepoint.id),
    )?;
    if !bytecode_index.is_valid() {
        return Err(JitPlanValidationError::SafepointInvalidBytecodeIndex(
            safepoint.id,
        ));
    }
    if !bytecode.contains(bytecode_index) {
        return Err(
            JitPlanValidationError::BaselineEligibilitySafepointBytecodeIndexOutOfRange {
                safepoint: safepoint.id,
                bytecode_index,
                start: bytecode.start,
                end: bytecode.end,
            },
        );
    }

    for root in &safepoint.roots {
        if let Some(root_bytecode_index) = root.bytecode_index {
            if !root_bytecode_index.is_valid() {
                return Err(JitPlanValidationError::SafepointInvalidBytecodeIndex(
                    safepoint.id,
                ));
            }
            if !bytecode.contains(root_bytecode_index) {
                return Err(
                    JitPlanValidationError::BaselineEligibilitySafepointBytecodeIndexOutOfRange {
                        safepoint: safepoint.id,
                        bytecode_index: root_bytecode_index,
                        start: bytecode.start,
                        end: bytecode.end,
                    },
                );
            }
        }
    }

    Ok(())
}

fn collect_baseline_bytecode_instructions<'a>(
    decoded_instructions: impl IntoIterator<
        Item = Result<DecodedInstruction<'a>, InstructionDecodeError>,
    >,
) -> Result<Vec<BaselineBytecodeInstruction>, JitPlanValidationError> {
    let mut instructions = Vec::new();
    for (ordinal, decoded) in decoded_instructions.into_iter().enumerate() {
        let decoded = decoded.map_err(|error| {
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            }
        })?;
        let opcode = CoreOpcode::from_opcode(decoded.opcode).ok_or(
            JitPlanValidationError::BaselineEligibilityUnsupportedNonCoreOpcode {
                bytecode_index: decoded.bytecode_index,
                opcode: decoded.opcode,
                reason: BaselineOpcodeRejectionReason::Unsupported,
            },
        )?;
        instructions.push(BaselineBytecodeInstruction {
            bytecode_index: decoded.bytecode_index,
            opcode,
        });
    }
    Ok(instructions)
}

fn baseline_bytecode_snapshot_fingerprint_from_code_block(
    code_block: &CodeBlock,
) -> Result<BaselineBytecodeSnapshotFingerprint, JitPlanValidationError> {
    let mut instruction_hasher = BaselineSnapshotHasher::new(0x6a09_e667_f3bc_c909);
    let mut instruction_count = 0_u32;
    for (ordinal, decoded) in code_block
        .unlinked()
        .instructions()
        .decoded_instructions()
        .enumerate()
    {
        let decoded = decoded.map_err(|error| {
            JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                bytecode_index: BytecodeIndex::from_offset(ordinal as u32),
                error,
            }
        })?;
        write_decoded_instruction_fingerprint(&mut instruction_hasher, ordinal as u32, decoded);
        instruction_count = instruction_count.saturating_add(1);
    }

    let mut side_table_hasher = BaselineSnapshotHasher::new(0xbb67_ae85_84ca_a73b);
    write_root_map_fingerprint(&mut side_table_hasher, &code_block.side_tables().root_maps);
    write_exception_metadata_fingerprint(
        &mut side_table_hasher,
        baseline_exception_metadata_from_code_block(code_block),
    );
    write_code_block_string_literal_table_fingerprint(&mut side_table_hasher, code_block);
    write_property_inline_cache_table_fingerprint(
        &mut side_table_hasher,
        &code_block.side_tables().inline_caches.property_accesses,
    );

    Ok(BaselineBytecodeSnapshotFingerprint {
        instruction_count,
        instruction_stream_hash: instruction_hasher.finish(),
        side_table_hash: side_table_hasher.finish(),
    })
}

fn baseline_bytecode_snapshot_fingerprint_from_record(
    record: &BaselineBytecodeEligibilityRecord,
) -> BaselineBytecodeSnapshotFingerprint {
    let mut instruction_hasher = BaselineSnapshotHasher::new(0x6a09_e667_f3bc_c909);
    for (ordinal, instruction) in record.instructions.iter().enumerate() {
        instruction_hasher.write_u32(0x100);
        instruction_hasher.write_u32(ordinal as u32);
        instruction_hasher.write_u32(instruction.bytecode_index.as_bits());
        write_opcode_fingerprint(&mut instruction_hasher, instruction.opcode.opcode());
        instruction_hasher.write_u32(0);
    }

    let mut side_table_hasher = BaselineSnapshotHasher::new(0xbb67_ae85_84ca_a73b);
    write_root_map_fingerprint(
        &mut side_table_hasher,
        &record.root_map_requirements.root_maps,
    );
    write_exception_metadata_fingerprint(&mut side_table_hasher, record.exception_metadata);

    BaselineBytecodeSnapshotFingerprint {
        instruction_count: record.instructions.len().min(u32::MAX as usize) as u32,
        instruction_stream_hash: instruction_hasher.finish(),
        side_table_hash: side_table_hasher.finish(),
    }
}

fn validate_baseline_generated_runtime_helper_proofs(
    proofs: &mut [BaselineGeneratedRuntimeHelperProof],
) -> Result<(), JitPlanValidationError> {
    for proof in proofs.iter() {
        if !proof.bytecode_index.is_valid() {
            return Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanInvalidBytecodeIndex {
                    bytecode_index: proof.bytecode_index,
                },
            );
        }
        validate_baseline_generated_runtime_helper_proof(proof)?;
    }

    proofs.sort_by_key(|proof| proof.bytecode_index);
    for duplicate in proofs.windows(2) {
        if duplicate[0].bytecode_index == duplicate[1].bytecode_index {
            return Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanDuplicateProof {
                    bytecode_index: duplicate[0].bytecode_index,
                },
            );
        }
    }

    Ok(())
}

fn validate_baseline_generated_property_handoff_sites(
    sites: &mut [BaselineGeneratedPropertyHandoffSite],
) -> Result<(), JitPlanValidationError> {
    for site in sites.iter() {
        if !site.bytecode_index.is_valid() {
            return Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanInvalidBytecodeIndex {
                    bytecode_index: site.bytecode_index,
                },
            );
        }
        validate_baseline_generated_property_handoff_site_metadata(site)?;
    }

    sites.sort_by_key(|site| site.bytecode_index);
    for duplicate in sites.windows(2) {
        if duplicate[0].bytecode_index == duplicate[1].bytecode_index {
            return Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanDuplicateSite {
                    bytecode_index: duplicate[0].bytecode_index,
                },
            );
        }
    }

    Ok(())
}

fn validate_baseline_generated_js_call_native_exit_sites(
    sites: &mut [BaselineGeneratedJsCallNativeExitSite],
) -> Result<(), JitPlanValidationError> {
    if sites.len() > BASELINE_GENERATED_JS_CALL_NATIVE_EXIT_PLAN_SITE_CAPACITY {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanTooManySites {
                capacity: BASELINE_GENERATED_JS_CALL_NATIVE_EXIT_PLAN_SITE_CAPACITY,
                actual: sites.len(),
            },
        );
    }

    for site in sites.iter() {
        validate_baseline_generated_js_call_native_exit_site_metadata(site)?;
    }

    sites.sort_by_key(|site| site.bytecode_index);
    for duplicate in sites.windows(2) {
        if duplicate[0].bytecode_index == duplicate[1].bytecode_index {
            return Err(
                JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanDuplicateSite {
                    bytecode_index: duplicate[0].bytecode_index,
                },
            );
        }
    }

    Ok(())
}

fn validate_baseline_generated_owner_continuation_labels(
    labels: &mut [BaselineGeneratedOwnerBytecodeLabel],
) -> Result<(), JitPlanValidationError> {
    for label in labels.iter() {
        if label.owner == CodeBlockId::default() || !label.bytecode_index.is_valid() {
            return Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMalformedLabel {
                    bytecode_index: label.bytecode_index,
                },
            );
        }
        if label
            .next_bytecode_index
            .is_some_and(|next| !next.is_valid())
        {
            return Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapInvalidBytecodeIndex {
                    bytecode_index: label.next_bytecode_index.unwrap_or(BytecodeIndex::INVALID),
                },
            );
        }
    }

    labels.sort_by_key(|label| label.bytecode_index);
    for duplicate in labels.windows(2) {
        if duplicate[0].bytecode_index == duplicate[1].bytecode_index {
            return Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapDuplicateLabel {
                    bytecode_index: duplicate[0].bytecode_index,
                },
            );
        }
    }

    Ok(())
}

fn validate_baseline_generated_owner_continuation_sites(
    labels: &[BaselineGeneratedOwnerBytecodeLabel],
    call_sites: &mut [BaselineGeneratedOwnerContinuationSite],
) -> Result<(), JitPlanValidationError> {
    for site in call_sites.iter() {
        validate_baseline_generated_owner_continuation_site_metadata(site)?;
        let label = labels
            .binary_search_by_key(&site.call_bytecode_index, |label| label.bytecode_index)
            .ok()
            .and_then(|index| labels.get(index))
            .ok_or(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMissingLabel {
                    bytecode_index: site.call_bytecode_index,
                },
            )?;
        if label.owner != site.owner
            || label.opcode != site.opcode
            || label.next_bytecode_index != site.resume_bytecode_index
        {
            return Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMalformedSite {
                    bytecode_index: site.call_bytecode_index,
                },
            );
        }
    }

    call_sites.sort_by_key(|site| site.call_bytecode_index);
    for duplicate in call_sites.windows(2) {
        if duplicate[0].call_bytecode_index == duplicate[1].call_bytecode_index {
            return Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapDuplicateSite {
                    bytecode_index: duplicate[0].call_bytecode_index,
                },
            );
        }
    }

    Ok(())
}

pub(crate) fn validate_baseline_generated_owner_continuation_site_metadata(
    site: &BaselineGeneratedOwnerContinuationSite,
) -> Result<(), JitPlanValidationError> {
    if site.owner == CodeBlockId::default()
        || !site.call_bytecode_index.is_valid()
        || site.argument_count_including_this == 0
        || site
            .resume_bytecode_index
            .is_some_and(|resume| !resume.is_valid())
    {
        return Err(
            JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMalformedSite {
                bytecode_index: site.call_bytecode_index,
            },
        );
    }
    let Some(expected_kind) = owner_continuation_kind_for_opcode(site.opcode) else {
        return Err(
            JitPlanValidationError::BaselineGeneratedOwnerContinuationMapUnsupportedOpcode {
                bytecode_index: site.call_bytecode_index,
                opcode: site.opcode,
            },
        );
    };
    if site.kind != expected_kind {
        return Err(
            JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMalformedSite {
                bytecode_index: site.call_bytecode_index,
            },
        );
    }
    match (site.kind, site.result_profile) {
        (BaselineGeneratedOwnerContinuationKind::Call, Some(profile))
            if profile.profile_slot.0 != 0
                && profile.bytecode_index == site.call_bytecode_index
                && profile.checkpoint == Checkpoint::NONE
                && profile.bucket_kind == ValueProfileBucketKind::Sample
                && profile.storage_generation != ValueProfileJitStorageGeneration::default()
                && profile.value_profile_offset != 0
                && profile.metadata_table_base_address != 0
                && profile.raw_bucket_address != 0
                && profile.raw_bucket_bytes > 0 => {}
        (BaselineGeneratedOwnerContinuationKind::Construct, None) => {}
        _ => {
            return Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapMalformedSite {
                    bytecode_index: site.call_bytecode_index,
                },
            )
        }
    }
    Ok(())
}

pub(crate) fn validate_baseline_generated_js_call_native_exit_site_metadata(
    site: &BaselineGeneratedJsCallNativeExitSite,
) -> Result<(), JitPlanValidationError> {
    if site.owner == CodeBlockId::default()
        || !site.bytecode_index.is_valid()
        || !baseline_opcode_is_generated_js_call_handoff(site.opcode)
        || site.provided_argument_count as usize != site.argument_registers.len()
        || !site.requires_no_gc_exit_reentry
        || !site.may_throw
    {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanMalformedSite {
                bytecode_index: site.bytecode_index,
            },
        );
    }
    if (matches!(site.opcode, CoreOpcode::Call | CoreOpcode::Construct)
        && site.this_register.is_some())
        || (site.opcode == CoreOpcode::CallWithThis && site.this_register.is_none())
    {
        return Err(
            JitPlanValidationError::BaselineGeneratedJsCallNativeExitPlanMalformedSite {
                bytecode_index: site.bytecode_index,
            },
        );
    }
    Ok(())
}

pub(crate) fn validate_baseline_generated_property_handoff_site_metadata(
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Result<(), JitPlanValidationError> {
    let bytecode_index = site.bytecode_index;
    let expected = property_handoff_shape_for_opcode(site.opcode).map_err(|_| {
        JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanUnsupportedOpcode {
            bytecode_index,
            opcode: site.opcode,
        }
    })?;
    if site.cache_kind != expected.cache_kind {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCacheKindMismatch {
                bytecode_index,
                expected: expected.cache_kind,
                actual: site.cache_kind,
            },
        );
    }
    if site.access != expected.access {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanAccessMismatch {
                bytecode_index,
                expected: expected.access,
                actual: site.access,
            },
        );
    }
    if site.property_cache_kind != expected.property_cache_kind {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanPropertyCacheKindMismatch {
                bytecode_index,
                expected: expected.property_cache_kind,
                actual: site.property_cache_kind,
            },
        );
    }
    if !matches!(
        (site.opcode, site.property_key),
        (
            CoreOpcode::GetByName
                | CoreOpcode::GetGlobalObjectProperty
                | CoreOpcode::GetLength
                | CoreOpcode::PutByName
                | CoreOpcode::InById,
            PropertyCacheKey::Key(_)
        ) | (
            CoreOpcode::PutGlobalObjectProperty,
            PropertyCacheKey::Key(_)
        ) | (
            CoreOpcode::GetByValue | CoreOpcode::PutByValue | CoreOpcode::InByVal,
            PropertyCacheKey::RuntimeValue(_)
        )
    ) {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMalformedSite {
                bytecode_index,
            },
        );
    }
    if site.fallback != InlineCacheFallbackSemantics::SlowPathLookup {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanFallbackMismatch {
                bytecode_index,
                expected: InlineCacheFallbackSemantics::SlowPathLookup,
                actual: site.fallback,
            },
        );
    }
    validate_baseline_generated_property_handoff_miss_descriptor(site)?;
    if !site.requires_no_gc_exit_reentry || !site.may_throw {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMalformedSite {
                bytecode_index,
            },
        );
    }
    Ok(())
}

fn validate_baseline_generated_property_handoff_miss_descriptor(
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Result<(), JitPlanValidationError> {
    let bytecode_index = site.bytecode_index;
    let descriptor = &site.cold_miss_handoff;
    if descriptor.owner != site.owner {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffOwnerMismatch {
                bytecode_index,
                expected: site.owner,
                actual: descriptor.owner,
            },
        );
    }
    if descriptor.slot != site.slot {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffSlotMismatch {
                bytecode_index,
                expected: site.slot,
                actual: descriptor.slot,
            },
        );
    }
    if descriptor.bytecode_index != bytecode_index.offset() {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffBytecodeIndexMismatch {
                bytecode_index,
                expected: bytecode_index.offset(),
                actual: descriptor.bytecode_index,
            },
        );
    }
    if descriptor.cache_kind != site.cache_kind {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffCacheKindMismatch {
                bytecode_index,
                expected: site.cache_kind,
                actual: descriptor.cache_kind,
            },
        );
    }
    if descriptor.miss_kind != InlineCacheMissKind::Cold {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffMissKindMismatch {
                bytecode_index,
                expected: InlineCacheMissKind::Cold,
                actual: descriptor.miss_kind,
            },
        );
    }
    if descriptor.fallback != InlineCacheFallbackSemantics::SlowPathLookup {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffFallbackMismatch {
                bytecode_index,
                expected: InlineCacheFallbackSemantics::SlowPathLookup,
                actual: descriptor.fallback,
            },
        );
    }
    if descriptor.boundary.is_some() {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffBoundaryMismatch {
                bytecode_index,
            },
        );
    }
    if descriptor.call_link.is_some() {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffCallLinkMismatch {
                bytecode_index,
            },
        );
    }
    if !descriptor.preserves_operand_registers {
        return Err(
            JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffPreservesOperandRegistersMismatch {
                bytecode_index,
            },
        );
    }
    descriptor.validate().map_err(|error| {
        JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffInvalid {
            bytecode_index,
            error,
        }
    })
}

fn validate_baseline_generated_runtime_helper_proof(
    proof: &BaselineGeneratedRuntimeHelperProof,
) -> Result<(), JitPlanValidationError> {
    let bytecode_index = proof.bytecode_index;
    let boundary = proof.proof;
    let opcode = boundary.contract.opcode;
    if !boundary.contract.effects.calls_runtime_helper {
        return Err(
            JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanContractDoesNotCallRuntimeHelper {
                bytecode_index,
                opcode,
            },
        );
    }
    if !boundary.contract.effects.touches_gc_roots {
        return Err(
            JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanContractDoesNotTouchGcRoots {
                bytecode_index,
                opcode,
            },
        );
    }
    if boundary.may_throw != boundary.contract.effects.may_throw {
        return Err(
            JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMayThrowMismatch {
                bytecode_index,
                opcode,
                proof_may_throw: boundary.may_throw,
                contract_may_throw: boundary.contract.effects.may_throw,
            },
        );
    }
    if boundary.contract.requirements.no_gc_exit_reentry && !boundary.no_gc_exit_reentry {
        return Err(
            JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMissingNoGcExitReentry {
                bytecode_index,
                opcode,
            },
        );
    }
    if boundary.contract.requirements.complete_safepoint_root_map && boundary.root_map.is_none() {
        return Err(
            JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMissingRootMap {
                bytecode_index,
                opcode,
                safepoint: boundary.safepoint,
            },
        );
    }
    Ok(())
}

fn write_decoded_instruction_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    ordinal: u32,
    instruction: DecodedInstruction<'_>,
) {
    hasher.write_u32(0x200);
    hasher.write_u32(ordinal);
    hasher.write_u32(instruction.bytecode_index.as_bits());
    write_opcode_fingerprint(hasher, instruction.opcode);
    write_operand_width_fingerprint(hasher, instruction.width);
    write_decoded_source_fingerprint(hasher, instruction.source);
    match instruction.schema {
        Some(schema) => {
            hasher.write_u32(1);
            write_opcode_fingerprint(hasher, schema.opcode);
            hasher.write_u32(schema.operand_start);
            hasher.write_u32(u32::from(schema.operand_count));
        }
        None => hasher.write_u32(0),
    }
    hasher.write_u32(instruction.operands.len().min(u32::MAX as usize) as u32);
    for operand in instruction.operands {
        write_operand_fingerprint(hasher, *operand);
    }
}

fn write_opcode_fingerprint(hasher: &mut BaselineSnapshotHasher, opcode: Opcode) {
    match opcode {
        Opcode::Reserved => hasher.write_u32(0),
        Opcode::Wide16Prefix => hasher.write_u32(1),
        Opcode::Wide32Prefix => hasher.write_u32(2),
        Opcode::Generated(id) => {
            hasher.write_u32(3);
            hasher.write_u32(u32::from(id.generated_index()));
        }
        Opcode::RuntimeExtension(id) => {
            hasher.write_u32(4);
            hasher.write_u32(u32::from(id.generated_index()));
        }
    }
}

fn write_operand_width_fingerprint(hasher: &mut BaselineSnapshotHasher, width: OperandWidth) {
    hasher.write_u32(match width {
        OperandWidth::Narrow => 1,
        OperandWidth::Wide16 => 2,
        OperandWidth::Wide32 => 4,
    });
}

fn write_decoded_source_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    source: DecodedInstructionSource,
) {
    hasher.write_u32(match source {
        DecodedInstructionSource::Declaration => 1,
        DecodedInstructionSource::TypedPlaceholder => 2,
    });
}

fn write_operand_fingerprint(hasher: &mut BaselineSnapshotHasher, operand: Operand) {
    match operand {
        Operand::Register(register) => {
            hasher.write_u32(1);
            hasher.write_i32(register.raw());
        }
        Operand::EncodedRegister(encoded) => {
            hasher.write_u32(2);
            hasher.write_i32(encoded.register.raw());
            hasher.write_u32(match encoded.width {
                crate::bytecode::RegisterOperandWidth::Narrow8 => 1,
                crate::bytecode::RegisterOperandWidth::Wide16 => 2,
                crate::bytecode::RegisterOperandWidth::Wide32 => 4,
            });
        }
        Operand::SignedImmediate(value) => {
            hasher.write_u32(3);
            hasher.write_i32(value);
        }
        Operand::UnsignedImmediate(value) => {
            hasher.write_u32(4);
            hasher.write_u32(value);
        }
        Operand::ConstantPoolIndex(value) => {
            hasher.write_u32(5);
            hasher.write_u32(value);
        }
        Operand::ConstantCell(cell) => {
            hasher.write_u32(6);
            hasher.write_u32(cell.0);
        }
        Operand::IdentifierIndex(value) => {
            hasher.write_u32(7);
            hasher.write_u32(value);
        }
        Operand::IdentifierSet(set) => {
            hasher.write_u32(8);
            hasher.write_u32(set.0);
        }
        Operand::FunctionDeclIndex(value) => {
            hasher.write_u32(9);
            hasher.write_u32(value);
        }
        Operand::FunctionExprIndex(value) => {
            hasher.write_u32(10);
            hasher.write_u32(value);
        }
        Operand::BytecodeIndex(index) => {
            hasher.write_u32(11);
            hasher.write_u32(index.as_bits());
        }
        Operand::Label(label) => {
            hasher.write_u32(12);
            hasher.write_u32(label.0);
        }
        Operand::JumpTableIndex(value) => {
            hasher.write_u32(13);
            hasher.write_u32(value);
        }
        Operand::MetadataIndex(value) => {
            hasher.write_u32(14);
            hasher.write_u32(value);
        }
        Operand::InlineCacheIndex(value) => {
            hasher.write_u32(15);
            hasher.write_u32(value);
        }
        Operand::ProfileIndex(value) => {
            hasher.write_u32(16);
            hasher.write_u32(value);
        }
        Operand::Checkpoint(checkpoint) => {
            hasher.write_u32(17);
            hasher.write_u32(u32::from(checkpoint.0));
        }
        Operand::CallSite(call_site) => {
            hasher.write_u32(18);
            hasher.write_u32(call_site.0);
        }
        Operand::LinkTimeConstant(constant) => {
            hasher.write_u32(19);
            write_link_time_constant_fingerprint(hasher, constant);
        }
        Operand::RuntimeType(runtime_type) => {
            hasher.write_u32(20);
            hasher.write_u32(runtime_type.0);
        }
        Operand::SchemaReserved(value) => {
            hasher.write_u32(21);
            hasher.write_u32(value);
        }
    }
}

fn write_link_time_constant_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    constant: crate::bytecode::LinkTimeConstant,
) {
    match constant {
        crate::bytecode::LinkTimeConstant::CopyDataProperties => hasher.write_u32(1),
        crate::bytecode::LinkTimeConstant::IteratorSymbol => hasher.write_u32(2),
        crate::bytecode::LinkTimeConstant::AsyncIteratorSymbol => hasher.write_u32(3),
        crate::bytecode::LinkTimeConstant::PromiseConstructor => hasher.write_u32(4),
        crate::bytecode::LinkTimeConstant::OpaqueGenerated(value) => {
            hasher.write_u32(5);
            hasher.write_u32(value);
        }
    }
}

fn write_root_map_fingerprint(hasher: &mut BaselineSnapshotHasher, root_maps: &[BytecodeRootMap]) {
    hasher.write_u32(root_maps.len().min(u32::MAX as usize) as u32);
    for root_map in root_maps {
        hasher.write_u32(root_map.id.0);
        write_optional_code_block_id_fingerprint(hasher, root_map.owner);
        hasher.write_u32(root_map.bytecode_range_start.as_bits());
        hasher.write_u32(root_map.bytecode_range_end.as_bits());
        hasher.write_bool(root_map.complete);
        hasher.write_u32(root_map.slots.len().min(u32::MAX as usize) as u32);
        for slot in &root_map.slots {
            write_root_slot_fingerprint(hasher, *slot);
        }
    }
}

fn write_property_inline_cache_table_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    property_accesses: &[PropertyInlineCache],
) {
    hasher.write_u32(property_accesses.len().min(u32::MAX as usize) as u32);
    for cache in property_accesses {
        hasher.write_u32(cache.bytecode_index.as_bits());
        hasher.write_u32(match cache.access {
            PropertyAccessType::GetById => 1,
            PropertyAccessType::GetByIdWithThis => 2,
            PropertyAccessType::GetByIdDirect => 3,
            PropertyAccessType::TryGetById => 4,
            PropertyAccessType::GetByVal => 5,
            PropertyAccessType::GetByValWithThis => 6,
            PropertyAccessType::PutByIdStrict => 7,
            PropertyAccessType::PutByIdSloppy => 8,
            PropertyAccessType::PutByIdDirectStrict => 9,
            PropertyAccessType::PutByIdDirectSloppy => 10,
            PropertyAccessType::PutByValStrict => 11,
            PropertyAccessType::PutByValSloppy => 12,
            PropertyAccessType::PutByValDirectStrict => 13,
            PropertyAccessType::PutByValDirectSloppy => 14,
            PropertyAccessType::DefinePrivateNameByVal => 15,
            PropertyAccessType::DefinePrivateNameById => 16,
            PropertyAccessType::SetPrivateNameByVal => 17,
            PropertyAccessType::SetPrivateNameById => 18,
            PropertyAccessType::InById => 19,
            PropertyAccessType::InByVal => 20,
            PropertyAccessType::HasPrivateName => 21,
            PropertyAccessType::HasPrivateBrand => 22,
            PropertyAccessType::InstanceOf => 23,
            PropertyAccessType::DeleteByIdStrict => 24,
            PropertyAccessType::DeleteByIdSloppy => 25,
            PropertyAccessType::DeleteByValStrict => 26,
            PropertyAccessType::DeleteByValSloppy => 27,
            PropertyAccessType::GetPrivateName => 28,
            PropertyAccessType::GetPrivateNameById => 29,
            PropertyAccessType::CheckPrivateBrand => 30,
            PropertyAccessType::SetPrivateBrand => 31,
        });
        hasher.write_u32(match cache.kind {
            PropertyCacheKind::GetById => 1,
            PropertyCacheKind::GetByIdWithThis => 2,
            PropertyCacheKind::GetByIdDirect => 3,
            PropertyCacheKind::TryGetById => 4,
            PropertyCacheKind::GetByVal => 5,
            PropertyCacheKind::GetByValWithThis => 6,
            PropertyCacheKind::PutById => 7,
            PropertyCacheKind::PutByIdDirect => 8,
            PropertyCacheKind::PutByVal => 9,
            PropertyCacheKind::InById => 10,
            PropertyCacheKind::InByVal => 11,
            PropertyCacheKind::DeleteById => 12,
            PropertyCacheKind::DeleteByVal => 13,
            PropertyCacheKind::InstanceOf => 14,
            PropertyCacheKind::PrivateName => 15,
            PropertyCacheKind::PrivateBrand => 16,
        });
        write_optional_register_fingerprint(hasher, cache.base);
        write_property_cache_key_fingerprint(hasher, cache.property);
        hasher.write_bool(cache.get_by_id.is_some());
        hasher.write_bool(cache.put_by_id.is_some());
    }
}

fn write_optional_register_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    register: Option<VirtualRegister>,
) {
    match register {
        Some(register) => {
            hasher.write_u32(1);
            hasher.write_i32(register.raw());
        }
        None => hasher.write_u32(0),
    }
}

fn write_property_cache_key_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    key: PropertyCacheKey,
) {
    match key {
        PropertyCacheKey::None => hasher.write_u32(0),
        PropertyCacheKey::Key(key) => {
            hasher.write_u32(1);
            write_property_key_fingerprint(hasher, key);
        }
        PropertyCacheKey::RuntimeValue(register) => {
            hasher.write_u32(2);
            hasher.write_i32(register.raw());
        }
    }
}

fn write_property_key_fingerprint(hasher: &mut BaselineSnapshotHasher, key: PropertyKey) {
    match key {
        PropertyKey::String(identifier) => {
            hasher.write_u32(1);
            hasher.write_u32(identifier.atom().table_slot());
            hasher.write_u32(identifier.domain().table().vm_slot());
        }
        PropertyKey::Symbol(symbol) => {
            hasher.write_u32(2);
            hasher.write_u32(symbol.table_slot());
        }
        PropertyKey::PrivateName(name) => {
            hasher.write_u32(3);
            hasher.write_u32(name.uid().table_slot());
        }
        PropertyKey::Index(index) => {
            hasher.write_u32(4);
            hasher.write_u32(index.get());
        }
    }
}

fn write_root_slot_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    slot: BytecodeRootSlotDescriptor,
) {
    hasher.write_u32(slot.bytecode_index.as_bits());
    hasher.write_u32(match slot.kind {
        BytecodeRootSlotKind::VirtualRegister => 1,
        BytecodeRootSlotKind::Argument => 2,
        BytecodeRootSlotKind::Constant => 3,
        BytecodeRootSlotKind::MetadataSlot => 4,
        BytecodeRootSlotKind::InlineCache => 5,
        BytecodeRootSlotKind::ValueProfile => 6,
        BytecodeRootSlotKind::CallSite => 7,
    });
    write_root_slot_storage_fingerprint(hasher, slot.storage);
    write_root_kind_fingerprint(hasher, slot.root_kind);
    write_root_authority_fingerprint(hasher, slot.mutation_authority);
    hasher.write_bool(slot.precise);
}

fn write_root_slot_storage_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    storage: BytecodeRootSlotStorage,
) {
    match storage {
        BytecodeRootSlotStorage::Register(register) => {
            hasher.write_u32(1);
            hasher.write_i32(register.raw());
        }
        BytecodeRootSlotStorage::RuntimeSlot(slot) => {
            hasher.write_u32(2);
            hasher.write_u32(slot.0);
        }
        BytecodeRootSlotStorage::ConstantIndex(index) => {
            hasher.write_u32(3);
            hasher.write_u32(index);
        }
        BytecodeRootSlotStorage::CallSite(call_site) => {
            hasher.write_u32(4);
            hasher.write_u32(call_site);
        }
    }
}

fn write_root_kind_fingerprint(hasher: &mut BaselineSnapshotHasher, kind: RootKind) {
    hasher.write_u32(match kind {
        RootKind::Handle => 1,
        RootKind::ExplicitRoot => 2,
        RootKind::VMRegister => 3,
        RootKind::Stack => 4,
        RootKind::JitCode => 5,
        RootKind::Host => 6,
    });
}

fn write_root_authority_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    authority: RootSetMutationAuthority,
) {
    hasher.write_u32(match authority {
        RootSetMutationAuthority::HandleScope => 1,
        RootSetMutationAuthority::ExplicitRootRegistry => 2,
        RootSetMutationAuthority::VmRegisterFile => 3,
        RootSetMutationAuthority::JitCodeRegistry => 4,
        RootSetMutationAuthority::HostIntegration => 5,
        RootSetMutationAuthority::ConservativeScanner => 6,
    });
}

fn write_optional_code_block_id_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    owner: Option<CodeBlockId>,
) {
    match owner {
        Some(owner) => {
            hasher.write_u32(1);
            hasher.write_u32(owner.0 .0);
        }
        None => hasher.write_u32(0),
    }
}

fn write_exception_metadata_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    metadata: BaselineExceptionMetadataPresence,
) {
    match metadata {
        BaselineExceptionMetadataPresence::Missing => hasher.write_u32(0),
        BaselineExceptionMetadataPresence::Present { handler_count } => {
            hasher.write_u32(1);
            hasher.write_u32(handler_count);
        }
    }
}

fn write_code_block_string_literal_table_fingerprint(
    hasher: &mut BaselineSnapshotHasher,
    code_block: &CodeBlock,
) {
    let entries = code_block.unlinked().string_literals().entries();
    hasher.write_u32(entries.len().min(u32::MAX as usize) as u32);
    for entry in entries {
        hasher.write_u32(entry.identifier_index);
        write_string_fingerprint(hasher, &entry.text);
    }
}

fn write_string_fingerprint(hasher: &mut BaselineSnapshotHasher, text: &str) {
    let bytes = text.as_bytes();
    hasher.write_u32(bytes.len().min(u32::MAX as usize) as u32);
    for chunk in bytes.chunks(4) {
        let mut value = 0_u32;
        for (offset, byte) in chunk.iter().enumerate() {
            value |= u32::from(*byte) << ((offset * 8) as u32);
        }
        hasher.write_u32(value);
    }
}

#[derive(Clone, Copy, Debug)]
struct BaselineSnapshotHasher {
    low: u64,
    high: u64,
}

impl BaselineSnapshotHasher {
    const fn new(seed: u64) -> Self {
        Self {
            low: seed,
            high: seed ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn write_bool(&mut self, value: bool) {
        self.write_u64(if value { 1 } else { 0 });
    }

    fn write_i32(&mut self, value: i32) {
        self.write_u32(value as u32);
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.low = splitmix64(self.low ^ value);
        self.high = splitmix64(
            self.high
                .wrapping_add(value.rotate_left(32))
                .wrapping_add(self.low),
        );
    }

    fn finish(self) -> u128 {
        let low = splitmix64(self.low ^ 0xff51_afd7_ed55_8ccd);
        let high = splitmix64(self.high ^ low.rotate_left(17));
        (u128::from(high) << 64) | u128::from(low)
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn baseline_bytecode_range_from_instructions(
    instructions: &[BaselineBytecodeInstruction],
) -> BaselineBytecodeRange {
    let start = instructions
        .first()
        .map(|instruction| instruction.bytecode_index)
        .unwrap_or_else(|| BytecodeIndex::from_offset(0));
    let end = instructions
        .last()
        .map(|instruction| instruction.bytecode_index)
        .unwrap_or(start);
    BaselineBytecodeRange {
        start,
        end,
        instruction_count: instructions.len() as u32,
    }
}

fn baseline_exception_metadata_from_code_block(
    code_block: &CodeBlock,
) -> BaselineExceptionMetadataPresence {
    let linked = code_block.side_tables();
    let unlinked = code_block.unlinked().side_tables();
    let handler_count = linked
        .handlers
        .len()
        .saturating_add(linked.exception_handlers.handlers.len())
        .saturating_add(unlinked.handlers.len())
        .saturating_add(unlinked.exception_handlers.handlers.len());

    BaselineExceptionMetadataPresence::Present {
        handler_count: handler_count.min(u32::MAX as usize) as u32,
    }
}

const fn baseline_opcode_effect(opcode: CoreOpcode) -> BaselineOpcodeEffect {
    if matches!(opcode, CoreOpcode::LoopHint) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(false, false, false, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: false,
        };
    }
    if matches!(
        opcode,
        CoreOpcode::LoadUndefined
            | CoreOpcode::LoadNull
            | CoreOpcode::LoadBool
            | CoreOpcode::LoadInt32
            | CoreOpcode::LoadDouble
    ) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(false, true, false, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(opcode, CoreOpcode::Move) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, false, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(opcode, CoreOpcode::LoadCallee) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(opcode, CoreOpcode::Return) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, false, false, true),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: false,
        };
    }
    if matches!(
        opcode,
        CoreOpcode::AddInt32 | CoreOpcode::SubInt32 | CoreOpcode::MulInt32
    ) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(opcode, CoreOpcode::BitNotInt32) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(
        opcode,
        CoreOpcode::BitOrInt32
            | CoreOpcode::BitXorInt32
            | CoreOpcode::BitAndInt32
            | CoreOpcode::LeftShiftInt32
            | CoreOpcode::RightShiftInt32
            | CoreOpcode::UnsignedRightShiftInt32
    ) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(
        opcode,
        CoreOpcode::LessThanInt32
            | CoreOpcode::LessEqualInt32
            | CoreOpcode::GreaterThanInt32
            | CoreOpcode::GreaterEqualInt32
    ) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(opcode, CoreOpcode::Jump) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(false, false, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: false,
        };
    }
    if matches!(
        opcode,
        CoreOpcode::JumpIfFalse | CoreOpcode::JumpIfNotNullish
    ) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, false, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: false,
        };
    }
    if matches!(
        opcode,
        CoreOpcode::StrictEqual
            | CoreOpcode::StrictNotEqual
            | CoreOpcode::Equal
            | CoreOpcode::NotEqual
            | CoreOpcode::LogicalNot
            | CoreOpcode::ToNumber
            | CoreOpcode::NegateNumber
            | CoreOpcode::DivNumber
            | CoreOpcode::ModNumber
    ) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, true, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if matches!(opcode, CoreOpcode::Void) {
        return BaselineOpcodeEffect {
            opcode,
            summary: local_effect_summary(true, true, false, false),
            may_call_runtime: false,
            touches_gc_roots: false,
            records_write_barrier_handoff: true,
        };
    }
    if baseline_opcode_is_call(opcode) {
        return BaselineOpcodeEffect {
            opcode,
            summary: EffectSummary::for_call(),
            may_call_runtime: true,
            touches_gc_roots: true,
            records_write_barrier_handoff: false,
        };
    }
    if baseline_opcode_is_allocation_or_object(opcode) {
        return BaselineOpcodeEffect {
            opcode,
            summary: EffectSummary {
                reads_heap: true,
                writes_heap: true,
                allocates: true,
                may_call_js: false,
                may_throw: true,
                may_exit: true,
                terminates: false,
                reads_local_state: true,
                writes_local_state: true,
                reads_pinned: false,
                writes_pinned: false,
                fence: true,
            },
            may_call_runtime: true,
            touches_gc_roots: true,
            records_write_barrier_handoff: true,
        };
    }
    if baseline_opcode_is_property_access(opcode) {
        return BaselineOpcodeEffect {
            opcode,
            summary: EffectSummary {
                reads_heap: true,
                writes_heap: true,
                allocates: false,
                may_call_js: false,
                may_throw: true,
                may_exit: true,
                terminates: false,
                reads_local_state: true,
                writes_local_state: true,
                reads_pinned: false,
                writes_pinned: false,
                fence: true,
            },
            may_call_runtime: true,
            touches_gc_roots: true,
            records_write_barrier_handoff: true,
        };
    }
    if baseline_opcode_is_exception(opcode) {
        return BaselineOpcodeEffect {
            opcode,
            summary: EffectSummary {
                reads_heap: false,
                writes_heap: false,
                allocates: false,
                may_call_js: false,
                may_throw: true,
                may_exit: true,
                terminates: false,
                reads_local_state: true,
                writes_local_state: false,
                reads_pinned: false,
                writes_pinned: false,
                fence: true,
            },
            may_call_runtime: false,
            touches_gc_roots: true,
            records_write_barrier_handoff: false,
        };
    }

    BaselineOpcodeEffect {
        opcode,
        summary: EffectSummary {
            reads_heap: true,
            writes_heap: true,
            allocates: false,
            may_call_js: false,
            may_throw: true,
            may_exit: true,
            terminates: false,
            reads_local_state: true,
            writes_local_state: true,
            reads_pinned: false,
            writes_pinned: false,
            fence: true,
        },
        may_call_runtime: true,
        touches_gc_roots: true,
        records_write_barrier_handoff: true,
    }
}

const fn local_effect_summary(
    reads_local_state: bool,
    writes_local_state: bool,
    may_exit: bool,
    terminates: bool,
) -> EffectSummary {
    EffectSummary {
        reads_heap: false,
        writes_heap: false,
        allocates: false,
        may_call_js: false,
        may_throw: false,
        may_exit,
        terminates,
        reads_local_state,
        writes_local_state,
        reads_pinned: false,
        writes_pinned: false,
        fence: false,
    }
}

const fn baseline_generated_effect_rejection_reason(
    effect: BaselineOpcodeEffect,
) -> Option<BaselineGeneratedEffectRejectionReason> {
    if effect.summary.allocates {
        return Some(BaselineGeneratedEffectRejectionReason::MayAllocate);
    }
    if effect.may_call_runtime {
        return Some(BaselineGeneratedEffectRejectionReason::MayCallRuntime);
    }
    if effect.summary.may_call_js {
        return Some(BaselineGeneratedEffectRejectionReason::MayCallJs);
    }
    if effect.summary.may_throw {
        return Some(BaselineGeneratedEffectRejectionReason::MayThrow);
    }
    if effect.summary.writes_heap {
        return Some(BaselineGeneratedEffectRejectionReason::WritesHeap);
    }
    if effect.touches_gc_roots {
        return Some(BaselineGeneratedEffectRejectionReason::TouchesGcRoots);
    }
    None
}

#[allow(dead_code)]
pub const fn baseline_generated_runtime_boundary_contract(
    opcode: CoreOpcode,
) -> Result<BaselineGeneratedRuntimeBoundaryContract, BaselineGeneratedRuntimeBoundaryRejectionReason>
{
    if matches!(opcode, CoreOpcode::Throw) {
        let effect = baseline_opcode_effect(opcode);
        return Ok(BaselineGeneratedRuntimeBoundaryContract {
            opcode,
            category: BaselineGeneratedRuntimeBoundaryCategory::ExceptionThrow,
            summary: effect.summary,
            effects: BaselineGeneratedRuntimeBoundaryEffects {
                calls_runtime_helper: true,
                allocates: false,
                may_throw: true,
                writes_heap: false,
                touches_gc_roots: true,
                records_write_barrier_handoff: false,
            },
            requirements: BaselineGeneratedRuntimeBoundaryRequirements {
                complete_safepoint_root_map: true,
                no_gc_exit_reentry: true,
            },
        });
    }
    if baseline_opcode_is_property_access(opcode) {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess);
    }
    if baseline_opcode_is_call(opcode) {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::JsCallOrConstructor);
    }
    if !matches!(opcode, CoreOpcode::LoadFunction)
        && baseline_opcode_is_function_or_constructor(opcode)
    {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::FunctionOrConstructor);
    }
    if baseline_opcode_is_exception(opcode) {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::Exception);
    }
    if matches!(opcode, CoreOpcode::PowNumber) {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PowNumber);
    }
    if !baseline_opcode_is_no_js_call_heap_runtime_helper(opcode) {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::Unsupported);
    }

    let effect = baseline_opcode_effect(opcode);
    if effect.summary.may_call_js {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::JsCallOrConstructor);
    }
    if !effect.may_call_runtime {
        return Err(BaselineGeneratedRuntimeBoundaryRejectionReason::Unsupported);
    }

    Ok(BaselineGeneratedRuntimeBoundaryContract {
        opcode,
        category: BaselineGeneratedRuntimeBoundaryCategory::NoJsCallHeapRuntimeHelper,
        summary: effect.summary,
        effects: BaselineGeneratedRuntimeBoundaryEffects {
            calls_runtime_helper: effect.may_call_runtime,
            allocates: effect.summary.allocates,
            may_throw: effect.summary.may_throw,
            writes_heap: effect.summary.writes_heap,
            touches_gc_roots: effect.touches_gc_roots,
            records_write_barrier_handoff: effect.records_write_barrier_handoff,
        },
        requirements: BaselineGeneratedRuntimeBoundaryRequirements {
            complete_safepoint_root_map: true,
            no_gc_exit_reentry: true,
        },
    })
}

#[allow(dead_code)]
const fn baseline_opcode_rejection_reason(opcode: CoreOpcode) -> BaselineOpcodeRejectionReason {
    if baseline_opcode_is_call(opcode) {
        BaselineOpcodeRejectionReason::Call
    } else if baseline_opcode_is_allocation_or_object(opcode) {
        BaselineOpcodeRejectionReason::AllocationOrObject
    } else if baseline_opcode_is_property_access(opcode) {
        BaselineOpcodeRejectionReason::PropertyAccess
    } else if baseline_opcode_is_exception(opcode) {
        BaselineOpcodeRejectionReason::Exception
    } else {
        BaselineOpcodeRejectionReason::Unsupported
    }
}

#[allow(dead_code)]
const fn baseline_opcode_is_call(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::Call
            | CoreOpcode::CallWithThis
            | CoreOpcode::CallDirect
            | CoreOpcode::Construct
            | CoreOpcode::ConstructSuper
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_generated_js_call_handoff(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::Call | CoreOpcode::CallWithThis | CoreOpcode::Construct
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_generated_property_handoff(opcode: CoreOpcode) -> bool {
    matches!(
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
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_no_js_call_heap_runtime_helper(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::LoadString
            | CoreOpcode::LoadBigInt
            | CoreOpcode::LoadFunction
            | CoreOpcode::InitializeGlobalLexical
            | CoreOpcode::TypeOf
            | CoreOpcode::NewObject
            | CoreOpcode::NewArray
            | CoreOpcode::LoadCapture
            | CoreOpcode::NewClosureCell
            | CoreOpcode::GetClosureCell
            | CoreOpcode::PutClosureCell
            | CoreOpcode::ArrayAppend
            | CoreOpcode::ForInKeys
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_function_or_constructor(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::LoadFunction
            | CoreOpcode::LoadObjectConstructor
            | CoreOpcode::LoadArrayConstructor
            | CoreOpcode::LoadFunctionConstructor
            | CoreOpcode::LoadArrayBufferConstructor
            | CoreOpcode::LoadUint8ArrayConstructor
            | CoreOpcode::LoadDataViewConstructor
            | CoreOpcode::LoadProxyConstructor
            | CoreOpcode::LoadStringConstructor
            | CoreOpcode::LoadNumberConstructor
            | CoreOpcode::LoadBooleanConstructor
            | CoreOpcode::LoadErrorConstructor
            | CoreOpcode::LoadTypeErrorConstructor
            | CoreOpcode::LoadMapConstructor
            | CoreOpcode::LoadSetConstructor
            | CoreOpcode::LoadWeakMapConstructor
            | CoreOpcode::LoadWeakSetConstructor
            | CoreOpcode::LoadRegExpConstructor
            | CoreOpcode::LoadPromiseConstructor
            | CoreOpcode::LoadDateConstructor
            | CoreOpcode::LoadBigIntConstructor
            | CoreOpcode::LoadSymbolConstructor
            | CoreOpcode::SetFunctionSuper
            | CoreOpcode::SetDefaultDerivedConstructor
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_allocation_or_object(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::LoadFunction
            | CoreOpcode::LoadString
            | CoreOpcode::LoadBigInt
            | CoreOpcode::TypeOf
            | CoreOpcode::LoadObjectConstructor
            | CoreOpcode::LoadArrayConstructor
            | CoreOpcode::LoadFunctionConstructor
            | CoreOpcode::LoadMathObject
            | CoreOpcode::LoadJsonObject
            | CoreOpcode::LoadReflectObject
            | CoreOpcode::LoadArrayBufferConstructor
            | CoreOpcode::LoadUint8ArrayConstructor
            | CoreOpcode::LoadDataViewConstructor
            | CoreOpcode::LoadProxyConstructor
            | CoreOpcode::LoadStringConstructor
            | CoreOpcode::LoadNumberConstructor
            | CoreOpcode::LoadBooleanConstructor
            | CoreOpcode::LoadErrorConstructor
            | CoreOpcode::LoadTypeErrorConstructor
            | CoreOpcode::LoadMapConstructor
            | CoreOpcode::LoadSetConstructor
            | CoreOpcode::LoadWeakMapConstructor
            | CoreOpcode::LoadWeakSetConstructor
            | CoreOpcode::LoadRegExpConstructor
            | CoreOpcode::LoadPromiseConstructor
            | CoreOpcode::LoadDateConstructor
            | CoreOpcode::LoadBigIntConstructor
            | CoreOpcode::LoadSymbolConstructor
            | CoreOpcode::NewObject
            | CoreOpcode::NewArray
            | CoreOpcode::NewRegExp
            | CoreOpcode::NewClosureCell
            | CoreOpcode::SetPrototype
            | CoreOpcode::SetFunctionSuper
            | CoreOpcode::SetDefaultDerivedConstructor
            | CoreOpcode::AddInstanceField
            | CoreOpcode::AddInstanceFieldByValue
            | CoreOpcode::ArrayAppend
            | CoreOpcode::ArrayAppendSpread
            | CoreOpcode::ArrayAppendRest
            | CoreOpcode::ForInKeys
            | CoreOpcode::CopyObjectRest
            | CoreOpcode::CreateRestParameter
            | CoreOpcode::CreateArgumentsObject
            | CoreOpcode::DefineGetter
            | CoreOpcode::DefineSetter
            | CoreOpcode::DefineGetterByValue
            | CoreOpcode::DefineSetterByValue
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_property_access(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::ArrayLength
            | CoreOpcode::GetSuperByName
            | CoreOpcode::GetByName
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::PutByName
            | CoreOpcode::PutGlobalObjectProperty
            | CoreOpcode::DeleteByName
            | CoreOpcode::GetByValue
            | CoreOpcode::PutByValue
            | CoreOpcode::DeleteByValue
            | CoreOpcode::InById
            | CoreOpcode::InByVal
            | CoreOpcode::GetByIndex
            | CoreOpcode::PutByIndex
            | CoreOpcode::GetLength
    )
}

#[allow(dead_code)]
const fn baseline_opcode_is_exception(opcode: CoreOpcode) -> bool {
    matches!(opcode, CoreOpcode::Throw | CoreOpcode::TakeException)
}

const fn compiler_root_authority_is_valid(
    root_kind: RootKind,
    authority: RootSetMutationAuthority,
) -> bool {
    matches!(
        (root_kind, authority),
        (
            RootKind::VMRegister,
            RootSetMutationAuthority::VmRegisterFile
        ) | (RootKind::JitCode, RootSetMutationAuthority::JitCodeRegistry)
            | (
                RootKind::ExplicitRoot,
                RootSetMutationAuthority::ExplicitRootRegistry
            )
            | (
                RootKind::Stack,
                RootSetMutationAuthority::ConservativeScanner
            )
    )
}

fn lower_bytecode_root_map_slots(
    safepoint: CompilerSafepointId,
    root_map: BytecodeRootMapId,
    slots: &[BytecodeRootSlotDescriptor],
) -> Result<Vec<CompilerRootSlotDescriptor>, JitPlanValidationError> {
    let mut roots = Vec::with_capacity(slots.len());
    for slot in slots {
        let root = lower_bytecode_root_slot(safepoint, root_map, *slot)?;
        if roots.iter().any(|previous: &CompilerRootSlotDescriptor| {
            previous.bytecode_index == root.bytecode_index && previous.location == root.location
        }) {
            return Err(JitPlanValidationError::SafepointDuplicateRootSlot {
                safepoint,
                slot_index: roots.len(),
                location: root.location,
            });
        }
        roots.push(root);
    }
    Ok(roots)
}

fn lower_bytecode_root_slot(
    safepoint: CompilerSafepointId,
    root_map: BytecodeRootMapId,
    slot: BytecodeRootSlotDescriptor,
) -> Result<CompilerRootSlotDescriptor, JitPlanValidationError> {
    let location = match slot.storage {
        BytecodeRootSlotStorage::Register(register) => {
            if !register.is_valid() {
                return Err(JitPlanValidationError::SafepointInvalidVirtualRegister {
                    safepoint,
                    register,
                });
            }
            if matches!(
                (slot.root_kind, slot.mutation_authority),
                (
                    RootKind::Stack,
                    RootSetMutationAuthority::ConservativeScanner
                )
            ) {
                CompilerRootSlotLocation::StackSlot(register.raw())
            } else {
                CompilerRootSlotLocation::VirtualRegister(register)
            }
        }
        BytecodeRootSlotStorage::RuntimeSlot(runtime_slot) => match slot.kind {
            BytecodeRootSlotKind::MetadataSlot => {
                CompilerRootSlotLocation::MetadataSlot(runtime_slot.0)
            }
            BytecodeRootSlotKind::InlineCache => {
                CompilerRootSlotLocation::InlineCacheSlot(runtime_slot.0)
            }
            BytecodeRootSlotKind::ValueProfile => {
                CompilerRootSlotLocation::ValueProfileSlot(runtime_slot.0)
            }
            BytecodeRootSlotKind::VirtualRegister
            | BytecodeRootSlotKind::Argument
            | BytecodeRootSlotKind::Constant
            | BytecodeRootSlotKind::CallSite => {
                return Err(JitPlanValidationError::SafepointRootMapInvalid {
                    safepoint,
                    root_map,
                    error: BytecodeRootMapValidationError::RootSlotKindStorageMismatch {
                        bytecode_index: slot.bytecode_index,
                        kind: slot.kind,
                        storage: slot.storage,
                    },
                });
            }
        },
        BytecodeRootSlotStorage::ConstantIndex(index) => {
            CompilerRootSlotLocation::ConstantPool(index)
        }
        BytecodeRootSlotStorage::CallSite(index) => CompilerRootSlotLocation::CallSite(index),
    };

    Ok(CompilerRootSlotDescriptor {
        bytecode_index: Some(slot.bytecode_index),
        location,
        slot_kind: slot.kind,
        root_kind: slot.root_kind,
        mutation_authority: slot.mutation_authority,
        precise: slot.precise,
    })
}

fn validate_compiler_root_slot_location(
    safepoint: CompilerSafepointId,
    root: &CompilerRootSlotDescriptor,
) -> Result<(), JitPlanValidationError> {
    if let CompilerRootSlotLocation::VirtualRegister(register) = root.location {
        if !register.is_valid() {
            return Err(JitPlanValidationError::SafepointInvalidVirtualRegister {
                safepoint,
                register,
            });
        }
    }
    Ok(())
}

const fn compiler_root_is_conservative(root: &CompilerRootSlotDescriptor) -> bool {
    !root.precise
        || matches!(
            (root.root_kind, root.mutation_authority),
            (
                RootKind::Stack,
                RootSetMutationAuthority::ConservativeScanner
            )
        )
}

fn compiler_safepoint_root_id(safepoint: CompilerSafepointId, slot_index: usize) -> RootId {
    RootId(
        6_000_000_000_u64
            .saturating_add(u64::from(safepoint.0).saturating_mul(1_000))
            .saturating_add(slot_index as u64)
            .saturating_add(1),
    )
}

const fn tier_matches_mode(tier: JitPlanTier, mode: CompilationMode) -> bool {
    matches!(
        (tier, mode),
        (JitPlanTier::Baseline, CompilationMode::Baseline)
            | (JitPlanTier::Baseline, CompilationMode::InlineCacheStub)
            | (JitPlanTier::Dfg, CompilationMode::Dfg)
            | (JitPlanTier::Dfg, CompilationMode::OsrEntry)
            | (JitPlanTier::Ftl, CompilationMode::Ftl)
    )
}

const fn request_kind_matches_mode(kind: CompilationRequestKind, mode: CompilationMode) -> bool {
    matches!(
        (kind, mode),
        (CompilationRequestKind::TierUp(_), CompilationMode::Baseline)
            | (CompilationRequestKind::TierUp(_), CompilationMode::Dfg)
            | (CompilationRequestKind::TierUp(_), CompilationMode::Ftl)
            | (CompilationRequestKind::OsrEntry, CompilationMode::OsrEntry)
            | (
                CompilationRequestKind::RecompileAfterInvalidation,
                CompilationMode::Dfg
            )
            | (
                CompilationRequestKind::RecompileAfterInvalidation,
                CompilationMode::Ftl
            )
            | (
                CompilationRequestKind::InlineCacheRegeneration,
                CompilationMode::InlineCacheStub
            )
            | (
                CompilationRequestKind::WasmCompilation,
                CompilationMode::WasmFunction
            )
            | (
                CompilationRequestKind::HostBridge,
                CompilationMode::WasmBridge
            )
    )
}

const BASELINE_REQUEST_KINDS: &[CompilationRequestKind] =
    &[CompilationRequestKind::TierUp(TieringTrigger::EntryCounter)];
const DFG_REQUEST_KINDS: &[CompilationRequestKind] = &[
    CompilationRequestKind::TierUp(TieringTrigger::LoopCounter),
    CompilationRequestKind::OsrEntry,
    CompilationRequestKind::RecompileAfterInvalidation,
];
const FTL_REQUEST_KINDS: &[CompilationRequestKind] = &[
    CompilationRequestKind::TierUp(TieringTrigger::LoopCounter),
    CompilationRequestKind::OsrEntry,
    CompilationRequestKind::RecompileAfterInvalidation,
];
const IC_REQUEST_KINDS: &[CompilationRequestKind] =
    &[CompilationRequestKind::InlineCacheRegeneration];

const BASELINE_INSTALL_BARRIERS: &[CodeInstallBarrier] = &[CodeInstallBarrier::OwnerStillLive];
const OPTIMIZING_INSTALL_BARRIERS: &[CodeInstallBarrier] = &[
    CodeInstallBarrier::OwnerStillLive,
    CodeInstallBarrier::WatchpointsStillValid,
    CodeInstallBarrier::MainThreadFinalization,
];
const IC_INSTALL_BARRIERS: &[CodeInstallBarrier] = &[
    CodeInstallBarrier::OwnerStillLive,
    CodeInstallBarrier::StructureEpochUnchanged,
];

pub const STATIC_JIT_PLAN_DESCRIPTORS: &[StaticJitPlanDescriptor] = &[
    StaticJitPlanDescriptor {
        name: "baseline-from-bytecode",
        tier: JitPlanTier::Baseline,
        mode: CompilationMode::Baseline,
        request_kinds: BASELINE_REQUEST_KINDS,
        install_barriers: BASELINE_INSTALL_BARRIERS,
        owner: JitPlanSchemaOwner::BaselineJit,
        mutation_authority: JitPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: JitPlanSchemaProvenance {
            source: "JIT tier planning metadata",
            generated_by: "rust-static-schema",
        },
    },
    StaticJitPlanDescriptor {
        name: "dfg-optimizing",
        tier: JitPlanTier::Dfg,
        mode: CompilationMode::Dfg,
        request_kinds: DFG_REQUEST_KINDS,
        install_barriers: OPTIMIZING_INSTALL_BARRIERS,
        owner: JitPlanSchemaOwner::DfgJit,
        mutation_authority: JitPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: JitPlanSchemaProvenance {
            source: "DFG tier planning metadata",
            generated_by: "rust-static-schema",
        },
    },
    StaticJitPlanDescriptor {
        name: "ftl-optimizing",
        tier: JitPlanTier::Ftl,
        mode: CompilationMode::Ftl,
        request_kinds: FTL_REQUEST_KINDS,
        install_barriers: OPTIMIZING_INSTALL_BARRIERS,
        owner: JitPlanSchemaOwner::FtlJit,
        mutation_authority: JitPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: JitPlanSchemaProvenance {
            source: "FTL tier planning metadata",
            generated_by: "rust-static-schema",
        },
    },
    StaticJitPlanDescriptor {
        name: "inline-cache-stub",
        tier: JitPlanTier::Baseline,
        mode: CompilationMode::InlineCacheStub,
        request_kinds: IC_REQUEST_KINDS,
        install_barriers: IC_INSTALL_BARRIERS,
        owner: JitPlanSchemaOwner::InlineCacheRegistry,
        mutation_authority: JitPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: JitPlanSchemaProvenance {
            source: "Inline cache plan metadata",
            generated_by: "rust-static-schema",
        },
    },
];

pub const JIT_PLAN_DESCRIPTOR_REGISTRY: JitPlanDescriptorRegistry =
    JitPlanDescriptorRegistry::new(STATIC_JIT_PLAN_DESCRIPTORS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        BytecodeRange, BytecodeRootMap, BytecodeRootSlotDescriptor, BytecodeRootSlotStorage,
        CodeBlock, CodeKind, HandlerInfo, HandlerKind, HandlerRange, HandlerTarget,
        InstructionBuilder, LinkContext, LinkedSideTables, Operand, OperandWidth, RuntimeSlot,
        TypedInstruction, UnlinkedCodeBlock, UnlinkedCodeBlockPhase,
    };
    use crate::gc::{
        CellId, HeapId, RootKind, RootRecord, RootSetSemanticError, TargetedRootRecord,
    };
    use crate::jit::{CallBoundaryId, CallLinkInfoDescriptor, CallLinkMode, LinkedCallKind};

    #[test]
    fn static_jit_plan_registry_names_schema_owners() {
        let descriptors = JIT_PLAN_DESCRIPTOR_REGISTRY.descriptors();

        JIT_PLAN_DESCRIPTOR_REGISTRY.validate().unwrap();
        assert!(!descriptors.is_empty());
        assert!(descriptors
            .iter()
            .all(|descriptor| !descriptor.name.is_empty()));
        assert!(descriptors
            .iter()
            .all(|descriptor| !descriptor.install_barriers.is_empty()));

        let ftl = JIT_PLAN_DESCRIPTOR_REGISTRY.descriptor_for_name("ftl-optimizing");
        assert_eq!(
            ftl.map(|descriptor| descriptor.owner),
            Some(JitPlanSchemaOwner::FtlJit)
        );
        assert_eq!(
            ftl.map(|descriptor| descriptor.tier),
            Some(JitPlanTier::Ftl)
        );
    }

    #[test]
    fn compilation_request_builder_rejects_mismatched_tier() {
        let request = CompilationRequest::builder(
            CompilationPlanId(1),
            CompilationMode::Dfg,
            CompilationRequestKind::TierUp(TieringTrigger::LoopCounter),
        )
        .requested_tier(JitType::Baseline)
        .build();

        assert_eq!(request, Err(JitPlanValidationError::RequestTierMismatch));
    }

    #[test]
    fn request_order_places_tier_dependencies_before_hotter_consumers() {
        let ftl = CompilationRequest::builder(
            CompilationPlanId(3),
            CompilationMode::Ftl,
            CompilationRequestKind::TierUp(TieringTrigger::LoopCounter),
        )
        .priority(CompilationPriority::HotLoop)
        .build()
        .unwrap();
        let baseline = CompilationRequest::builder(
            CompilationPlanId(1),
            CompilationMode::Baseline,
            CompilationRequestKind::TierUp(TieringTrigger::EntryCounter),
        )
        .priority(CompilationPriority::Background)
        .build()
        .unwrap();
        let dfg = CompilationRequest::builder(
            CompilationPlanId(2),
            CompilationMode::Dfg,
            CompilationRequestKind::TierUp(TieringTrigger::LoopCounter),
        )
        .priority(CompilationPriority::Interactive)
        .build()
        .unwrap();

        assert_eq!(
            order_compilation_requests(&[ftl, dfg, baseline]),
            Ok(vec![
                CompilationPlanId(1),
                CompilationPlanId(2),
                CompilationPlanId(3)
            ])
        );
    }

    #[test]
    fn install_barrier_selection_uses_static_plan_schema() {
        let request = CompilationRequest::builder(
            CompilationPlanId(4),
            CompilationMode::InlineCacheStub,
            CompilationRequestKind::InlineCacheRegeneration,
        )
        .build()
        .unwrap();

        assert_eq!(
            install_barriers_for_request(&request, JIT_PLAN_DESCRIPTOR_REGISTRY),
            Ok(IC_INSTALL_BARRIERS)
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_accepts_p6_narrow_subset() {
        let record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadInt32),
            baseline_instruction(1, CoreOpcode::Move),
            baseline_instruction(2, CoreOpcode::AddInt32),
            baseline_instruction(3, CoreOpcode::Return),
        ]);

        let proof = record.validate().unwrap();

        assert_eq!(proof.owner(), baseline_owner());
        assert_eq!(
            proof.opcode_subset(),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
        );
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 0,
                safepoint_count: 0,
                complete_safepoint_root_map_count: 0,
            }
        );
        assert_eq!(proof.bytecode().instruction_count, 4);
        assert_eq!(
            proof.generated_effect_contract(),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
                .generated_effect_contract()
        );
        assert!(proof
            .generated_effect_contract()
            .permits_no_heap_allocation_no_runtime_call());
        assert!(
            proof
                .generated_effect_contract()
                .records_write_barrier_handoff
        );
        assert!(!proof.generated_effect_contract().touches_gc_roots);
    }

    #[test]
    fn p6_generated_effect_contract_is_no_heap_allocation_no_runtime_call() {
        let subset = BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::LoadUndefined,
            CoreOpcode::LoadNull,
            CoreOpcode::LoadBool,
            CoreOpcode::LoadInt32,
            CoreOpcode::Move,
            CoreOpcode::Return,
            CoreOpcode::AddInt32,
            CoreOpcode::SubInt32,
            CoreOpcode::MulInt32,
        ] {
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        }
    }

    #[test]
    fn current_generated_effect_contract_accepts_full_no_heap_subset() {
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary;
        let contract = subset.generated_effect_contract();

        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);

        for opcode in [
            CoreOpcode::LoadUndefined,
            CoreOpcode::LoadNull,
            CoreOpcode::LoadBool,
            CoreOpcode::LoadInt32,
            CoreOpcode::LoadDouble,
            CoreOpcode::Move,
            CoreOpcode::Return,
            CoreOpcode::AddInt32,
            CoreOpcode::SubInt32,
            CoreOpcode::MulInt32,
            CoreOpcode::BitNotInt32,
            CoreOpcode::BitOrInt32,
            CoreOpcode::BitXorInt32,
            CoreOpcode::BitAndInt32,
            CoreOpcode::LeftShiftInt32,
            CoreOpcode::RightShiftInt32,
            CoreOpcode::UnsignedRightShiftInt32,
            CoreOpcode::LessThanInt32,
            CoreOpcode::LessEqualInt32,
            CoreOpcode::GreaterThanInt32,
            CoreOpcode::GreaterEqualInt32,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfFalse,
            CoreOpcode::JumpIfNotNullish,
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LogicalNot,
            CoreOpcode::ToNumber,
            CoreOpcode::NegateNumber,
            CoreOpcode::DivNumber,
            CoreOpcode::ModNumber,
            CoreOpcode::Void,
            CoreOpcode::LoadCallee,
        ] {
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        }
    }

    #[test]
    fn generated_runtime_boundary_classifies_no_js_call_heap_helpers_without_enabling_them() {
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary;

        for opcode in [
            CoreOpcode::LoadString,
            CoreOpcode::LoadBigInt,
            CoreOpcode::LoadFunction,
            CoreOpcode::InitializeGlobalLexical,
            CoreOpcode::TypeOf,
            CoreOpcode::NewObject,
            CoreOpcode::NewArray,
        ] {
            assert!(!subset.supports(opcode));

            let contract = baseline_generated_runtime_boundary_contract(opcode).unwrap();
            assert_eq!(contract.opcode, opcode);
            assert_eq!(
                contract.category,
                BaselineGeneratedRuntimeBoundaryCategory::NoJsCallHeapRuntimeHelper
            );
            assert!(!contract.summary.may_call_js);
            assert!(contract.summary.may_throw);
            assert!(contract.summary.writes_heap);
            assert!(contract.effects.calls_runtime_helper);
            assert!(contract.effects.may_throw);
            assert!(contract.effects.writes_heap);
            assert!(contract.effects.touches_gc_roots);
            assert!(contract.effects.records_write_barrier_handoff);
            assert!(contract.requirements.complete_safepoint_root_map);
            assert!(contract.requirements.no_gc_exit_reentry);
        }

        for opcode in [
            CoreOpcode::LoadString,
            CoreOpcode::LoadBigInt,
            CoreOpcode::TypeOf,
            CoreOpcode::NewObject,
            CoreOpcode::NewArray,
        ] {
            assert!(
                baseline_generated_runtime_boundary_contract(opcode)
                    .unwrap()
                    .effects
                    .allocates
            );
        }
    }

    #[test]
    fn p6_bitwise_generated_effect_contract_extends_only_clean_int32_ops() {
        let subset = BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::BitNotInt32,
            CoreOpcode::BitOrInt32,
            CoreOpcode::BitXorInt32,
            CoreOpcode::BitAndInt32,
            CoreOpcode::LeftShiftInt32,
            CoreOpcode::RightShiftInt32,
            CoreOpcode::UnsignedRightShiftInt32,
        ] {
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        }
    }

    #[test]
    fn p6_bitand_or_exact_native_subset_excludes_unlowered_bitwise_ops() {
        let subset = BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOr;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);

        assert!(subset.supports(CoreOpcode::BitAndInt32));
        assert!(subset.supports(CoreOpcode::BitOrInt32));
        assert!(subset.supports(CoreOpcode::BitXorInt32));
        assert!(subset.supports(CoreOpcode::RightShiftInt32));
        for opcode in [
            CoreOpcode::BitNotInt32,
            CoreOpcode::LeftShiftInt32,
            CoreOpcode::UnsignedRightShiftInt32,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfFalse,
        ] {
            assert!(!subset.supports(opcode), "{opcode:?}");
        }

        assert!(
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish
                .supports(CoreOpcode::JumpIfNotNullish)
        );
        assert!(
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullishFalse
                .supports(CoreOpcode::JumpIfFalse)
        );
    }

    #[test]
    fn p6_bitand_or_equality_exact_native_subset_adds_only_int32_equality_ops() {
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);

        for opcode in [
            CoreOpcode::BitAndInt32,
            CoreOpcode::BitOrInt32,
            CoreOpcode::BitXorInt32,
            CoreOpcode::RightShiftInt32,
            CoreOpcode::Equal,
            CoreOpcode::NotEqual,
        ] {
            assert!(subset.supports(opcode), "{opcode:?}");
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        }

        for opcode in [
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LeftShiftInt32,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfFalse,
        ] {
            assert!(!subset.supports(opcode), "{opcode:?}");
        }

        assert!(
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish
                .supports(CoreOpcode::JumpIfNotNullish)
        );
        assert!(
            !BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish
                .supports(CoreOpcode::JumpIfFalse)
        );
        assert!(
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse
                .supports(CoreOpcode::JumpIfFalse)
        );
    }

    #[test]
    fn p6_bitand_or_equality_relational_exact_native_subset_adds_only_int32_relational_ops() {
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);

        for opcode in [
            CoreOpcode::BitAndInt32,
            CoreOpcode::BitOrInt32,
            CoreOpcode::BitXorInt32,
            CoreOpcode::RightShiftInt32,
            CoreOpcode::Equal,
            CoreOpcode::NotEqual,
            CoreOpcode::LessThanInt32,
            CoreOpcode::LessEqualInt32,
            CoreOpcode::GreaterThanInt32,
            CoreOpcode::GreaterEqualInt32,
        ] {
            assert!(subset.supports(opcode), "{opcode:?}");
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        }

        for opcode in [
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LogicalNot,
            CoreOpcode::LeftShiftInt32,
            CoreOpcode::ToNumber,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfFalse,
        ] {
            assert!(!subset.supports(opcode), "{opcode:?}");
        }

        assert!(
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish
                .supports(CoreOpcode::JumpIfNotNullish)
        );
        assert!(
            !BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish
                .supports(CoreOpcode::JumpIfFalse)
        );
        assert!(
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse
                .supports(CoreOpcode::JumpIfFalse)
        );
    }

    #[test]
    fn p6_no_call_loose_equality_native_subsets_preserve_no_heap_contract() {
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality;
        let relational_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational;
        for subset in [subset, relational_subset] {
            let contract = subset.generated_effect_contract();
            assert!(contract.permits_no_heap_allocation_no_runtime_call());
            assert!(!contract.summary.reads_heap);
            assert!(!contract.summary.writes_heap);
            assert!(!contract.summary.allocates);
            assert!(!contract.summary.may_call_js);
            assert!(!contract.summary.may_throw);
            assert!(!contract.may_call_runtime);
            assert!(!contract.touches_gc_roots);
            assert!(subset.supports(CoreOpcode::BitXorInt32));
            assert!(subset.supports(CoreOpcode::RightShiftInt32));
            assert!(subset.supports(CoreOpcode::Equal));
            assert!(subset.supports(CoreOpcode::NotEqual));
        }

        for opcode in [
            CoreOpcode::LessThanInt32,
            CoreOpcode::LessEqualInt32,
            CoreOpcode::GreaterThanInt32,
            CoreOpcode::GreaterEqualInt32,
        ] {
            assert!(!subset.supports(opcode));
            assert!(relational_subset.supports(opcode));
        }
        assert!(
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish
                .supports(CoreOpcode::JumpIfNotNullish)
        );
        assert!(
            !BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish
                .supports(CoreOpcode::JumpIfFalse)
        );
        assert!(
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse
                .supports(CoreOpcode::JumpIfFalse)
        );
    }

    #[test]
    fn p6_relational_generated_effect_contract_extends_only_clean_int32_comparisons() {
        let bitwise_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::LessThanInt32,
            CoreOpcode::LessEqualInt32,
            CoreOpcode::GreaterThanInt32,
            CoreOpcode::GreaterEqualInt32,
        ] {
            assert!(!bitwise_subset.supports(opcode));
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        }

        for opcode in [
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LogicalNot,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfFalse,
            CoreOpcode::JumpIfNotNullish,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadInt32),
            baseline_instruction(1, CoreOpcode::LoadInt32),
            baseline_instruction(2, CoreOpcode::LessThanInt32),
            baseline_instruction(3, CoreOpcode::Return),
        ]);
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(2),
                    opcode: CoreOpcode::LessThanInt32,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p6_jump_generated_effect_contract_extends_only_pc_branches() {
        let relational_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelational;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [CoreOpcode::Jump, CoreOpcode::JumpIfNotNullish] {
            assert!(!relational_subset.supports(opcode));
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
            assert!(!effect.summary.reads_heap);
            assert!(!effect.summary.writes_heap);
            assert!(!effect.summary.allocates);
            assert!(!effect.summary.may_call_js);
            assert!(!effect.summary.may_throw);
            assert!(effect.summary.may_exit);
            assert!(!effect.summary.writes_local_state);
            assert!(!effect.may_call_runtime);
            assert!(!effect.touches_gc_roots);
            assert!(!effect.records_write_barrier_handoff);
        }

        assert!(
            !baseline_opcode_effect(CoreOpcode::Jump)
                .summary
                .reads_local_state
        );
        assert!(
            baseline_opcode_effect(CoreOpcode::JumpIfNotNullish)
                .summary
                .reads_local_state
        );

        for opcode in [
            CoreOpcode::JumpIfFalse,
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LogicalNot,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadUndefined),
            baseline_instruction(1, CoreOpcode::JumpIfNotNullish),
            baseline_instruction(2, CoreOpcode::Jump),
            baseline_instruction(3, CoreOpcode::Return),
        ]);
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::JumpIfNotNullish,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p8a_native_branch_subset_advertises_only_p6_plus_jump_and_nullish_branch() {
        let subset =
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish;
        let contract = subset.generated_effect_contract();
        assert_eq!(contract.opcode_subset, subset);
        assert!(contract.permits_no_heap_allocation_no_runtime_call());

        for opcode in [
            CoreOpcode::LoadUndefined,
            CoreOpcode::LoadNull,
            CoreOpcode::LoadBool,
            CoreOpcode::LoadInt32,
            CoreOpcode::Move,
            CoreOpcode::Return,
            CoreOpcode::AddInt32,
            CoreOpcode::SubInt32,
            CoreOpcode::MulInt32,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfNotNullish,
        ] {
            assert!(subset.supports(opcode), "{opcode:?}");
        }

        for opcode in [
            CoreOpcode::JumpIfFalse,
            CoreOpcode::BitNotInt32,
            CoreOpcode::BitAndInt32,
            CoreOpcode::LessThanInt32,
            CoreOpcode::StrictEqual,
            CoreOpcode::LogicalNot,
        ] {
            assert!(!subset.supports(opcode), "{opcode:?}");
        }
    }

    #[test]
    fn p8b_native_branch_subset_advertises_p8a_plus_jump_if_false_only() {
        let p8a =
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish;
        let subset =
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse;
        let contract = subset.generated_effect_contract();
        assert_eq!(contract.opcode_subset, subset);
        assert!(contract.permits_no_heap_allocation_no_runtime_call());

        for opcode in [
            CoreOpcode::LoadUndefined,
            CoreOpcode::LoadNull,
            CoreOpcode::LoadBool,
            CoreOpcode::LoadInt32,
            CoreOpcode::Move,
            CoreOpcode::Return,
            CoreOpcode::AddInt32,
            CoreOpcode::SubInt32,
            CoreOpcode::MulInt32,
            CoreOpcode::Jump,
            CoreOpcode::JumpIfNotNullish,
            CoreOpcode::JumpIfFalse,
        ] {
            assert!(subset.supports(opcode), "{opcode:?}");
        }

        assert!(!p8a.supports(CoreOpcode::JumpIfFalse));
        for opcode in [
            CoreOpcode::BitNotInt32,
            CoreOpcode::BitAndInt32,
            CoreOpcode::LessThanInt32,
            CoreOpcode::StrictEqual,
            CoreOpcode::LogicalNot,
            CoreOpcode::LoadDouble,
        ] {
            assert!(!subset.supports(opcode), "{opcode:?}");
        }

        let effect = baseline_opcode_effect(CoreOpcode::JumpIfFalse);
        assert_eq!(effect.opcode, CoreOpcode::JumpIfFalse);
        assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        assert!(!effect.summary.reads_heap);
        assert!(!effect.summary.writes_heap);
        assert!(!effect.summary.allocates);
        assert!(!effect.summary.may_call_js);
        assert!(!effect.summary.may_throw);
        assert!(effect.summary.may_exit);
        assert!(effect.summary.reads_local_state);
        assert!(!effect.summary.writes_local_state);
        assert!(!effect.may_call_runtime);
        assert!(!effect.touches_gc_roots);
    }

    #[test]
    fn p6_primitive_truthiness_generated_effect_contract_extends_only_jump_if_false() {
        let jumps_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumps;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        assert!(!jumps_subset.supports(CoreOpcode::JumpIfFalse));
        assert!(subset.supports(CoreOpcode::JumpIfFalse));
        for opcode in [CoreOpcode::Jump, CoreOpcode::JumpIfNotNullish] {
            assert!(jumps_subset.supports(opcode));
            assert!(subset.supports(opcode));
        }

        let effect = baseline_opcode_effect(CoreOpcode::JumpIfFalse);
        assert_eq!(effect.opcode, CoreOpcode::JumpIfFalse);
        assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
        assert!(!effect.summary.reads_heap);
        assert!(!effect.summary.writes_heap);
        assert!(!effect.summary.allocates);
        assert!(!effect.summary.may_call_js);
        assert!(!effect.summary.may_throw);
        assert!(effect.summary.may_exit);
        assert!(effect.summary.reads_local_state);
        assert!(!effect.summary.writes_local_state);
        assert!(!effect.may_call_runtime);
        assert!(!effect.touches_gc_roots);
        assert!(!effect.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LogicalNot,
            CoreOpcode::Call,
            CoreOpcode::NewObject,
            CoreOpcode::GetByName,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadUndefined),
            baseline_instruction(1, CoreOpcode::JumpIfFalse),
            baseline_instruction(2, CoreOpcode::Return),
        ]);
        record.opcode_subset = jumps_subset;
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::JumpIfFalse,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p6_primitive_boolean_generated_effect_contract_extends_truthiness() {
        let truthiness_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthiness;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::StrictEqual,
            CoreOpcode::StrictNotEqual,
            CoreOpcode::LogicalNot,
        ] {
            assert!(!truthiness_subset.supports(opcode));
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
            assert!(!effect.summary.reads_heap);
            assert!(!effect.summary.writes_heap);
            assert!(!effect.summary.allocates);
            assert!(!effect.summary.may_call_js);
            assert!(!effect.summary.may_throw);
            assert!(effect.summary.may_exit);
            assert!(effect.summary.reads_local_state);
            assert!(effect.summary.writes_local_state);
            assert!(!effect.may_call_runtime);
            assert!(!effect.touches_gc_roots);
            assert!(effect.records_write_barrier_handoff);
        }

        for opcode in [
            CoreOpcode::Call,
            CoreOpcode::NewObject,
            CoreOpcode::GetByName,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadBool),
            baseline_instruction(1, CoreOpcode::LogicalNot),
            baseline_instruction(2, CoreOpcode::StrictEqual),
            baseline_instruction(3, CoreOpcode::StrictNotEqual),
            baseline_instruction(4, CoreOpcode::Return),
        ]);
        record.opcode_subset = truthiness_subset;
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::LogicalNot,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p6_primitive_number_generated_effect_contract_extends_primitive_boolean() {
        let primitive_boolean_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        let load_double_effect = baseline_opcode_effect(CoreOpcode::LoadDouble);
        assert_eq!(load_double_effect.opcode, CoreOpcode::LoadDouble);
        assert_eq!(
            baseline_generated_effect_rejection_reason(load_double_effect),
            None
        );
        assert!(!load_double_effect.summary.reads_heap);
        assert!(!load_double_effect.summary.writes_heap);
        assert!(!load_double_effect.summary.allocates);
        assert!(!load_double_effect.summary.may_call_js);
        assert!(!load_double_effect.summary.may_throw);
        assert!(!load_double_effect.summary.may_exit);
        assert!(!load_double_effect.summary.reads_local_state);
        assert!(load_double_effect.summary.writes_local_state);
        assert!(!load_double_effect.may_call_runtime);
        assert!(!load_double_effect.touches_gc_roots);
        assert!(load_double_effect.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::NegateNumber,
            CoreOpcode::DivNumber,
            CoreOpcode::ModNumber,
        ] {
            assert!(!primitive_boolean_subset.supports(opcode));
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
            assert!(!effect.summary.reads_heap);
            assert!(!effect.summary.writes_heap);
            assert!(!effect.summary.allocates);
            assert!(!effect.summary.may_call_js);
            assert!(!effect.summary.may_throw);
            assert!(effect.summary.may_exit);
            assert!(effect.summary.reads_local_state);
            assert!(effect.summary.writes_local_state);
            assert!(!effect.may_call_runtime);
            assert!(!effect.touches_gc_roots);
            assert!(effect.records_write_barrier_handoff);
        }

        for opcode in [
            CoreOpcode::PowNumber,
            CoreOpcode::ToNumber,
            CoreOpcode::Call,
            CoreOpcode::NewObject,
            CoreOpcode::GetByName,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadDouble),
            baseline_instruction(1, CoreOpcode::NegateNumber),
            baseline_instruction(2, CoreOpcode::DivNumber),
            baseline_instruction(3, CoreOpcode::ModNumber),
            baseline_instruction(4, CoreOpcode::Return),
        ]);
        record.opcode_subset = primitive_boolean_subset;
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::LoadDouble,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p6_primitive_to_number_void_generated_effect_contract_extends_primitive_number() {
        let primitive_number_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumber;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [CoreOpcode::ToNumber, CoreOpcode::Void] {
            assert!(!primitive_number_subset.supports(opcode));
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
            assert!(!effect.summary.reads_heap);
            assert!(!effect.summary.writes_heap);
            assert!(!effect.summary.allocates);
            assert!(!effect.summary.may_call_js);
            assert!(!effect.summary.may_throw);
            assert!(effect.summary.reads_local_state);
            assert!(effect.summary.writes_local_state);
            assert!(!effect.may_call_runtime);
            assert!(!effect.touches_gc_roots);
            assert!(effect.records_write_barrier_handoff);
        }

        assert!(
            baseline_opcode_effect(CoreOpcode::ToNumber)
                .summary
                .may_exit
        );
        assert!(!baseline_opcode_effect(CoreOpcode::Void).summary.may_exit);

        for opcode in [
            CoreOpcode::PowNumber,
            CoreOpcode::TypeOf,
            CoreOpcode::Call,
            CoreOpcode::NewObject,
            CoreOpcode::GetByName,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadBool),
            baseline_instruction(1, CoreOpcode::ToNumber),
            baseline_instruction(2, CoreOpcode::Void),
            baseline_instruction(3, CoreOpcode::Return),
        ]);
        record.opcode_subset = primitive_number_subset;
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::ToNumber,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p6_primitive_to_number_emitted_effect_contract_is_guarded_no_call_surface() {
        let base_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber;
        let p8a_subset =
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish;
        let p8b_subset =
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse;
        let contract = subset.generated_effect_contract();

        assert!(!base_subset.supports(CoreOpcode::ToNumber));
        assert!(subset.supports(CoreOpcode::ToNumber));
        assert!(!subset.supports(CoreOpcode::Void));
        assert!(p8a_subset.supports(CoreOpcode::JumpIfNotNullish));
        assert!(!p8a_subset.supports(CoreOpcode::JumpIfFalse));
        assert!(p8b_subset.supports(CoreOpcode::JumpIfFalse));
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(contract.summary.may_exit);
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadBool),
            baseline_instruction(1, CoreOpcode::ToNumber),
            baseline_instruction(2, CoreOpcode::Return),
        ]);
        record.opcode_subset = base_subset;
        assert_eq!(
            record.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::ToNumber,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn p6_pure_number_binary_generated_effect_contract_keeps_clean_binary_surface() {
        let previous_subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoid;
        let subset =
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanPrimitiveNumberPrimitiveToNumberVoidPureNumberBinary;
        let contract = subset.generated_effect_contract();
        assert!(contract.permits_no_heap_allocation_no_runtime_call());
        assert!(!contract.summary.reads_heap);
        assert!(!contract.summary.writes_heap);
        assert!(!contract.summary.allocates);
        assert!(!contract.summary.may_call_js);
        assert!(!contract.summary.may_throw);
        assert!(contract.summary.may_exit);
        assert!(contract.summary.reads_local_state);
        assert!(contract.summary.writes_local_state);
        assert!(!contract.may_call_runtime);
        assert!(!contract.touches_gc_roots);
        assert!(contract.records_write_barrier_handoff);

        for opcode in [
            CoreOpcode::AddInt32,
            CoreOpcode::SubInt32,
            CoreOpcode::MulInt32,
            CoreOpcode::DivNumber,
            CoreOpcode::ModNumber,
            CoreOpcode::BitOrInt32,
            CoreOpcode::BitXorInt32,
            CoreOpcode::BitAndInt32,
            CoreOpcode::LeftShiftInt32,
            CoreOpcode::RightShiftInt32,
            CoreOpcode::UnsignedRightShiftInt32,
            CoreOpcode::LessThanInt32,
            CoreOpcode::LessEqualInt32,
            CoreOpcode::GreaterThanInt32,
            CoreOpcode::GreaterEqualInt32,
        ] {
            assert!(previous_subset.supports(opcode));
            assert!(subset.supports(opcode));
            let effect = baseline_opcode_effect(opcode);
            assert_eq!(effect.opcode, opcode);
            assert_eq!(baseline_generated_effect_rejection_reason(effect), None);
            assert!(!effect.summary.reads_heap);
            assert!(!effect.summary.writes_heap);
            assert!(!effect.summary.allocates);
            assert!(!effect.summary.may_call_js);
            assert!(!effect.summary.may_throw);
            assert!(effect.summary.may_exit);
            assert!(effect.summary.reads_local_state);
            assert!(effect.summary.writes_local_state);
            assert!(!effect.may_call_runtime);
            assert!(!effect.touches_gc_roots);
            assert!(effect.records_write_barrier_handoff);
        }

        for opcode in [
            CoreOpcode::PowNumber,
            CoreOpcode::TypeOf,
            CoreOpcode::Call,
            CoreOpcode::NewObject,
            CoreOpcode::GetByName,
        ] {
            assert!(!subset.supports(opcode));
        }

        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadDouble),
            baseline_instruction(1, CoreOpcode::AddInt32),
            baseline_instruction(2, CoreOpcode::BitOrInt32),
            baseline_instruction(3, CoreOpcode::LessThanInt32),
            baseline_instruction(4, CoreOpcode::DivNumber),
            baseline_instruction(5, CoreOpcode::ModNumber),
            baseline_instruction(6, CoreOpcode::Return),
        ]);
        record.opcode_subset = subset;
        let proof = record.validate().unwrap();
        assert_eq!(proof.opcode_subset(), subset);
        assert_eq!(proof.generated_effect_contract(), contract);
    }

    #[test]
    fn generated_effect_audit_rejects_call_allocation_property_and_exception_effects() {
        assert_eq!(
            baseline_generated_effect_rejection_reason(baseline_opcode_effect(CoreOpcode::Call)),
            Some(BaselineGeneratedEffectRejectionReason::MayCallRuntime)
        );
        assert_eq!(
            baseline_generated_effect_rejection_reason(baseline_opcode_effect(
                CoreOpcode::NewObject
            )),
            Some(BaselineGeneratedEffectRejectionReason::MayAllocate)
        );
        assert_eq!(
            baseline_generated_effect_rejection_reason(baseline_opcode_effect(
                CoreOpcode::GetByName
            )),
            Some(BaselineGeneratedEffectRejectionReason::MayCallRuntime)
        );
        assert_eq!(
            baseline_generated_effect_rejection_reason(baseline_opcode_effect(CoreOpcode::Throw)),
            Some(BaselineGeneratedEffectRejectionReason::MayThrow)
        );
    }

    #[test]
    fn generated_runtime_boundary_candidate_rejects_missing_and_incomplete_root_map() {
        let missing = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::NewObject,
            safepoint: baseline_safepoint_without_root_map(Vec::new()),
            root_map: None,
            no_gc_exit_reentry: true,
        };

        assert_eq!(
            missing.validate(),
            Err(BaselineGeneratedRuntimeBoundaryValidationError::Safepoint(
                JitPlanValidationError::SafepointMissingRootMap(CompilerSafepointId(1))
            ))
        );

        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(41);
        let mut incomplete_root_map = complete_root_map(
            root_map_id,
            Some(baseline_owner()),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        incomplete_root_map.complete = false;
        let incomplete = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::NewObject,
            safepoint: baseline_safepoint_referencing(root_map_id, Vec::new()),
            root_map: Some(incomplete_root_map),
            no_gc_exit_reentry: true,
        };

        assert_eq!(
            incomplete.validate(),
            Err(BaselineGeneratedRuntimeBoundaryValidationError::Safepoint(
                JitPlanValidationError::SafepointIncompleteRootMap {
                    safepoint: CompilerSafepointId(1),
                    root_map: root_map_id,
                }
            ))
        );
    }

    #[test]
    fn generated_runtime_boundary_candidate_accepts_complete_root_map() {
        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(42);
        let root_map = complete_root_map(
            root_map_id,
            Some(baseline_owner()),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::NewObject,
            safepoint: baseline_safepoint_referencing(root_map_id, Vec::new()),
            root_map: Some(root_map),
            no_gc_exit_reentry: true,
        };

        let proof = candidate.validate().unwrap();

        assert_eq!(proof.safepoint, CompilerSafepointId(1));
        assert_eq!(proof.root_map, Some(root_map_id));
        assert_eq!(proof.root_count, 1);
        assert!(proof.no_gc_exit_reentry);
        assert!(proof.may_throw);
        assert_eq!(proof.contract.opcode, CoreOpcode::NewObject);
        assert!(proof.contract.effects.calls_runtime_helper);
        assert!(proof.contract.effects.allocates);
        assert_eq!(proof.may_throw, proof.contract.effects.may_throw);
        assert!(proof.contract.requirements.complete_safepoint_root_map);
        assert!(proof.contract.requirements.no_gc_exit_reentry);
    }

    #[test]
    fn generated_runtime_boundary_candidate_accepts_throw_exception_exit() {
        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(43);
        let root_map = complete_root_map(
            root_map_id,
            Some(baseline_owner()),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::Throw,
            safepoint: baseline_safepoint_referencing(root_map_id, Vec::new()),
            root_map: Some(root_map),
            no_gc_exit_reentry: true,
        };

        let proof = candidate.validate().unwrap();

        assert_eq!(proof.safepoint, CompilerSafepointId(1));
        assert_eq!(proof.root_map, Some(root_map_id));
        assert_eq!(proof.root_count, 1);
        assert!(proof.no_gc_exit_reentry);
        assert!(proof.may_throw);
        assert_eq!(proof.contract.opcode, CoreOpcode::Throw);
        assert_eq!(
            proof.contract.category,
            BaselineGeneratedRuntimeBoundaryCategory::ExceptionThrow
        );
        assert!(proof.contract.effects.calls_runtime_helper);
        assert!(!proof.contract.effects.allocates);
        assert!(proof.contract.effects.touches_gc_roots);
        assert_eq!(proof.may_throw, proof.contract.effects.may_throw);
        assert!(proof.contract.requirements.complete_safepoint_root_map);
        assert!(proof.contract.requirements.no_gc_exit_reentry);
    }

    #[test]
    fn generated_runtime_helper_plan_rejects_stale_may_throw_proof_metadata() {
        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(42);
        let root_map = complete_root_map(
            root_map_id,
            Some(baseline_owner()),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::NewObject,
            safepoint: baseline_safepoint_referencing(root_map_id, Vec::new()),
            root_map: Some(root_map),
            no_gc_exit_reentry: true,
        };
        let mut proof = candidate.validate().unwrap();
        proof.may_throw = false;

        let result = BaselineGeneratedRuntimeHelperPlanMetadata::new(
            BaselineBytecodeSnapshotFingerprint {
                instruction_count: 1,
                instruction_stream_hash: 0,
                side_table_hash: 0,
            },
            vec![BaselineGeneratedRuntimeHelperProof::new(index, proof)],
        );

        assert_eq!(
            result,
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperPlanMayThrowMismatch {
                    bytecode_index: index,
                    opcode: CoreOpcode::NewObject,
                    proof_may_throw: false,
                    contract_may_throw: true,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_boundary_candidate_requires_no_gc_exit_reentry() {
        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(43);
        let root_map = complete_root_map(
            root_map_id,
            Some(baseline_owner()),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        let candidate = BaselineGeneratedRuntimeBoundaryCandidate {
            opcode: CoreOpcode::NewObject,
            safepoint: baseline_safepoint_referencing(root_map_id, Vec::new()),
            root_map: Some(root_map),
            no_gc_exit_reentry: false,
        };

        assert_eq!(
            candidate.validate(),
            Err(
                BaselineGeneratedRuntimeBoundaryValidationError::MissingNoGcExitReentry {
                    opcode: CoreOpcode::NewObject,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derives_new_object_from_code_block_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(50);
        let code_block = new_object_code_block_with_root_maps(
            destination,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 1);
        let helper = metadata.proof_at(0).unwrap();
        assert_eq!(helper.bytecode_index, helper_index);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::NewObject);
        assert_eq!(helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);
        assert_eq!(
            derivation.safepoints,
            vec![CompilerSafepointDescriptor {
                id: CompilerSafepointId(1),
                owner: Some(owner),
                code: None,
                tier: JitType::Baseline,
                kind: CompilerSafepointKind::Call,
                bytecode_index: Some(helper_index),
                root_map: Some(root_map_id),
                roots: Vec::new(),
                may_call: true,
                may_allocate: true,
            }]
        );

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 1,
                safepoint_count: 1,
                complete_safepoint_root_map_count: 1,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derives_new_array_from_code_block_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(58);
        let code_block = new_array_code_block_with_root_maps(
            destination,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 1);
        let helper = metadata.proof_at(0).unwrap();
        assert_eq!(helper.bytecode_index, helper_index);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::NewArray);
        assert_eq!(helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 1,
                safepoint_count: 1,
                complete_safepoint_root_map_count: 1,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derives_closure_capture_family_from_root_maps() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let source = VirtualRegister::argument_or_header(5);
        let value = VirtualRegister::argument_or_header(6);

        for (ordinal, (opcode, operands, return_register, slots, root_count, may_allocate)) in [
            (
                CoreOpcode::LoadCapture,
                vec![
                    Operand::Register(destination),
                    Operand::UnsignedImmediate(0),
                ],
                destination,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
                1,
                false,
            ),
            (
                CoreOpcode::NewClosureCell,
                vec![Operand::Register(destination), Operand::Register(source)],
                destination,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::Argument),
                ],
                2,
                true,
            ),
            (
                CoreOpcode::GetClosureCell,
                vec![Operand::Register(destination), Operand::Register(source)],
                destination,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::Argument),
                ],
                2,
                false,
            ),
            (
                CoreOpcode::PutClosureCell,
                vec![Operand::Register(source), Operand::Register(value)],
                value,
                vec![
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::Argument),
                    register_root_slot(helper_index, value, BytecodeRootSlotKind::Argument),
                ],
                2,
                false,
            ),
            (
                CoreOpcode::ArrayAppend,
                vec![Operand::Register(source), Operand::Register(value)],
                source,
                vec![
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::Argument),
                    register_root_slot(helper_index, value, BytecodeRootSlotKind::Argument),
                ],
                2,
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let root_map = BytecodeRootMapId(70 + ordinal as u32);
            let code_block = code_block_from_typed_instructions(vec![
                typed_instruction(0, opcode, operands),
                typed_instruction(
                    1,
                    CoreOpcode::Return,
                    vec![Operand::Register(return_register)],
                ),
            ])
            .with_side_tables(LinkedSideTables {
                root_maps: vec![complete_root_map(
                    root_map,
                    Some(owner),
                    helper_index,
                    slots,
                )],
                ..LinkedSideTables::default()
            });

            let derivation =
                derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                    .unwrap();
            let metadata = derivation.metadata.expect("derived helper metadata");
            assert!(baseline_generated_runtime_helper_plan_is_native_exit_eligible(&metadata));
            assert_eq!(metadata.proof_count(), 1);
            let helper = metadata.proof_at(0).unwrap();
            assert_eq!(helper.bytecode_index, helper_index);
            assert_eq!(helper.proof.contract.opcode, opcode);
            assert_eq!(helper.proof.root_map, Some(root_map));
            assert_eq!(helper.proof.root_count, root_count);
            assert_eq!(derivation.safepoints[0].may_allocate, may_allocate);
        }
    }

    #[test]
    fn generated_runtime_helper_derives_typeof_from_code_block_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let source = VirtualRegister::local(1);
        let root_map_id = BytecodeRootMapId(62);
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::VirtualRegister),
                ],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 1);
        let helper = metadata.proof_at(0).unwrap();
        assert_eq!(helper.bytecode_index, helper_index);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::TypeOf);
        assert_eq!(helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 2);
        assert_eq!(
            derivation.safepoints,
            vec![CompilerSafepointDescriptor {
                id: CompilerSafepointId(1),
                owner: Some(owner),
                code: None,
                tier: JitType::Baseline,
                kind: CompilerSafepointKind::Call,
                bytecode_index: Some(helper_index),
                root_map: Some(root_map_id),
                roots: Vec::new(),
                may_call: true,
                may_allocate: true,
            }]
        );

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 1,
                safepoint_count: 1,
                complete_safepoint_root_map_count: 1,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derives_load_string_from_code_block_literal_and_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let literal_key = 7;
        let root_map_id = BytecodeRootMapId(77);
        let code_block = load_string_code_block_with_root_maps(
            destination,
            literal_key,
            vec![(literal_key, "owned literal".to_string())],
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 1);
        let helper = metadata.proof_at(0).unwrap();
        assert_eq!(helper.bytecode_index, helper_index);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::LoadString);
        assert_eq!(helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);
        assert_eq!(
            derivation.safepoints,
            vec![CompilerSafepointDescriptor {
                id: CompilerSafepointId(1),
                owner: Some(owner),
                code: None,
                tier: JitType::Baseline,
                kind: CompilerSafepointKind::Call,
                bytecode_index: Some(helper_index),
                root_map: Some(root_map_id),
                roots: Vec::new(),
                may_call: true,
                may_allocate: true,
            }]
        );

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 1,
                safepoint_count: 1,
                complete_safepoint_root_map_count: 1,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derives_load_bigint_from_code_block_literal_and_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let literal_key = 11;
        let root_map_id = BytecodeRootMapId(82);
        let code_block = load_bigint_code_block_with_root_maps(
            destination,
            literal_key,
            vec![(literal_key, "12345678901234567890n".to_string())],
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 1);
        let helper = metadata.proof_at(0).unwrap();
        assert_eq!(helper.bytecode_index, helper_index);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::LoadBigInt);
        assert_eq!(helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);
        assert_eq!(
            derivation.safepoints,
            vec![CompilerSafepointDescriptor {
                id: CompilerSafepointId(1),
                owner: Some(owner),
                code: None,
                tier: JitType::Baseline,
                kind: CompilerSafepointKind::Call,
                bytecode_index: Some(helper_index),
                root_map: Some(root_map_id),
                roots: Vec::new(),
                may_call: true,
                may_allocate: true,
            }]
        );

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 1,
                safepoint_count: 1,
                complete_safepoint_root_map_count: 1,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derives_mixed_destination_only_family_in_bytecode_order() {
        let owner = baseline_owner();
        let object_index = BytecodeIndex::from_offset(0);
        let array_index = BytecodeIndex::from_offset(1);
        let object_destination = VirtualRegister::local(0);
        let array_destination = VirtualRegister::local(1);
        let object_root_map = BytecodeRootMapId(59);
        let array_root_map = BytecodeRootMapId(60);
        let code_block = mixed_new_object_new_array_code_block_with_root_maps(
            object_destination,
            array_destination,
            vec![
                complete_root_map(
                    array_root_map,
                    Some(owner),
                    array_index,
                    vec![BytecodeRootSlotDescriptor::virtual_register(
                        array_index,
                        array_destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    )],
                ),
                complete_root_map(
                    object_root_map,
                    Some(owner),
                    object_index,
                    vec![BytecodeRootSlotDescriptor::virtual_register(
                        object_index,
                        object_destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    )],
                ),
            ],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 2);
        let object_helper = metadata.proof_at(0).unwrap();
        assert_eq!(object_helper.bytecode_index, object_index);
        assert_eq!(object_helper.proof.contract.opcode, CoreOpcode::NewObject);
        assert_eq!(object_helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(object_helper.proof.root_map, Some(object_root_map));
        let array_helper = metadata.proof_at(1).unwrap();
        assert_eq!(array_helper.bytecode_index, array_index);
        assert_eq!(array_helper.proof.contract.opcode, CoreOpcode::NewArray);
        assert_eq!(array_helper.proof.safepoint, CompilerSafepointId(2));
        assert_eq!(array_helper.proof.root_map, Some(array_root_map));
        assert_eq!(
            derivation
                .safepoints
                .iter()
                .map(|safepoint| safepoint.bytecode_index)
                .collect::<Vec<_>>(),
            vec![Some(object_index), Some(array_index)]
        );

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 2,
                safepoint_count: 2,
                complete_safepoint_root_map_count: 2,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derives_mixed_family_in_bytecode_order() {
        let owner = baseline_owner();
        let object_index = BytecodeIndex::from_offset(0);
        let array_index = BytecodeIndex::from_offset(1);
        let type_of_index = BytecodeIndex::from_offset(2);
        let object_destination = VirtualRegister::local(0);
        let array_destination = VirtualRegister::local(1);
        let type_of_destination = VirtualRegister::local(2);
        let object_root_map = BytecodeRootMapId(73);
        let array_root_map = BytecodeRootMapId(74);
        let type_of_root_map = BytecodeRootMapId(75);
        let code_block = mixed_runtime_helper_code_block_with_root_maps(
            object_destination,
            array_destination,
            type_of_destination,
            vec![
                complete_root_map(
                    array_root_map,
                    Some(owner),
                    array_index,
                    vec![register_root_slot(
                        array_index,
                        array_destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    )],
                ),
                complete_root_map(
                    type_of_root_map,
                    Some(owner),
                    type_of_index,
                    vec![
                        register_root_slot(
                            type_of_index,
                            type_of_destination,
                            BytecodeRootSlotKind::VirtualRegister,
                        ),
                        register_root_slot(
                            type_of_index,
                            array_destination,
                            BytecodeRootSlotKind::VirtualRegister,
                        ),
                    ],
                ),
                complete_root_map(
                    object_root_map,
                    Some(owner),
                    object_index,
                    vec![register_root_slot(
                        object_index,
                        object_destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    )],
                ),
            ],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");

        assert_eq!(metadata.proof_count(), 3);
        let object_helper = metadata.proof_at(0).unwrap();
        assert_eq!(object_helper.bytecode_index, object_index);
        assert_eq!(object_helper.proof.contract.opcode, CoreOpcode::NewObject);
        assert_eq!(object_helper.proof.safepoint, CompilerSafepointId(1));
        assert_eq!(object_helper.proof.root_map, Some(object_root_map));
        let array_helper = metadata.proof_at(1).unwrap();
        assert_eq!(array_helper.bytecode_index, array_index);
        assert_eq!(array_helper.proof.contract.opcode, CoreOpcode::NewArray);
        assert_eq!(array_helper.proof.safepoint, CompilerSafepointId(2));
        assert_eq!(array_helper.proof.root_map, Some(array_root_map));
        let type_of_helper = metadata.proof_at(2).unwrap();
        assert_eq!(type_of_helper.bytecode_index, type_of_index);
        assert_eq!(type_of_helper.proof.contract.opcode, CoreOpcode::TypeOf);
        assert_eq!(type_of_helper.proof.safepoint, CompilerSafepointId(3));
        assert_eq!(type_of_helper.proof.root_map, Some(type_of_root_map));
        assert_eq!(
            derivation
                .safepoints
                .iter()
                .map(|safepoint| safepoint.bytecode_index)
                .collect::<Vec<_>>(),
            vec![Some(object_index), Some(array_index), Some(type_of_index)]
        );

        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &metadata,
            )
            .unwrap();
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 3,
                safepoint_count: 3,
                complete_safepoint_root_map_count: 3,
            }
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_missing_root_map() {
        let owner = baseline_owner();
        let code_block =
            new_object_code_block_with_root_maps(VirtualRegister::local(0), Vec::new());

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointMissingRootMap(
                CompilerSafepointId(1)
            ))
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_incomplete_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let root_map_id = BytecodeRootMapId(51);
        let mut root_map = complete_root_map(
            root_map_id,
            Some(owner),
            helper_index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                helper_index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        root_map.complete = false;
        let code_block =
            new_object_code_block_with_root_maps(VirtualRegister::local(0), vec![root_map]);

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointIncompleteRootMap {
                safepoint: CompilerSafepointId(1),
                root_map: root_map_id,
            })
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_wrong_root_map_owner() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let other_owner = CodeBlockId(CellId(8));
        let root_map_id = BytecodeRootMapId(52);
        let code_block = new_object_code_block_with_root_maps(
            VirtualRegister::local(0),
            vec![complete_root_map(
                root_map_id,
                Some(other_owner),
                helper_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    helper_index,
                    VirtualRegister::local(0),
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointRootMapOwnerMismatch {
                safepoint: CompilerSafepointId(1),
                safepoint_owner: Some(owner),
                root_map_owner: Some(other_owner),
            })
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_non_covering_root_map_range() {
        let owner = baseline_owner();
        let map_index = BytecodeIndex::from_offset(1);
        let root_map_id = BytecodeRootMapId(53);
        let code_block = new_object_code_block_with_root_maps(
            VirtualRegister::local(0),
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                map_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    map_index,
                    VirtualRegister::local(0),
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointMissingRootMap(
                CompilerSafepointId(1)
            ))
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_ambiguous_covering_root_maps() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let slot = BytecodeRootSlotDescriptor::virtual_register(
            helper_index,
            destination,
            BytecodeRootSlotKind::VirtualRegister,
        );
        let code_block = new_object_code_block_with_root_maps(
            destination,
            vec![
                complete_root_map(BytecodeRootMapId(54), Some(owner), helper_index, vec![slot]),
                complete_root_map(BytecodeRootMapId(55), Some(owner), helper_index, vec![slot]),
            ],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationAmbiguousRootMaps {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::NewObject,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_only_source_root_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let source = VirtualRegister::local(1);
        let root_map_id = BytecodeRootMapId(63);
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    source,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::TypeOf,
                    root_map: root_map_id,
                    register: destination,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_only_destination_root_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let source = VirtualRegister::local(1);
        let root_map_id = BytecodeRootMapId(64);
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::TypeOf,
                    root_map: root_map_id,
                    register: source,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_new_array_missing_destination_register() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(0, CoreOpcode::NewArray, Vec::new()),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(destination)]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps: vec![complete_root_map(
                BytecodeRootMapId(61),
                Some(owner),
                helper_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
            ..LinkedSideTables::default()
        });

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::NewArray,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_missing_source_register() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let code_block = type_of_code_block_with_operands(
            vec![Operand::Register(destination)],
            destination,
            vec![complete_root_map(
                BytecodeRootMapId(65),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceRegister {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::TypeOf,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_missing_destination_register() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::local(1);
        let code_block = type_of_code_block_with_operands(
            vec![Operand::SignedImmediate(0), Operand::Register(source)],
            destination,
            vec![complete_root_map(
                BytecodeRootMapId(66),
                Some(owner),
                helper_index,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::VirtualRegister),
                ],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::TypeOf,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_string_missing_destination_register() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let literal_key = 9;
        let code_block = load_string_code_block_with_operands(
            vec![
                Operand::SignedImmediate(0),
                Operand::IdentifierIndex(literal_key),
            ],
            destination,
            vec![(literal_key, "owned".to_string())],
            vec![complete_root_map(
                BytecodeRootMapId(78),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadString,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_string_missing_literal_operand() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let code_block = load_string_code_block_with_operands(
            vec![Operand::Register(destination)],
            destination,
            vec![(9, "owned".to_string())],
            vec![complete_root_map(
                BytecodeRootMapId(79),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingStringLiteralOperand {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadString,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_string_missing_code_block_literal() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let literal_key = 9;
        let code_block = load_string_code_block_with_root_maps(
            destination,
            literal_key,
            Vec::new(),
            vec![complete_root_map(
                BytecodeRootMapId(80),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingCodeBlockStringLiteral {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadString,
                    identifier_index: literal_key,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_missing_destination_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(56);
        let code_block = new_object_code_block_with_root_maps(
            destination,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    helper_index,
                    VirtualRegister::local(1),
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::NewObject,
                    root_map: root_map_id,
                    register: destination,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_string_missing_destination_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(81);
        let literal_key = 9;
        let code_block = load_string_code_block_with_root_maps(
            destination,
            literal_key,
            vec![(literal_key, "owned".to_string())],
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    VirtualRegister::local(1),
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadString,
                    root_map: root_map_id,
                    register: destination,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_bigint_missing_destination_register() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let literal_key = 9;
        let code_block = load_bigint_code_block_with_operands(
            vec![
                Operand::SignedImmediate(0),
                Operand::IdentifierIndex(literal_key),
            ],
            destination,
            vec![(literal_key, "19n".to_string())],
            vec![complete_root_map(
                BytecodeRootMapId(83),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationRegister {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadBigInt,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_bigint_missing_literal_operand() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let code_block = load_bigint_code_block_with_operands(
            vec![Operand::Register(destination)],
            destination,
            vec![(9, "19n".to_string())],
            vec![complete_root_map(
                BytecodeRootMapId(84),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingStringLiteralOperand {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadBigInt,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_bigint_missing_code_block_literal() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let literal_key = 9;
        let code_block = load_bigint_code_block_with_root_maps(
            destination,
            literal_key,
            Vec::new(),
            vec![complete_root_map(
                BytecodeRootMapId(85),
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingCodeBlockStringLiteral {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadBigInt,
                    identifier_index: literal_key,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_bigint_missing_destination_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(86);
        let literal_key = 9;
        let code_block = load_bigint_code_block_with_root_maps(
            destination,
            literal_key,
            vec![(literal_key, "19n".to_string())],
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    VirtualRegister::local(1),
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingDestinationSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadBigInt,
                    root_map: root_map_id,
                    register: destination,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_incomplete_root_map() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::local(1);
        let root_map_id = BytecodeRootMapId(67);
        let mut root_map = complete_root_map(
            root_map_id,
            Some(owner),
            helper_index,
            vec![
                register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                ),
                register_root_slot(helper_index, source, BytecodeRootSlotKind::VirtualRegister),
            ],
        );
        root_map.complete = false;
        let code_block = type_of_code_block_with_root_maps(destination, source, vec![root_map]);

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointIncompleteRootMap {
                safepoint: CompilerSafepointId(1),
                root_map: root_map_id,
            })
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_wrong_root_map_owner() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let other_owner = CodeBlockId(CellId(8));
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::local(1);
        let root_map_id = BytecodeRootMapId(68);
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(other_owner),
                helper_index,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::VirtualRegister),
                ],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointRootMapOwnerMismatch {
                safepoint: CompilerSafepointId(1),
                safepoint_owner: Some(owner),
                root_map_owner: Some(other_owner),
            })
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_non_covering_root_map_range() {
        let owner = baseline_owner();
        let map_index = BytecodeIndex::from_offset(1);
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::local(1);
        let root_map_id = BytecodeRootMapId(69);
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                map_index,
                vec![
                    register_root_slot(
                        map_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(map_index, source, BytecodeRootSlotKind::VirtualRegister),
                ],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(JitPlanValidationError::SafepointMissingRootMap(
                CompilerSafepointId(1)
            ))
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_typeof_ambiguous_covering_root_maps() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let source = VirtualRegister::local(1);
        let slots = vec![
            register_root_slot(
                helper_index,
                destination,
                BytecodeRootSlotKind::VirtualRegister,
            ),
            register_root_slot(helper_index, source, BytecodeRootSlotKind::VirtualRegister),
        ];
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![
                complete_root_map(
                    BytecodeRootMapId(70),
                    Some(owner),
                    helper_index,
                    slots.clone(),
                ),
                complete_root_map(BytecodeRootMapId(71), Some(owner), helper_index, slots),
            ],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationAmbiguousRootMaps {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::TypeOf,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_accepts_typeof_same_register_with_one_root_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let register = VirtualRegister::local(0);
        let root_map_id = BytecodeRootMapId(72);
        let code_block = type_of_code_block_with_root_maps(
            register,
            register,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    register,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");
        let helper = metadata.proof_at(0).unwrap();

        assert_eq!(metadata.proof_count(), 1);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::TypeOf);
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);
    }

    #[test]
    fn generated_runtime_helper_derivation_accepts_typeof_argument_source_root() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let source = argument_including_this(1);
        let root_map_id = BytecodeRootMapId(76);
        let code_block = type_of_code_block_with_root_maps(
            destination,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, source, BytecodeRootSlotKind::Argument),
                ],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");
        let helper = metadata.proof_at(0).unwrap();

        assert_eq!(metadata.proof_count(), 1);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::TypeOf);
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 2);
    }

    #[test]
    fn generated_runtime_helper_derivation_accepts_throw_source_root() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let source = argument_including_this(1);
        let root_map_id = BytecodeRootMapId(77);
        let code_block = throw_code_block_with_root_maps(
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    source,
                    BytecodeRootSlotKind::Argument,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived throw metadata");
        let helper = metadata.proof_at(0).unwrap();

        assert_eq!(metadata.proof_count(), 1);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::Throw);
        assert_eq!(
            helper.proof.contract.category,
            BaselineGeneratedRuntimeBoundaryCategory::ExceptionThrow
        );
        assert!(helper.proof.may_throw);
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);
    }

    #[test]
    fn generated_runtime_helper_derivation_accepts_load_function_capture_roots() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let capture_a = argument_including_this(1);
        let capture_b = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(78);
        let code_block = load_function_code_block_with_root_maps(
            destination,
            14,
            vec![capture_a, capture_b],
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![
                    register_root_slot(
                        helper_index,
                        destination,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                    register_root_slot(helper_index, capture_a, BytecodeRootSlotKind::Argument),
                    register_root_slot(
                        helper_index,
                        capture_b,
                        BytecodeRootSlotKind::VirtualRegister,
                    ),
                ],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");
        let helper = metadata.proof_at(0).unwrap();

        assert_eq!(metadata.proof_count(), 1);
        assert_eq!(helper.proof.contract.opcode, CoreOpcode::LoadFunction);
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 3);
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_load_function_missing_capture_root_slot() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        let capture = VirtualRegister::local(2);
        let root_map_id = BytecodeRootMapId(78);
        let code_block = load_function_code_block_with_root_maps(
            destination,
            14,
            vec![capture],
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::LoadFunction,
                    root_map: root_map_id,
                    register: capture,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_accepts_initialize_global_lexical_source_root() {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let source = argument_including_this(1);
        let root_map_id = BytecodeRootMapId(79);
        let code_block = initialize_global_lexical_code_block_with_root_maps(
            21,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    source,
                    BytecodeRootSlotKind::Argument,
                )],
            )],
        );

        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("derived helper metadata");
        let helper = metadata.proof_at(0).unwrap();

        assert_eq!(metadata.proof_count(), 1);
        assert_eq!(
            helper.proof.contract.opcode,
            CoreOpcode::InitializeGlobalLexical
        );
        assert_eq!(helper.proof.root_map, Some(root_map_id));
        assert_eq!(helper.proof.root_count, 1);
    }

    #[test]
    fn generated_runtime_helper_derivation_rejects_initialize_global_lexical_missing_source_root_slot(
    ) {
        let owner = baseline_owner();
        let helper_index = BytecodeIndex::from_offset(0);
        let source = VirtualRegister::local(2);
        let unrelated = VirtualRegister::local(3);
        let root_map_id = BytecodeRootMapId(80);
        let code_block = initialize_global_lexical_code_block_with_root_maps(
            21,
            source,
            vec![complete_root_map(
                root_map_id,
                Some(owner),
                helper_index,
                vec![register_root_slot(
                    helper_index,
                    unrelated,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
        );

        assert_eq!(
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedRuntimeHelperDerivationMissingSourceSlot {
                    bytecode_index: helper_index,
                    opcode: CoreOpcode::InitializeGlobalLexical,
                    root_map: root_map_id,
                    register: source,
                }
            )
        );
    }

    #[test]
    fn generated_runtime_helper_derivation_skips_bigint_constructor_opcode_without_descriptor() {
        let owner = baseline_owner();
        let code_block =
            helper_classified_code_block_with_root_map(CoreOpcode::LoadBigIntConstructor, owner);
        let derivation =
            derive_baseline_generated_runtime_helper_plan_from_code_block(&code_block, owner)
                .unwrap();

        assert!(derivation.metadata.is_none());
        assert!(derivation.safepoints.is_empty());
    }

    #[test]
    fn generated_runtime_boundary_classifier_rejects_excluded_opcode_classes() {
        for (opcode, reason) in [
            (
                CoreOpcode::GetByName,
                BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess,
            ),
            (
                CoreOpcode::PutByValue,
                BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess,
            ),
            (
                CoreOpcode::InById,
                BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess,
            ),
            (
                CoreOpcode::InByVal,
                BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess,
            ),
            (
                CoreOpcode::Call,
                BaselineGeneratedRuntimeBoundaryRejectionReason::JsCallOrConstructor,
            ),
            (
                CoreOpcode::Construct,
                BaselineGeneratedRuntimeBoundaryRejectionReason::JsCallOrConstructor,
            ),
            (
                CoreOpcode::ConstructSuper,
                BaselineGeneratedRuntimeBoundaryRejectionReason::JsCallOrConstructor,
            ),
            (
                CoreOpcode::PowNumber,
                BaselineGeneratedRuntimeBoundaryRejectionReason::PowNumber,
            ),
            (
                CoreOpcode::LoadObjectConstructor,
                BaselineGeneratedRuntimeBoundaryRejectionReason::FunctionOrConstructor,
            ),
            (
                CoreOpcode::TakeException,
                BaselineGeneratedRuntimeBoundaryRejectionReason::Exception,
            ),
        ] {
            assert_eq!(
                baseline_generated_runtime_boundary_contract(opcode),
                Err(reason)
            );
        }
    }

    #[test]
    fn mixed_baseline_classifies_call_call_with_this_and_construct_as_js_call_handoff() {
        for opcode in [
            CoreOpcode::Call,
            CoreOpcode::CallWithThis,
            CoreOpcode::Construct,
        ] {
            let record = baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, opcode),
                baseline_instruction(2, CoreOpcode::Return),
            ]);

            let sites = record.validate_mixed_bytecode_sites(None).unwrap();

            assert_eq!(
                sites,
                vec![
                    BaselineMixedBytecodeSite {
                        bytecode_index: BytecodeIndex::from_offset(0),
                        opcode: CoreOpcode::LoadInt32,
                        kind: BaselineMixedBytecodeSiteKind::Generated,
                    },
                    BaselineMixedBytecodeSite {
                        bytecode_index: BytecodeIndex::from_offset(1),
                        opcode,
                        kind: BaselineMixedBytecodeSiteKind::JsCallHandoff,
                    },
                    BaselineMixedBytecodeSite {
                        bytecode_index: BytecodeIndex::from_offset(2),
                        opcode: CoreOpcode::Return,
                        kind: BaselineMixedBytecodeSiteKind::Generated,
                    },
                ]
            );
            assert!(!record.opcode_subset.supports(opcode));
            assert_eq!(
                baseline_generated_runtime_boundary_contract(opcode),
                Err(BaselineGeneratedRuntimeBoundaryRejectionReason::JsCallOrConstructor)
            );
        }
    }

    #[test]
    fn mixed_baseline_classifies_named_and_by_value_load_store_as_property_handoff() {
        let record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadInt32),
            baseline_instruction(1, CoreOpcode::GetByName),
            baseline_instruction(2, CoreOpcode::PutByName),
            baseline_instruction(3, CoreOpcode::PutGlobalObjectProperty),
            baseline_instruction(4, CoreOpcode::GetByValue),
            baseline_instruction(5, CoreOpcode::PutByValue),
            baseline_instruction(6, CoreOpcode::Return),
        ]);

        let sites = record.validate_mixed_bytecode_sites(None).unwrap();

        assert_eq!(
            sites,
            vec![
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::LoadInt32,
                    kind: BaselineMixedBytecodeSiteKind::Generated,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::GetByName,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(2),
                    opcode: CoreOpcode::PutByName,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(3),
                    opcode: CoreOpcode::PutGlobalObjectProperty,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(4),
                    opcode: CoreOpcode::GetByValue,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(5),
                    opcode: CoreOpcode::PutByValue,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(6),
                    opcode: CoreOpcode::Return,
                    kind: BaselineMixedBytecodeSiteKind::Generated,
                },
            ]
        );
        assert!(!record.opcode_subset.supports(CoreOpcode::GetByName));
        assert_eq!(
            baseline_generated_runtime_boundary_contract(CoreOpcode::GetByName),
            Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess)
        );
        assert!(!record.opcode_subset.supports(CoreOpcode::PutByName));
        assert_eq!(
            baseline_generated_runtime_boundary_contract(CoreOpcode::PutByName),
            Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess)
        );
        assert!(!record
            .opcode_subset
            .supports(CoreOpcode::PutGlobalObjectProperty));
        assert_eq!(
            baseline_generated_runtime_boundary_contract(CoreOpcode::PutGlobalObjectProperty),
            Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess)
        );
        assert!(!record.opcode_subset.supports(CoreOpcode::GetByValue));
        assert_eq!(
            baseline_generated_runtime_boundary_contract(CoreOpcode::GetByValue),
            Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess)
        );
        assert!(!record.opcode_subset.supports(CoreOpcode::PutByValue));
        assert_eq!(
            baseline_generated_runtime_boundary_contract(CoreOpcode::PutByValue),
            Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess)
        );
    }

    #[test]
    fn mixed_baseline_routes_in_opcodes_to_property_handoff() {
        let record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadInt32),
            baseline_instruction(1, CoreOpcode::InById),
            baseline_instruction(2, CoreOpcode::InByVal),
            baseline_instruction(3, CoreOpcode::GetLength),
            baseline_instruction(4, CoreOpcode::Return),
        ]);

        let sites = record.validate_mixed_bytecode_sites(None).unwrap();

        assert_eq!(
            sites,
            vec![
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode: CoreOpcode::LoadInt32,
                    kind: BaselineMixedBytecodeSiteKind::Generated,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::InById,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(2),
                    opcode: CoreOpcode::InByVal,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(3),
                    opcode: CoreOpcode::GetLength,
                    kind: BaselineMixedBytecodeSiteKind::PropertyHandoff,
                },
                BaselineMixedBytecodeSite {
                    bytecode_index: BytecodeIndex::from_offset(4),
                    opcode: CoreOpcode::Return,
                    kind: BaselineMixedBytecodeSiteKind::Generated,
                },
            ]
        );
        for opcode in [
            CoreOpcode::InById,
            CoreOpcode::InByVal,
            CoreOpcode::GetLength,
        ] {
            assert!(!record.opcode_subset.supports(opcode));
            assert_eq!(
                baseline_opcode_rejection_reason(opcode),
                BaselineOpcodeRejectionReason::PropertyAccess
            );
            assert_eq!(
                baseline_generated_runtime_boundary_contract(opcode),
                Err(BaselineGeneratedRuntimeBoundaryRejectionReason::PropertyAccess)
            );
        }
    }

    #[test]
    fn generated_property_handoff_derives_get_by_name_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("property handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::GetByName);
        assert_eq!(site.cache_kind, InlineCacheKind::PropertyLoad);
        assert_eq!(site.access, PropertyAccessType::GetById);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::GetById);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::Key(identifier_property_key(17))
        );
        assert_eq!(site.fallback, InlineCacheFallbackSemantics::SlowPathLookup);
        assert_eq!(site.cold_miss_handoff.owner, owner);
        assert_eq!(site.cold_miss_handoff.slot, site.slot);
        assert_eq!(
            site.cold_miss_handoff.bytecode_index,
            site.bytecode_index.offset()
        );
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::PropertyLoad
        );
        assert_eq!(site.cold_miss_handoff.miss_kind, InlineCacheMissKind::Cold);
        assert_eq!(
            site.cold_miss_handoff.fallback,
            InlineCacheFallbackSemantics::SlowPathLookup
        );
        assert_eq!(site.cold_miss_handoff.boundary, None);
        assert_eq!(site.cold_miss_handoff.call_link, None);
        assert!(site.cold_miss_handoff.preserves_operand_registers);
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_get_length_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = get_length_code_block(17);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("get length handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::GetLength);
        assert_eq!(site.cache_kind, InlineCacheKind::PropertyLoad);
        assert_eq!(site.access, PropertyAccessType::GetById);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::GetById);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::Key(identifier_property_key(17))
        );
        assert_eq!(site.fallback, InlineCacheFallbackSemantics::SlowPathLookup);
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::PropertyLoad
        );
        assert_eq!(site.cold_miss_handoff.miss_kind, InlineCacheMissKind::Cold);
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_put_by_name_store_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = put_by_name_code_block(19);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation
            .metadata
            .expect("property store handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::PutByName);
        assert_eq!(site.cache_kind, InlineCacheKind::PropertyStore);
        assert_eq!(site.access, PropertyAccessType::PutByIdSloppy);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::PutById);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::Key(identifier_property_key(19))
        );
        assert_eq!(site.fallback, InlineCacheFallbackSemantics::SlowPathLookup);
        assert_eq!(site.cold_miss_handoff.owner, owner);
        assert_eq!(site.cold_miss_handoff.slot, site.slot);
        assert_eq!(
            site.cold_miss_handoff.bytecode_index,
            site.bytecode_index.offset()
        );
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::PropertyStore
        );
        assert_eq!(site.cold_miss_handoff.miss_kind, InlineCacheMissKind::Cold);
        assert_eq!(
            site.cold_miss_handoff.fallback,
            InlineCacheFallbackSemantics::SlowPathLookup
        );
        assert_eq!(site.cold_miss_handoff.boundary, None);
        assert_eq!(site.cold_miss_handoff.call_link, None);
        assert!(site.cold_miss_handoff.preserves_operand_registers);
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_put_global_object_property_store_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = put_global_object_property_code_block(23);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation
            .metadata
            .expect("global property store handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::PutGlobalObjectProperty);
        assert_eq!(site.cache_kind, InlineCacheKind::PropertyStore);
        assert_eq!(site.access, PropertyAccessType::PutByIdSloppy);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::PutById);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::Key(identifier_property_key(23))
        );
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::PropertyStore
        );
        assert_eq!(site.cold_miss_handoff.miss_kind, InlineCacheMissKind::Cold);
        assert_eq!(
            site.cold_miss_handoff.fallback,
            InlineCacheFallbackSemantics::SlowPathLookup
        );
        assert!(site.cold_miss_handoff.preserves_operand_registers);
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_get_global_object_property_load_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = get_global_object_property_code_block(23);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation
            .metadata
            .expect("global property load handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::GetGlobalObjectProperty);
        assert_eq!(site.cache_kind, InlineCacheKind::PropertyLoad);
        assert_eq!(site.access, PropertyAccessType::GetById);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::GetById);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::Key(identifier_property_key(23))
        );
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::PropertyLoad
        );
        assert_eq!(site.cold_miss_handoff.miss_kind, InlineCacheMissKind::Cold);
        assert_eq!(
            site.cold_miss_handoff.fallback,
            InlineCacheFallbackSemantics::SlowPathLookup
        );
        assert!(site.cold_miss_handoff.preserves_operand_registers);
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_get_by_value_element_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = get_by_value_code_block();

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("element load handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::GetByValue);
        assert_eq!(site.cache_kind, InlineCacheKind::ElementLoad);
        assert_eq!(site.access, PropertyAccessType::GetByVal);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::GetByVal);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(3))
        );
        assert_eq!(site.fallback, InlineCacheFallbackSemantics::SlowPathLookup);
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::ElementLoad
        );
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_put_by_value_element_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = put_by_value_code_block();

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("element store handoff metadata");
        let site = metadata.site_at(0).unwrap();

        assert_eq!(metadata.site_count(), 1);
        assert_eq!(site.owner, owner);
        assert_eq!(site.slot, InlineCacheSlotId(0));
        assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(site.opcode, CoreOpcode::PutByValue);
        assert_eq!(site.cache_kind, InlineCacheKind::ElementStore);
        assert_eq!(site.access, PropertyAccessType::PutByValSloppy);
        assert_eq!(site.property_cache_kind, PropertyCacheKind::PutByVal);
        assert_eq!(
            site.property_key,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(3))
        );
        assert_eq!(site.fallback, InlineCacheFallbackSemantics::SlowPathLookup);
        assert_eq!(
            site.cold_miss_handoff.cache_kind,
            InlineCacheKind::ElementStore
        );
        assert_eq!(site.cold_miss_handoff.validate(), Ok(()));
    }

    #[test]
    fn generated_property_handoff_derives_in_by_has_ic_site_metadata() {
        let owner = baseline_owner();
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::IdentifierIndex(31),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::Register(VirtualRegister::local(3)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(2))],
            ),
        ]);
        let property_accesses = &code_block.side_tables().inline_caches.property_accesses;

        assert_eq!(property_accesses.len(), 2);
        assert_eq!(property_accesses[0].access, PropertyAccessType::InById);
        assert_eq!(property_accesses[0].kind, PropertyCacheKind::InById);
        assert_eq!(property_accesses[1].access, PropertyAccessType::InByVal);
        assert_eq!(property_accesses[1].kind, PropertyCacheKind::InByVal);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("in has handoff metadata");
        assert_eq!(metadata.site_count(), 2);
        let by_id = metadata.site_at(0).unwrap();
        assert_eq!(by_id.owner, owner);
        assert_eq!(by_id.slot, InlineCacheSlotId(0));
        assert_eq!(by_id.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(by_id.opcode, CoreOpcode::InById);
        assert_eq!(by_id.cache_kind, InlineCacheKind::HasProperty);
        assert_eq!(by_id.access, PropertyAccessType::InById);
        assert_eq!(by_id.property_cache_kind, PropertyCacheKind::InById);
        assert_eq!(
            by_id.property_key,
            PropertyCacheKey::Key(identifier_property_key(31))
        );
        assert_eq!(
            by_id.cold_miss_handoff.cache_kind,
            InlineCacheKind::HasProperty
        );
        assert_eq!(by_id.cold_miss_handoff.miss_kind, InlineCacheMissKind::Cold);
        assert_eq!(
            by_id.cold_miss_handoff.fallback,
            InlineCacheFallbackSemantics::SlowPathLookup
        );
        assert!(by_id.cold_miss_handoff.preserves_operand_registers);
        assert_eq!(by_id.cold_miss_handoff.validate(), Ok(()));

        let by_val = metadata.site_at(1).unwrap();
        assert_eq!(by_val.owner, owner);
        assert_eq!(by_val.slot, InlineCacheSlotId(1));
        assert_eq!(by_val.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(by_val.opcode, CoreOpcode::InByVal);
        assert_eq!(by_val.cache_kind, InlineCacheKind::HasProperty);
        assert_eq!(by_val.access, PropertyAccessType::InByVal);
        assert_eq!(by_val.property_cache_kind, PropertyCacheKind::InByVal);
        assert_eq!(
            by_val.property_key,
            PropertyCacheKey::RuntimeValue(VirtualRegister::local(3))
        );
        assert_eq!(
            by_val.cold_miss_handoff.cache_kind,
            InlineCacheKind::HasProperty
        );
        assert_eq!(by_val.cold_miss_handoff.validate(), Ok(()));

        let current_derivation =
            derive_baseline_generated_property_handoff_plan_from_current_code_block_metadata(
                &code_block,
                owner,
            )
            .unwrap();
        assert_eq!(current_derivation.metadata, Some(metadata));
    }

    #[test]
    fn generated_property_handoff_metadata_scales_past_inline_site_count() {
        let owner = baseline_owner();
        let code_block = many_get_by_name_code_block(20);

        let derivation =
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("property handoff metadata");

        assert_eq!(metadata.site_count(), 20);
        for index in 0..20 {
            let bytecode_index = BytecodeIndex::from_offset(index + 1);
            let site = metadata
                .site_for_bytecode_index(bytecode_index)
                .expect("property handoff site");
            assert_eq!(site.owner, owner);
            assert_eq!(site.slot, InlineCacheSlotId(index));
            assert_eq!(site.bytecode_index, bytecode_index);
            assert_eq!(site.opcode, CoreOpcode::GetByName);
            assert_eq!(
                site.property_key,
                PropertyCacheKey::Key(identifier_property_key(100 + index))
            );
        }
        assert!(metadata
            .site_for_bytecode_index(BytecodeIndex::from_offset(25))
            .is_none());
    }

    #[test]
    fn generated_property_handoff_rejects_get_by_name_without_bytecode_ic_site() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let mut side_tables = code_block.side_tables().clone();
        side_tables.inline_caches.property_accesses.clear();
        let code_block = code_block.with_side_tables(side_tables);

        assert_eq!(
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissingBytecodeCache {
                    bytecode_index: BytecodeIndex::from_offset(1),
                }
            )
        );
    }

    #[test]
    fn generated_property_handoff_rejects_duplicate_get_by_name_bytecode_ic_sites() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let mut side_tables = code_block.side_tables().clone();
        let duplicate = side_tables.inline_caches.property_accesses[0].clone();
        side_tables.inline_caches.property_accesses.push(duplicate);
        let code_block = code_block.with_side_tables(side_tables);

        assert_eq!(
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanDuplicateBytecodeCache {
                    bytecode_index: BytecodeIndex::from_offset(1),
                }
            )
        );
    }

    #[test]
    fn generated_property_handoff_rejects_warmed_bytecode_ic_state() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let mut side_tables = code_block.side_tables().clone();
        side_tables.inline_caches.property_accesses[0].state =
            BytecodeInlineCacheState::Monomorphic;
        let code_block = code_block.with_side_tables(side_tables);

        assert_eq!(
            derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanBytecodeCacheStateMismatch {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    expected: BytecodeInlineCacheState::Unset,
                    actual: BytecodeInlineCacheState::Monomorphic,
                }
            )
        );
    }

    #[test]
    fn generated_property_handoff_current_metadata_accepts_warmed_named_and_global_load_ics() {
        let owner = baseline_owner();
        for (opcode, code_block) in [
            (CoreOpcode::GetByName, get_by_name_code_block(17)),
            (
                CoreOpcode::GetGlobalObjectProperty,
                get_global_object_property_code_block(23),
            ),
        ] {
            let mut side_tables = code_block.side_tables().clone();
            let cache = &mut side_tables.inline_caches.property_accesses[0];
            cache.state = BytecodeInlineCacheState::Monomorphic;
            cache.dispatch = PropertyInlineCacheDispatch::Handler;
            cache.mutation_authority = InlineCacheMutationAuthority::BaselineJit;
            let code_block = code_block.with_side_tables(side_tables);

            assert!(
                derive_baseline_generated_property_handoff_plan_from_code_block(&code_block, owner)
                    .is_err(),
                "cold generated-code install must reject warmed {:?} ICs",
                opcode
            );

            let derivation =
                derive_baseline_generated_property_handoff_plan_from_current_code_block_metadata(
                    &code_block,
                    owner,
                )
                .unwrap();
            let metadata = derivation
                .metadata
                .expect("current property handoff metadata");
            let site = metadata.site_at(0).unwrap();

            assert_eq!(metadata.site_count(), 1);
            assert_eq!(site.owner, owner);
            assert_eq!(site.bytecode_index, BytecodeIndex::from_offset(1));
            assert_eq!(site.opcode, opcode);
        }
    }

    #[test]
    fn generated_property_handoff_rejects_mismatched_put_by_name_store_metadata() {
        let owner = baseline_owner();
        let code_block = put_by_name_code_block(19);
        let mut side_tables = code_block.side_tables().clone();
        side_tables.inline_caches.property_accesses[0].kind = PropertyCacheKind::GetById;
        let mismatched_cache_kind = code_block.clone().with_side_tables(side_tables);

        assert_eq!(
            derive_baseline_generated_property_handoff_plan_from_code_block(
                &mismatched_cache_kind,
                owner
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanPropertyCacheKindMismatch {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    expected: PropertyCacheKind::PutById,
                    actual: PropertyCacheKind::GetById,
                }
            )
        );

        let mut site = put_property_handoff_site(owner, BytecodeIndex::from_offset(1), 19);
        site.cache_kind = InlineCacheKind::PropertyLoad;

        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCacheKindMismatch {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    expected: InlineCacheKind::PropertyStore,
                    actual: InlineCacheKind::PropertyLoad,
                }
            )
        );
    }

    #[test]
    fn explicit_property_handoff_metadata_rejects_stale_snapshot_and_mismatched_key() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let stale_code_block = get_by_name_code_block(18);
        let site = property_handoff_site(owner, BytecodeIndex::from_offset(1), 17);
        let stale_snapshot =
            baseline_bytecode_snapshot_fingerprint_from_code_block(&stale_code_block).unwrap();
        let stale_metadata =
            BaselineGeneratedPropertyHandoffPlanMetadata::new(stale_snapshot, vec![site]).unwrap();

        assert_eq!(
            validate_baseline_generated_property_handoff_plan_against_code_block(
                &code_block,
                owner,
                &stale_metadata,
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
                    expected_site_count: 1,
                    actual_site_count: 1,
                    first_mismatch: None,
                    bytecode_snapshot_matches: Some(false),
                }
            )
        );

        let mut wrong_key = site;
        wrong_key.property_key = PropertyCacheKey::Key(identifier_property_key(18));
        let wrong_key_metadata =
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![wrong_key],
            )
            .unwrap();

        assert_eq!(
            validate_baseline_generated_property_handoff_plan_against_code_block(
                &code_block,
                owner,
                &wrong_key_metadata,
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
                    expected_site_count: 1,
                    actual_site_count: 1,
                    first_mismatch: Some(0),
                    bytecode_snapshot_matches: Some(true),
                }
            )
        );
    }

    #[test]
    fn explicit_property_handoff_metadata_rejects_bad_opcode_and_cache_kind() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let mut site = property_handoff_site(owner, BytecodeIndex::from_offset(1), 17);
        site.opcode = CoreOpcode::DeleteByName;

        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::DeleteByName,
                }
            )
        );

        let mut site = property_handoff_site(owner, BytecodeIndex::from_offset(1), 17);
        site.cache_kind = InlineCacheKind::ElementLoad;

        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCacheKindMismatch {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    expected: InlineCacheKind::PropertyLoad,
                    actual: InlineCacheKind::ElementLoad,
                }
            )
        );
    }

    #[test]
    fn explicit_property_handoff_metadata_rejects_malformed_miss_handoff_descriptor() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let bytecode_index = BytecodeIndex::from_offset(1);
        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.owner = CodeBlockId(CellId(8));
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffOwnerMismatch {
                    bytecode_index,
                    expected: owner,
                    actual: CodeBlockId(CellId(8)),
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.slot = InlineCacheSlotId(99);
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffSlotMismatch {
                    bytecode_index,
                    expected: InlineCacheSlotId(0),
                    actual: InlineCacheSlotId(99),
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.bytecode_index = 99;
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffBytecodeIndexMismatch {
                    bytecode_index,
                    expected: bytecode_index.offset(),
                    actual: 99,
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.cache_kind = InlineCacheKind::ElementLoad;
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffCacheKindMismatch {
                    bytecode_index,
                    expected: InlineCacheKind::PropertyLoad,
                    actual: InlineCacheKind::ElementLoad,
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.miss_kind = InlineCacheMissKind::CaseMiss;
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffMissKindMismatch {
                    bytecode_index,
                    expected: InlineCacheMissKind::Cold,
                    actual: InlineCacheMissKind::CaseMiss,
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.fallback = InlineCacheFallbackSemantics::MegamorphicGeneric;
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffFallbackMismatch {
                    bytecode_index,
                    expected: InlineCacheFallbackSemantics::SlowPathLookup,
                    actual: InlineCacheFallbackSemantics::MegamorphicGeneric,
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.boundary = Some(CallBoundaryId(77));
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffBoundaryMismatch {
                    bytecode_index,
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.call_link = Some(CallLinkInfoDescriptor {
            mode: CallLinkMode::Init,
            call_kind: LinkedCallKind::Call,
            owner: Some(owner),
            executable: None,
            callee: None,
            target_code_block: None,
            boundary: None,
            slow_path_count: 0,
            max_argument_count_including_this: 1,
        });
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffCallLinkMismatch {
                    bytecode_index,
                }
            )
        );

        let mut site = property_handoff_site(owner, bytecode_index, 17);
        site.cold_miss_handoff.preserves_operand_registers = false;
        assert_eq!(
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanMissHandoffPreservesOperandRegistersMismatch {
                    bytecode_index,
                }
            )
        );
    }

    #[test]
    fn explicit_property_handoff_metadata_rejects_owner_that_differs_from_code_block_owner() {
        let owner = baseline_owner();
        let code_block = get_by_name_code_block(17);
        let wrong_owner = CodeBlockId(CellId(8));
        let wrong_owner_metadata =
            BaselineGeneratedPropertyHandoffPlanMetadata::from_code_block_snapshot(
                &code_block,
                vec![property_handoff_site(
                    wrong_owner,
                    BytecodeIndex::from_offset(1),
                    17,
                )],
            )
            .unwrap();

        assert_eq!(
            validate_baseline_generated_property_handoff_plan_against_code_block(
                &code_block,
                owner,
                &wrong_owner_metadata,
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedPropertyHandoffPlanCodeBlockDerivationMismatch {
                    expected_site_count: 1,
                    actual_site_count: 1,
                    first_mismatch: Some(0),
                    bytecode_snapshot_matches: Some(true),
                }
            )
        );
    }

    #[test]
    fn generated_property_handoff_derivation_ignores_other_property_ops() {
        for opcode in [
            CoreOpcode::PutByIndex,
            CoreOpcode::DeleteByName,
            CoreOpcode::ArrayLength,
        ] {
            let code_block = code_block_from_typed_instructions(vec![
                typed_instruction(
                    0,
                    opcode,
                    vec![
                        Operand::Register(VirtualRegister::local(0)),
                        Operand::Register(VirtualRegister::local(1)),
                        Operand::IdentifierIndex(17),
                    ],
                ),
                typed_instruction(
                    1,
                    CoreOpcode::Return,
                    vec![Operand::Register(VirtualRegister::local(0))],
                ),
            ]);

            let derivation = derive_baseline_generated_property_handoff_plan_from_code_block(
                &code_block,
                baseline_owner(),
            )
            .unwrap();

            assert!(derivation.metadata.is_none(), "{opcode:?}");
        }
    }

    #[test]
    fn mixed_baseline_does_not_promote_other_call_or_property_opcodes_to_typed_handoffs() {
        for opcode in [
            CoreOpcode::CallDirect,
            CoreOpcode::ConstructSuper,
            CoreOpcode::PutByIndex,
            CoreOpcode::DeleteByName,
            CoreOpcode::DeleteByValue,
            CoreOpcode::ArrayLength,
        ] {
            let record = baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, opcode),
                baseline_instruction(2, CoreOpcode::Return),
            ]);
            let sites = record.validate_mixed_bytecode_sites(None).unwrap();

            assert_eq!(
                sites[1].kind,
                BaselineMixedBytecodeSiteKind::InterpreterFallback
            );
            assert!(!record.opcode_subset.supports(opcode));
        }
    }

    #[test]
    fn mixed_baseline_accepts_js_call_handoff_without_generated_prefix_or_helper_site() {
        for opcode in [
            CoreOpcode::Call,
            CoreOpcode::CallWithThis,
            CoreOpcode::Construct,
        ] {
            let record = baseline_eligibility_record(vec![baseline_instruction(0, opcode)]);
            let sites = record.validate_mixed_bytecode_sites(None).unwrap();
            assert_eq!(sites.len(), 1);
            assert_eq!(sites[0].kind, BaselineMixedBytecodeSiteKind::JsCallHandoff);
            assert!(record.validate_mixed_for_vm_install(None).is_ok());
        }
    }

    #[test]
    fn generated_js_call_native_exit_metadata_scales_past_inline_argument_register_count() {
        let owner = baseline_owner();
        let code_block = many_construct_code_block(20);

        let derivation =
            derive_baseline_generated_js_call_native_exit_plan_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("construct handoff metadata");

        assert_eq!(metadata.site_count(), 20);
        for index in 0..20 {
            let bytecode_index = BytecodeIndex::from_offset(index);
            let site = metadata
                .site_for_bytecode_index(bytecode_index)
                .expect("construct handoff site");
            assert_eq!(site.owner, owner);
            assert_eq!(site.bytecode_index, bytecode_index);
            assert_eq!(site.opcode, CoreOpcode::Construct);
            assert_eq!(site.destination, VirtualRegister::local(index + 1));
            assert_eq!(site.callee, VirtualRegister::local(0));
            assert_eq!(site.provided_argument_count, 0);
            assert!(site.argument_registers.is_empty());
        }
    }

    #[test]
    fn baseline_generated_owner_continuation_map_derives_call_call_with_this_and_construct_sites() {
        let owner = baseline_owner();
        let code_block = owner_continuation_code_block();

        let derivation =
            derive_baseline_generated_owner_continuation_map_from_code_block(&code_block, owner)
                .unwrap();
        let metadata = derivation.metadata.expect("owner continuation map");

        assert_eq!(metadata.label_count(), 5);
        assert_eq!(metadata.call_site_count(), 3);
        assert_eq!(
            metadata.label_for_bytecode_index(BytecodeIndex::from_offset(1)),
            Some(&BaselineGeneratedOwnerBytecodeLabel {
                owner,
                bytecode_index: BytecodeIndex::from_offset(1),
                opcode: CoreOpcode::Call,
                next_bytecode_index: Some(BytecodeIndex::from_offset(2)),
            })
        );

        let call = metadata.call_site_at(0).unwrap();
        assert_eq!(call.owner, owner);
        assert_eq!(call.call_bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(call.opcode, CoreOpcode::Call);
        assert_eq!(call.destination, VirtualRegister::local(1));
        assert_eq!(call.argument_count_including_this, 3);
        assert_eq!(
            call.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(2))
        );
        assert_eq!(call.kind, BaselineGeneratedOwnerContinuationKind::Call);
        let call_profile = call.result_profile.expect("Call result profile site");
        assert_eq!(call_profile.profile_slot, RuntimeSlot(1));
        assert_eq!(call_profile.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(call_profile.checkpoint, Checkpoint::NONE);
        assert_eq!(call_profile.bucket_kind, ValueProfileBucketKind::Sample);
        assert_eq!(
            call_profile.storage_generation,
            ValueProfileJitStorageGeneration(1)
        );
        assert_eq!(call_profile.value_profile_offset, 1);
        assert_eq!(call_profile.metadata_table_displacement, -32);
        assert_ne!(call_profile.metadata_table_base_address, 0);
        assert_eq!(
            call_profile
                .metadata_table_base_address
                .checked_sub((-call_profile.metadata_table_displacement) as usize),
            Some(call_profile.raw_bucket_address)
        );
        assert_ne!(call_profile.raw_bucket_address, 0);
        assert_eq!(call_profile.raw_bucket_bytes, 8);
        assert_eq!(
            call_profile.emission_policy,
            ValueProfileEmissionPolicy::default()
        );

        let call_with_this = metadata.call_site_at(1).unwrap();
        assert_eq!(call_with_this.opcode, CoreOpcode::CallWithThis);
        assert_eq!(call_with_this.destination, VirtualRegister::local(5));
        assert_eq!(call_with_this.argument_count_including_this, 2);
        assert_eq!(
            call_with_this.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(3))
        );
        assert_eq!(
            metadata.call_site_for_bytecode_index(BytecodeIndex::from_offset(2)),
            Some(call_with_this)
        );
        let call_with_this_profile = call_with_this
            .result_profile
            .expect("CallWithThis result profile site");
        assert_eq!(call_with_this_profile.profile_slot, RuntimeSlot(2));
        assert_eq!(
            call_with_this_profile.bytecode_index,
            BytecodeIndex::from_offset(2)
        );
        assert_eq!(call_with_this_profile.checkpoint, Checkpoint::NONE);
        assert_eq!(
            call_with_this_profile.bucket_kind,
            ValueProfileBucketKind::Sample
        );
        assert_eq!(
            call_with_this_profile.storage_generation,
            ValueProfileJitStorageGeneration(1)
        );
        assert_eq!(call_with_this_profile.value_profile_offset, 2);
        assert_eq!(call_with_this_profile.metadata_table_displacement, -48);
        assert_ne!(call_with_this_profile.metadata_table_base_address, 0);
        assert_eq!(
            call_with_this_profile
                .metadata_table_base_address
                .checked_sub((-call_with_this_profile.metadata_table_displacement) as usize),
            Some(call_with_this_profile.raw_bucket_address)
        );
        assert_ne!(call_with_this_profile.raw_bucket_address, 0);
        assert_eq!(call_with_this_profile.raw_bucket_bytes, 8);
        assert_eq!(
            call_with_this_profile.emission_policy,
            ValueProfileEmissionPolicy::default()
        );

        let construct = metadata.call_site_at(2).unwrap();
        assert_eq!(construct.opcode, CoreOpcode::Construct);
        assert_eq!(construct.destination, VirtualRegister::local(9));
        assert_eq!(construct.argument_count_including_this, 1);
        assert_eq!(
            construct.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(4))
        );
        assert_eq!(
            construct.kind,
            BaselineGeneratedOwnerContinuationKind::Construct
        );
        assert_eq!(construct.result_profile, None);
    }

    #[test]
    fn baseline_generated_owner_continuation_map_rejects_duplicate_or_malformed_sites() {
        let owner = baseline_owner();
        let code_block = owner_continuation_code_block();
        let snapshot = baseline_bytecode_snapshot_fingerprint_from_code_block(&code_block).unwrap();
        let label = BaselineGeneratedOwnerBytecodeLabel {
            owner,
            bytecode_index: BytecodeIndex::from_offset(1),
            opcode: CoreOpcode::Call,
            next_bytecode_index: Some(BytecodeIndex::from_offset(2)),
        };
        let site = BaselineGeneratedOwnerContinuationSite {
            owner,
            call_bytecode_index: BytecodeIndex::from_offset(1),
            opcode: CoreOpcode::Call,
            destination: VirtualRegister::local(1),
            argument_count_including_this: 1,
            resume_bytecode_index: Some(BytecodeIndex::from_offset(2)),
            kind: BaselineGeneratedOwnerContinuationKind::Call,
            result_profile: Some(BaselineGeneratedOwnerCallResultProfileSite {
                profile_slot: RuntimeSlot(1),
                bytecode_index: BytecodeIndex::from_offset(1),
                checkpoint: Checkpoint::NONE,
                bucket_kind: ValueProfileBucketKind::Sample,
                storage_generation: ValueProfileJitStorageGeneration(1),
                value_profile_offset: 1,
                metadata_table_displacement: -32,
                metadata_table_base_address: 33,
                raw_bucket_address: 1,
                raw_bucket_bytes: 8,
                emission_policy: ValueProfileEmissionPolicy::default(),
            }),
        };

        assert_eq!(
            BaselineGeneratedOwnerContinuationMapMetadata::new(
                snapshot,
                vec![label],
                vec![site, site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapDuplicateSite {
                    bytecode_index: BytecodeIndex::from_offset(1),
                }
            )
        );

        assert_eq!(
            BaselineGeneratedOwnerContinuationMapMetadata::new(
                snapshot,
                vec![label, label],
                vec![],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapDuplicateLabel {
                    bytecode_index: BytecodeIndex::from_offset(1),
                }
            )
        );

        let malformed_site = BaselineGeneratedOwnerContinuationSite {
            opcode: CoreOpcode::Return,
            ..site
        };
        assert_eq!(
            BaselineGeneratedOwnerContinuationMapMetadata::new(
                snapshot,
                vec![label],
                vec![malformed_site],
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Return,
                }
            )
        );
    }

    #[test]
    fn baseline_generated_owner_continuation_map_validation_rejects_stale_code_block_snapshot() {
        let owner = baseline_owner();
        let code_block = owner_continuation_code_block();
        let stale_code_block = owner_continuation_code_block_with_wider_first_call();
        let stale_metadata = derive_baseline_generated_owner_continuation_map_from_code_block(
            &stale_code_block,
            owner,
        )
        .unwrap()
        .metadata
        .expect("stale owner continuation map");

        assert_eq!(
            validate_baseline_generated_owner_continuation_map_against_code_block(
                &code_block,
                owner,
                &stale_metadata,
            ),
            Err(
                JitPlanValidationError::BaselineGeneratedOwnerContinuationMapCodeBlockDerivationMismatch {
                    expected_label_count: 5,
                    actual_label_count: 5,
                    first_label_mismatch: None,
                    expected_site_count: 3,
                    actual_site_count: 3,
                    first_site_mismatch: Some(0),
                    bytecode_snapshot_matches: Some(false),
                }
            )
        );
    }

    #[test]
    fn mixed_baseline_rejects_property_handoff_without_generated_prefix_or_helper_site() {
        for opcode in [CoreOpcode::GetByName, CoreOpcode::PutByName] {
            assert_eq!(
                baseline_eligibility_record(vec![baseline_instruction(0, opcode)])
                    .validate_mixed_for_vm_install(None),
                Err(JitPlanValidationError::BaselineEligibilityNoGeneratedOrRuntimeHelperSites)
            );
        }
    }

    #[test]
    fn baseline_bytecode_eligibility_extracts_real_code_block_subset() {
        let owner = baseline_owner();
        let code_block = code_block_from_core_opcodes(&[
            CoreOpcode::LoadInt32,
            CoreOpcode::Move,
            CoreOpcode::SubInt32,
            CoreOpcode::MulInt32,
            CoreOpcode::Return,
        ]);

        let proof = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner,
            baseline_tiering_snapshot(owner),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            proof.bytecode(),
            BaselineBytecodeRange {
                start: BytecodeIndex::from_offset(0),
                end: BytecodeIndex::from_offset(4),
                instruction_count: 5,
            }
        );
        assert_eq!(
            proof.exception_metadata(),
            BaselineExceptionMetadataPresence::Present { handler_count: 0 }
        );
        assert_eq!(
            proof.root_map_requirements(),
            BaselineRootMapRequirementsProof {
                root_map_count: 0,
                safepoint_count: 0,
                complete_safepoint_root_map_count: 0,
            }
        );
    }

    #[test]
    fn baseline_bytecode_snapshot_fingerprint_matches_exact_code_block() {
        let owner = baseline_owner();
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ]);
        let proof = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner,
            baseline_tiering_snapshot(owner),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&code_block),
            Ok(proof.bytecode_snapshot_fingerprint())
        );

        let different_operand = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(8),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ]);
        assert_ne!(
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&different_operand)
                .unwrap(),
            proof.bytecode_snapshot_fingerprint()
        );

        let different_indices = code_block_from_typed_instructions(vec![
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                5,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ]);
        assert_ne!(
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&different_indices)
                .unwrap(),
            proof.bytecode_snapshot_fingerprint()
        );
    }

    #[test]
    fn baseline_bytecode_proof_binding_rejects_snapshot_owner_mismatch() {
        let owner = baseline_owner();
        let snapshot_owner = CodeBlockId(CellId(8));
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ]);
        let mut proof = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner,
            baseline_tiering_snapshot(owner),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .unwrap();
        proof.snapshot.owner = snapshot_owner;

        assert_eq!(
            bind_baseline_bytecode_proof_owner(owner, &proof),
            Err(
                BaselineBytecodeProofBindingError::ProofSnapshotOwnerMismatch {
                    proof_owner: owner,
                    snapshot_owner,
                }
            )
        );
    }

    #[test]
    fn baseline_bytecode_proof_binding_rejects_non_baseline_snapshot_target() {
        let owner = baseline_owner();
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ]);
        let mut proof = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner,
            baseline_tiering_snapshot(owner),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .unwrap();
        proof.snapshot.to_tier = JitType::Dfg;

        assert_eq!(
            bind_baseline_bytecode_proof_owner(owner, &proof),
            Err(
                BaselineBytecodeProofBindingError::ProofSnapshotTierMismatch {
                    from_tier: JitType::None,
                    to_tier: JitType::Dfg,
                }
            )
        );
    }

    #[test]
    fn baseline_bytecode_snapshot_fingerprint_includes_code_block_string_literals() {
        let instructions = || {
            vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadString,
                    vec![
                        Operand::Register(VirtualRegister::local(0)),
                        Operand::IdentifierIndex(7),
                    ],
                ),
                typed_instruction(
                    1,
                    CoreOpcode::Return,
                    vec![Operand::Register(VirtualRegister::local(0))],
                ),
            ]
        };
        let first = code_block_from_typed_instructions_with_string_literals(
            instructions(),
            vec![(7, "first".to_string())],
        );
        let second = code_block_from_typed_instructions_with_string_literals(
            instructions(),
            vec![(7, "second".to_string())],
        );

        assert_ne!(
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&first).unwrap(),
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&second).unwrap()
        );
    }

    #[test]
    fn baseline_bytecode_snapshot_fingerprint_includes_load_bigint_literal_text() {
        let instructions = || {
            vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadBigInt,
                    vec![
                        Operand::Register(VirtualRegister::local(0)),
                        Operand::IdentifierIndex(7),
                    ],
                ),
                typed_instruction(
                    1,
                    CoreOpcode::Return,
                    vec![Operand::Register(VirtualRegister::local(0))],
                ),
            ]
        };
        let first = code_block_from_typed_instructions_with_string_literals(
            instructions(),
            vec![(7, "123n".to_string())],
        );
        let second = code_block_from_typed_instructions_with_string_literals(
            instructions(),
            vec![(7, "456n".to_string())],
        );

        assert_ne!(
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&first).unwrap(),
            BaselineBytecodeEligibilityProof::fingerprint_code_block_snapshot(&second).unwrap()
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_rejects_code_block_unsupported_opcodes() {
        let owner = baseline_owner();
        let unsupported_core = code_block_from_core_opcodes(&[
            CoreOpcode::LoadInt32,
            CoreOpcode::Call,
            CoreOpcode::Return,
        ]);

        assert_eq!(
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
                &unsupported_core,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                Vec::new(),
            ),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Call,
                    reason: BaselineOpcodeRejectionReason::Call,
                }
            )
        );

        let non_core = code_block_from_opcodes(&[
            CoreOpcode::LoadInt32.opcode(),
            Opcode::Reserved,
            CoreOpcode::Return.opcode(),
        ]);

        assert_eq!(
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
                &non_core,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                Vec::new(),
            ),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedNonCoreOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: Opcode::Reserved,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_rejects_code_block_handler_metadata() {
        let owner = baseline_owner();
        let start = BytecodeIndex::from_offset(0);
        let end = BytecodeIndex::from_offset(1);
        let code_block = code_block_from_core_opcodes(&[CoreOpcode::LoadInt32, CoreOpcode::Return])
            .with_side_tables(LinkedSideTables {
                handlers: vec![HandlerInfo {
                    range: HandlerRange::Bytecode(BytecodeRange { start, end }),
                    target: HandlerTarget::Bytecode(end),
                    kind: HandlerKind::Catch,
                }],
                ..LinkedSideTables::default()
            });

        assert_eq!(
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                Vec::new(),
            ),
            Err(
                JitPlanValidationError::BaselineEligibilityExceptionHandlersUnsupported {
                    handler_count: 1,
                }
            )
        );
    }

    #[test]
    fn baseline_code_block_extraction_propagates_decode_failures() {
        let result = collect_baseline_bytecode_instructions([Err(
            InstructionDecodeError::RawBytesRequireGeneratedDecoder,
        )]);

        assert_eq!(
            result,
            Err(
                JitPlanValidationError::BaselineEligibilityInstructionDecodeFailed {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    error: InstructionDecodeError::RawBytesRequireGeneratedDecoder,
                }
            )
        );
    }

    #[test]
    fn baseline_code_block_extraction_rejects_invalid_decoded_index() {
        let owner = baseline_owner();
        let code_block = code_block_from_typed_instructions(vec![TypedInstruction {
            opcode: CoreOpcode::Return.opcode(),
            width: OperandWidth::Narrow,
            operands: Vec::new(),
            schema: None,
            bytecode_index: Some(BytecodeIndex::INVALID),
        }]);

        assert_eq!(
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
                &code_block,
                owner,
                baseline_tiering_snapshot(owner),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                Vec::new(),
            ),
            Err(
                JitPlanValidationError::BaselineEligibilityInvalidBytecodeRange {
                    start: BytecodeIndex::INVALID,
                    end: BytecodeIndex::INVALID,
                }
            )
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_rejects_unsupported_call_and_allocation_opcodes() {
        assert_eq!(
            baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, CoreOpcode::StrictEqual),
                baseline_instruction(2, CoreOpcode::Return),
            ])
            .validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::StrictEqual,
                    reason: BaselineOpcodeRejectionReason::Unsupported,
                }
            )
        );

        assert_eq!(
            baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, CoreOpcode::Call),
                baseline_instruction(2, CoreOpcode::Return),
            ])
            .validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Call,
                    reason: BaselineOpcodeRejectionReason::Call,
                }
            )
        );

        assert_eq!(
            baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, CoreOpcode::NewObject),
                baseline_instruction(2, CoreOpcode::Return),
            ])
            .validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::NewObject,
                    reason: BaselineOpcodeRejectionReason::AllocationOrObject,
                }
            )
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_rejects_property_and_exception_opcodes() {
        assert_eq!(
            baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, CoreOpcode::GetByName),
                baseline_instruction(2, CoreOpcode::Return),
            ])
            .validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::GetByName,
                    reason: BaselineOpcodeRejectionReason::PropertyAccess,
                }
            )
        );

        assert_eq!(
            baseline_eligibility_record(vec![
                baseline_instruction(0, CoreOpcode::LoadInt32),
                baseline_instruction(1, CoreOpcode::Throw),
                baseline_instruction(2, CoreOpcode::Return),
            ])
            .validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Throw,
                    reason: BaselineOpcodeRejectionReason::Exception,
                }
            )
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_rejects_ownerless_empty_and_invalid_bytecode() {
        let mut ownerless =
            baseline_eligibility_record(vec![baseline_instruction(0, CoreOpcode::Return)]);
        ownerless.owner = None;
        assert_eq!(
            ownerless.validate(),
            Err(JitPlanValidationError::BaselineEligibilityMissingOwner)
        );

        let mut default_owner =
            baseline_eligibility_record(vec![baseline_instruction(0, CoreOpcode::Return)]);
        default_owner.owner = Some(CodeBlockId::default());
        default_owner.snapshot.owner = CodeBlockId::default();
        assert_eq!(
            default_owner.validate(),
            Err(JitPlanValidationError::BaselineEligibilityDefaultOwner)
        );

        assert_eq!(
            baseline_eligibility_record(Vec::new()).validate(),
            Err(JitPlanValidationError::BaselineEligibilityEmptyBytecode)
        );

        let mut invalid_range =
            baseline_eligibility_record(vec![baseline_instruction(0, CoreOpcode::Return)]);
        invalid_range.bytecode.start = BytecodeIndex::from_offset(2);
        invalid_range.bytecode.end = BytecodeIndex::from_offset(1);
        assert_eq!(
            invalid_range.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityInvalidBytecodeRange {
                    start: BytecodeIndex::from_offset(2),
                    end: BytecodeIndex::from_offset(1),
                }
            )
        );

        let mut invalid_index =
            baseline_eligibility_record(vec![baseline_instruction(0, CoreOpcode::Return)]);
        invalid_index.instructions[0].bytecode_index = BytecodeIndex::INVALID;
        assert_eq!(
            invalid_index.validate(),
            Err(
                JitPlanValidationError::BaselineEligibilityInvalidBytecodeIndex(
                    BytecodeIndex::INVALID
                )
            )
        );
    }

    #[test]
    fn baseline_bytecode_eligibility_rejects_incomplete_safepoint_root_map() {
        let owner = baseline_owner();
        let index = BytecodeIndex::from_offset(1);
        let root_map_id = BytecodeRootMapId(30);
        let mut record = baseline_eligibility_record(vec![
            baseline_instruction(0, CoreOpcode::LoadInt32),
            baseline_instruction(1, CoreOpcode::AddInt32),
            baseline_instruction(2, CoreOpcode::Return),
        ]);
        record.root_map_requirements = BaselineRootMapRequirements {
            root_maps: vec![BytecodeRootMap {
                id: root_map_id,
                owner: Some(owner),
                bytecode_range_start: index,
                bytecode_range_end: index,
                slots: Vec::new(),
                complete: false,
            }],
            safepoints: vec![CompilerSafepointDescriptor {
                id: CompilerSafepointId(30),
                owner: Some(owner),
                code: None,
                tier: JitType::Baseline,
                kind: CompilerSafepointKind::Call,
                bytecode_index: Some(index),
                root_map: Some(root_map_id),
                roots: Vec::new(),
                may_call: true,
                may_allocate: false,
            }],
        };

        assert_eq!(
            record.validate(),
            Err(JitPlanValidationError::SafepointIncompleteRootMap {
                safepoint: CompilerSafepointId(30),
                root_map: root_map_id,
            })
        );
    }

    #[test]
    fn compiler_safepoint_requires_root_map_or_inline_roots() {
        let safepoint = CompilerSafepointDescriptor {
            id: CompilerSafepointId(1),
            owner: Some(CodeBlockId(CellId(7))),
            code: None,
            tier: JitType::Dfg,
            kind: CompilerSafepointKind::Call,
            bytecode_index: Some(BytecodeIndex::from_offset(20)),
            root_map: None,
            roots: vec![CompilerRootSlotDescriptor {
                bytecode_index: Some(BytecodeIndex::from_offset(20)),
                location: CompilerRootSlotLocation::StackSlot(-8),
                slot_kind: BytecodeRootSlotKind::VirtualRegister,
                root_kind: RootKind::Stack,
                mutation_authority: RootSetMutationAuthority::ConservativeScanner,
                precise: false,
            }],
            may_call: true,
            may_allocate: true,
        };

        assert_eq!(safepoint.validate(), Ok(()));
    }

    #[test]
    fn compiler_safepoint_lowers_complete_root_map_slots() {
        let index = BytecodeIndex::from_offset(20);
        let owner = Some(CodeBlockId(CellId(7)));
        let root_map_id = BytecodeRootMapId(9);
        let root_map = complete_root_map(
            root_map_id,
            owner,
            index,
            vec![
                BytecodeRootSlotDescriptor::virtual_register(
                    index,
                    VirtualRegister::local(0),
                    BytecodeRootSlotKind::VirtualRegister,
                ),
                BytecodeRootSlotDescriptor::runtime_slot(
                    index,
                    RuntimeSlot(4),
                    BytecodeRootSlotKind::InlineCache,
                ),
            ],
        );
        let safepoint = baseline_safepoint_referencing(root_map_id, Vec::new());

        let resolved = safepoint.resolve_root_map(&root_map).unwrap();

        assert_eq!(resolved.root_map, Some(root_map_id));
        assert_eq!(
            resolved.roots,
            vec![
                CompilerRootSlotDescriptor {
                    bytecode_index: Some(index),
                    location: CompilerRootSlotLocation::VirtualRegister(VirtualRegister::local(0)),
                    slot_kind: BytecodeRootSlotKind::VirtualRegister,
                    root_kind: RootKind::VMRegister,
                    mutation_authority: RootSetMutationAuthority::VmRegisterFile,
                    precise: true,
                },
                CompilerRootSlotDescriptor {
                    bytecode_index: Some(index),
                    location: CompilerRootSlotLocation::InlineCacheSlot(4),
                    slot_kind: BytecodeRootSlotKind::InlineCache,
                    root_kind: RootKind::JitCode,
                    mutation_authority: RootSetMutationAuthority::JitCodeRegistry,
                    precise: true,
                },
            ]
        );
    }

    #[test]
    fn baseline_call_safepoint_rejects_missing_complete_root_map() {
        let safepoint = baseline_safepoint_without_root_map(vec![baseline_vm_register_slot()]);

        assert_eq!(
            safepoint.validate(),
            Err(JitPlanValidationError::SafepointMissingRootMap(
                CompilerSafepointId(1)
            ))
        );

        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(3);
        let mut root_map = complete_root_map(
            root_map_id,
            Some(CodeBlockId(CellId(7))),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        root_map.complete = false;
        let safepoint = baseline_safepoint_referencing(root_map_id, Vec::new());

        assert_eq!(
            safepoint.resolve_root_map(&root_map),
            Err(JitPlanValidationError::SafepointIncompleteRootMap {
                safepoint: CompilerSafepointId(1),
                root_map: root_map_id,
            })
        );
    }

    #[test]
    fn compiler_safepoint_rejects_mismatched_root_map() {
        let index = BytecodeIndex::from_offset(20);
        let actual_root_map = BytecodeRootMapId(11);
        let root_map = complete_root_map(
            actual_root_map,
            Some(CodeBlockId(CellId(7))),
            index,
            vec![BytecodeRootSlotDescriptor::virtual_register(
                index,
                VirtualRegister::local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
        );
        let safepoint = baseline_safepoint_referencing(BytecodeRootMapId(12), Vec::new());

        assert_eq!(
            safepoint.resolve_root_map(&root_map),
            Err(JitPlanValidationError::SafepointRootMapMismatch {
                safepoint: CompilerSafepointId(1),
                expected: Some(BytecodeRootMapId(12)),
                actual: actual_root_map,
            })
        );
    }

    #[test]
    fn compiler_safepoint_rejects_duplicate_and_invalid_root_map_slots() {
        let index = BytecodeIndex::from_offset(20);
        let root_map_id = BytecodeRootMapId(14);
        let duplicate_slot = BytecodeRootSlotDescriptor::virtual_register(
            index,
            VirtualRegister::local(0),
            BytecodeRootSlotKind::VirtualRegister,
        );
        let duplicate_map = complete_root_map(
            root_map_id,
            Some(CodeBlockId(CellId(7))),
            index,
            vec![duplicate_slot, duplicate_slot],
        );
        let safepoint = baseline_safepoint_referencing(root_map_id, Vec::new());

        assert_eq!(
            safepoint.resolve_root_map(&duplicate_map),
            Err(JitPlanValidationError::SafepointRootMapInvalid {
                safepoint: CompilerSafepointId(1),
                root_map: root_map_id,
                error: BytecodeRootMapValidationError::DuplicateSlot {
                    bytecode_index: index,
                    kind: BytecodeRootSlotKind::VirtualRegister,
                    storage: BytecodeRootSlotStorage::Register(VirtualRegister::local(0)),
                },
            })
        );

        let invalid_slot = BytecodeRootSlotDescriptor::virtual_register(
            index,
            VirtualRegister::INVALID,
            BytecodeRootSlotKind::VirtualRegister,
        );
        let invalid_map = complete_root_map(
            root_map_id,
            Some(CodeBlockId(CellId(7))),
            index,
            vec![invalid_slot],
        );

        assert_eq!(
            safepoint.resolve_root_map(&invalid_map),
            Err(JitPlanValidationError::SafepointRootMapInvalid {
                safepoint: CompilerSafepointId(1),
                root_map: root_map_id,
                error: BytecodeRootMapValidationError::InvalidVirtualRegister {
                    bytecode_index: index,
                    kind: BytecodeRootSlotKind::VirtualRegister,
                    register: VirtualRegister::INVALID,
                },
            })
        );
    }

    #[test]
    fn compiler_safepoint_maps_baseline_vm_register_targeted_roots() {
        let heap = HeapId(3);
        let safepoint = baseline_safepoint_with_roots(vec![baseline_vm_register_slot()]);

        let plan = safepoint
            .targeted_root_plan(
                heap,
                &[CompilerSafepointRootBinding::targeted(0, CellId(42))],
            )
            .unwrap();
        let root = compiler_safepoint_root_id(CompilerSafepointId(1), 0);

        assert_eq!(
            plan,
            CompilerSafepointTargetedRootPlan {
                safepoint: CompilerSafepointId(1),
                heap,
                roots: vec![TargetedRootRecord {
                    root: RootRecord {
                        id: root,
                        kind: RootKind::VMRegister,
                        heap,
                    },
                    target: CellId(42),
                }],
            }
        );
    }

    #[test]
    fn baseline_targeted_root_plan_rejects_unresolved_root_map() {
        let root_map = BytecodeRootMapId(21);
        let safepoint = baseline_safepoint_referencing(root_map, Vec::new());

        assert_eq!(
            safepoint.targeted_root_plan(HeapId(3), &[]),
            Err(JitPlanValidationError::SafepointUnresolvedRootMap {
                safepoint: CompilerSafepointId(1),
                root_map,
            })
        );
    }

    #[test]
    fn compiler_safepoint_rejects_default_target_cell() {
        let safepoint = baseline_safepoint_with_roots(vec![baseline_vm_register_slot()]);

        assert_eq!(
            safepoint.targeted_root_plan(
                HeapId(3),
                &[CompilerSafepointRootBinding::targeted(0, CellId::default())],
            ),
            Err(JitPlanValidationError::SafepointTargetedRootSetInvalid {
                safepoint: CompilerSafepointId(1),
                error: RootSetSemanticError::InvalidRootTarget {
                    root: compiler_safepoint_root_id(CompilerSafepointId(1), 0),
                    target: CellId::default(),
                },
            })
        );
    }

    #[test]
    fn compiler_safepoint_rejects_targeted_root_authority_mismatch() {
        let mut slot = baseline_vm_register_slot();
        slot.mutation_authority = RootSetMutationAuthority::ExplicitRootRegistry;
        let safepoint = baseline_safepoint_with_roots(vec![slot]);

        assert_eq!(
            safepoint.targeted_root_plan(
                HeapId(3),
                &[CompilerSafepointRootBinding::targeted(0, CellId(42))],
            ),
            Err(JitPlanValidationError::SafepointRootAuthorityMismatch {
                safepoint: CompilerSafepointId(1),
                root_kind: RootKind::VMRegister,
                authority: RootSetMutationAuthority::ExplicitRootRegistry,
            })
        );
    }

    #[test]
    fn compiler_safepoint_keeps_no_target_and_unknown_slots_rootless() {
        let safepoint = baseline_safepoint_with_roots(vec![
            CompilerRootSlotDescriptor {
                bytecode_index: Some(BytecodeIndex::from_offset(20)),
                location: CompilerRootSlotLocation::StackSlot(-8),
                slot_kind: BytecodeRootSlotKind::VirtualRegister,
                root_kind: RootKind::Stack,
                mutation_authority: RootSetMutationAuthority::ConservativeScanner,
                precise: false,
            },
            CompilerRootSlotDescriptor {
                bytecode_index: Some(BytecodeIndex::from_offset(20)),
                location: CompilerRootSlotLocation::ConstantPool(3),
                slot_kind: BytecodeRootSlotKind::Constant,
                root_kind: RootKind::JitCode,
                mutation_authority: RootSetMutationAuthority::JitCodeRegistry,
                precise: true,
            },
        ]);

        let plan = safepoint
            .targeted_root_plan(
                HeapId(3),
                &[
                    CompilerSafepointRootBinding::no_target(0),
                    CompilerSafepointRootBinding::unknown(1),
                ],
            )
            .unwrap();

        assert!(plan.roots.is_empty());
    }

    fn owner_continuation_code_block() -> CodeBlock {
        owner_continuation_code_block_with_first_call_arguments(vec![
            VirtualRegister::local(3),
            VirtualRegister::local(4),
        ])
    }

    fn owner_continuation_code_block_with_wider_first_call() -> CodeBlock {
        owner_continuation_code_block_with_first_call_arguments(vec![
            VirtualRegister::local(3),
            VirtualRegister::local(4),
            VirtualRegister::local(11),
        ])
    }

    fn owner_continuation_code_block_with_first_call_arguments(
        first_call_arguments: Vec<VirtualRegister>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            call_instruction(
                1,
                VirtualRegister::local(1),
                VirtualRegister::local(2),
                first_call_arguments,
            ),
            call_with_this_instruction(
                2,
                VirtualRegister::local(5),
                VirtualRegister::local(6),
                VirtualRegister::local(7),
                vec![VirtualRegister::local(8)],
            ),
            construct_instruction(
                3,
                VirtualRegister::local(9),
                VirtualRegister::local(10),
                Vec::new(),
            ),
            typed_instruction(
                4,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(9))],
            ),
        ])
    }

    fn call_instruction(
        offset: u32,
        destination: VirtualRegister,
        callee: VirtualRegister,
        arguments: Vec<VirtualRegister>,
    ) -> TypedInstruction {
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::UnsignedImmediate(arguments.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(arguments.into_iter().map(Operand::Register));
        typed_instruction(offset, CoreOpcode::Call, operands)
    }

    fn call_with_this_instruction(
        offset: u32,
        destination: VirtualRegister,
        callee: VirtualRegister,
        this_value: VirtualRegister,
        arguments: Vec<VirtualRegister>,
    ) -> TypedInstruction {
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::Register(this_value),
            Operand::UnsignedImmediate(arguments.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(arguments.into_iter().map(Operand::Register));
        typed_instruction(offset, CoreOpcode::CallWithThis, operands)
    }

    fn construct_instruction(
        offset: u32,
        destination: VirtualRegister,
        callee: VirtualRegister,
        arguments: Vec<VirtualRegister>,
    ) -> TypedInstruction {
        let mut operands = vec![
            Operand::Register(destination),
            Operand::Register(callee),
            Operand::UnsignedImmediate(arguments.len().try_into().unwrap_or(u32::MAX)),
        ];
        operands.extend(arguments.into_iter().map(Operand::Register));
        typed_instruction(offset, CoreOpcode::Construct, operands)
    }

    fn many_construct_code_block(count: u32) -> CodeBlock {
        let mut instructions = Vec::new();
        for index in 0..count {
            instructions.push(construct_instruction(
                index,
                VirtualRegister::local(index.saturating_add(1)),
                VirtualRegister::local(0),
                Vec::new(),
            ));
        }
        instructions.push(typed_instruction(
            count,
            CoreOpcode::Return,
            vec![Operand::Register(VirtualRegister::local(count))],
        ));
        code_block_from_typed_instructions(instructions)
    }

    fn baseline_eligibility_record(
        instructions: Vec<BaselineBytecodeInstruction>,
    ) -> BaselineBytecodeEligibilityRecord {
        let owner = baseline_owner();
        let start = instructions
            .first()
            .map(|instruction| instruction.bytecode_index)
            .unwrap_or_else(|| BytecodeIndex::from_offset(0));
        let end = instructions
            .last()
            .map(|instruction| instruction.bytecode_index)
            .unwrap_or(start);
        BaselineBytecodeEligibilityRecord {
            owner: Some(owner),
            snapshot: baseline_tiering_snapshot(owner),
            bytecode: BaselineBytecodeRange {
                start,
                end,
                instruction_count: instructions.len() as u32,
            },
            opcode_subset: BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            instructions,
            root_map_requirements: BaselineRootMapRequirements::default(),
            exception_metadata: BaselineExceptionMetadataPresence::Present { handler_count: 0 },
        }
    }

    fn baseline_instruction(offset: u32, opcode: CoreOpcode) -> BaselineBytecodeInstruction {
        BaselineBytecodeInstruction {
            bytecode_index: BytecodeIndex::from_offset(offset),
            opcode,
        }
    }

    fn baseline_owner() -> CodeBlockId {
        CodeBlockId(CellId(7))
    }

    fn argument_including_this(index: u32) -> VirtualRegister {
        VirtualRegister::argument_including_this(
            index,
            crate::bytecode::register::ThisArgumentOffset(5),
        )
    }

    fn baseline_tiering_snapshot(owner: CodeBlockId) -> TieringSnapshot {
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

    fn code_block_from_core_opcodes(opcodes: &[CoreOpcode]) -> CodeBlock {
        let opcodes = opcodes
            .iter()
            .copied()
            .map(CoreOpcode::opcode)
            .collect::<Vec<_>>();
        code_block_from_opcodes(&opcodes)
    }

    fn code_block_from_opcodes(opcodes: &[Opcode]) -> CodeBlock {
        let mut builder = InstructionBuilder::new();
        for opcode in opcodes {
            builder.declare_instruction(*opcode, OperandWidth::Narrow, Vec::new());
        }
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Program, builder.finalize())
            .with_phase(UnlinkedCodeBlockPhase::Finalized);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    fn code_block_from_typed_instructions(instructions: Vec<TypedInstruction>) -> CodeBlock {
        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Program,
            crate::bytecode::PackedInstructionStream::from_typed_placeholder(instructions),
        )
        .with_phase(UnlinkedCodeBlockPhase::Finalized);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    fn code_block_from_typed_instructions_with_string_literals(
        instructions: Vec<TypedInstruction>,
        literals: Vec<(u32, String)>,
    ) -> CodeBlock {
        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Program,
            crate::bytecode::PackedInstructionStream::from_typed_placeholder(instructions),
        )
        .with_string_literals(literals)
        .with_phase(UnlinkedCodeBlockPhase::Finalized);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    fn identifier_property_key(identifier_index: u32) -> PropertyKey {
        PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(
            identifier_index,
        )))
    }

    fn property_handoff_site(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::get_by_name_property_load(
            owner,
            InlineCacheSlotId(0),
            bytecode_index,
            identifier_property_key(identifier_index),
        )
    }

    fn put_property_handoff_site(
        owner: CodeBlockId,
        bytecode_index: BytecodeIndex,
        identifier_index: u32,
    ) -> BaselineGeneratedPropertyHandoffSite {
        BaselineGeneratedPropertyHandoffSite::put_by_name_property_store(
            owner,
            InlineCacheSlotId(0),
            bytecode_index,
            identifier_property_key(identifier_index),
        )
    }

    fn get_by_name_code_block(identifier_index: u32) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::IdentifierIndex(identifier_index),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(1))],
            ),
        ])
    }

    fn get_length_code_block(identifier_index: u32) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetLength,
                vec![
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::IdentifierIndex(identifier_index),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(1))],
            ),
        ])
    }

    fn put_by_name_code_block(identifier_index: u32) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::IdentifierIndex(identifier_index),
                    Operand::Register(VirtualRegister::local(0)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ])
    }

    fn put_global_object_property_code_block(identifier_index: u32) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::PutGlobalObjectProperty,
                vec![
                    Operand::IdentifierIndex(identifier_index),
                    Operand::Register(VirtualRegister::local(0)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ])
    }

    fn get_global_object_property_code_block(identifier_index: u32) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(1),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetGlobalObjectProperty,
                vec![
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::IdentifierIndex(identifier_index),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::Register(VirtualRegister::local(0)),
                ],
            ),
            typed_instruction(
                3,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(2))],
            ),
        ])
    }

    fn get_by_value_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetByValue,
                vec![
                    Operand::Register(VirtualRegister::local(1)),
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::Register(VirtualRegister::local(3)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(1))],
            ),
        ])
    }

    fn put_by_value_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::SignedImmediate(7),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::PutByValue,
                vec![
                    Operand::Register(VirtualRegister::local(2)),
                    Operand::Register(VirtualRegister::local(3)),
                    Operand::Register(VirtualRegister::local(0)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(VirtualRegister::local(0))],
            ),
        ])
    }

    fn many_get_by_name_code_block(count: u32) -> CodeBlock {
        let mut instructions = vec![typed_instruction(
            0,
            CoreOpcode::LoadInt32,
            vec![
                Operand::Register(VirtualRegister::local(0)),
                Operand::SignedImmediate(7),
            ],
        )];
        for index in 0..count {
            instructions.push(typed_instruction(
                index + 1,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(VirtualRegister::local(index + 1)),
                    Operand::Register(VirtualRegister::local(0)),
                    Operand::IdentifierIndex(100 + index),
                ],
            ));
        }
        instructions.push(typed_instruction(
            count + 1,
            CoreOpcode::Return,
            vec![Operand::Register(VirtualRegister::local(count))],
        ));
        code_block_from_typed_instructions(instructions)
    }

    fn new_object_code_block_with_root_maps(
        destination: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        destination_only_helper_code_block_with_root_maps(
            CoreOpcode::NewObject,
            destination,
            root_maps,
        )
    }

    fn new_array_code_block_with_root_maps(
        destination: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        destination_only_helper_code_block_with_root_maps(
            CoreOpcode::NewArray,
            destination,
            root_maps,
        )
    }

    fn destination_only_helper_code_block_with_root_maps(
        opcode: CoreOpcode,
        destination: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(0, opcode, vec![Operand::Register(destination)]),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(destination)]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn type_of_code_block_with_root_maps(
        destination: VirtualRegister,
        source: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        type_of_code_block_with_operands(
            vec![Operand::Register(destination), Operand::Register(source)],
            destination,
            root_maps,
        )
    }

    fn type_of_code_block_with_operands(
        operands: Vec<Operand>,
        return_register: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(0, CoreOpcode::TypeOf, operands),
            typed_instruction(
                1,
                CoreOpcode::Return,
                vec![Operand::Register(return_register)],
            ),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn throw_code_block_with_root_maps(
        source: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(0, CoreOpcode::Throw, vec![Operand::Register(source)]),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(source)]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn load_string_code_block_with_root_maps(
        destination: VirtualRegister,
        literal_key: u32,
        literals: Vec<(u32, String)>,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        load_string_code_block_with_operands(
            vec![
                Operand::Register(destination),
                Operand::IdentifierIndex(literal_key),
            ],
            destination,
            literals,
            root_maps,
        )
    }

    fn load_string_code_block_with_operands(
        operands: Vec<Operand>,
        return_register: VirtualRegister,
        literals: Vec<(u32, String)>,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions_with_string_literals(
            vec![
                typed_instruction(0, CoreOpcode::LoadString, operands),
                typed_instruction(
                    1,
                    CoreOpcode::Return,
                    vec![Operand::Register(return_register)],
                ),
            ],
            literals,
        )
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn load_bigint_code_block_with_root_maps(
        destination: VirtualRegister,
        literal_key: u32,
        literals: Vec<(u32, String)>,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        load_bigint_code_block_with_operands(
            vec![
                Operand::Register(destination),
                Operand::IdentifierIndex(literal_key),
            ],
            destination,
            literals,
            root_maps,
        )
    }

    fn load_bigint_code_block_with_operands(
        operands: Vec<Operand>,
        return_register: VirtualRegister,
        literals: Vec<(u32, String)>,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions_with_string_literals(
            vec![
                typed_instruction(0, CoreOpcode::LoadBigInt, operands),
                typed_instruction(
                    1,
                    CoreOpcode::Return,
                    vec![Operand::Register(return_register)],
                ),
            ],
            literals,
        )
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn load_function_code_block_with_root_maps(
        destination: VirtualRegister,
        function_index: u32,
        captures: Vec<VirtualRegister>,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        let mut operands = vec![
            Operand::Register(destination),
            Operand::UnsignedImmediate(function_index),
            Operand::UnsignedImmediate(captures.len().min(u32::MAX as usize) as u32),
        ];
        operands.extend(captures.into_iter().map(Operand::Register));
        code_block_from_typed_instructions(vec![
            typed_instruction(0, CoreOpcode::LoadFunction, operands),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(destination)]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn initialize_global_lexical_code_block_with_root_maps(
        identifier: u32,
        source: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::InitializeGlobalLexical,
                vec![
                    Operand::IdentifierIndex(identifier),
                    Operand::Register(source),
                ],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(source)]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn mixed_new_object_new_array_code_block_with_root_maps(
        object_destination: VirtualRegister,
        array_destination: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::NewObject,
                vec![Operand::Register(object_destination)],
            ),
            typed_instruction(
                1,
                CoreOpcode::NewArray,
                vec![Operand::Register(array_destination)],
            ),
            typed_instruction(
                2,
                CoreOpcode::Return,
                vec![Operand::Register(array_destination)],
            ),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn mixed_runtime_helper_code_block_with_root_maps(
        object_destination: VirtualRegister,
        array_destination: VirtualRegister,
        type_of_destination: VirtualRegister,
        root_maps: Vec<BytecodeRootMap>,
    ) -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::NewObject,
                vec![Operand::Register(object_destination)],
            ),
            typed_instruction(
                1,
                CoreOpcode::NewArray,
                vec![Operand::Register(array_destination)],
            ),
            typed_instruction(
                2,
                CoreOpcode::TypeOf,
                vec![
                    Operand::Register(type_of_destination),
                    Operand::Register(array_destination),
                ],
            ),
            typed_instruction(
                3,
                CoreOpcode::Return,
                vec![Operand::Register(type_of_destination)],
            ),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps,
            ..LinkedSideTables::default()
        })
    }

    fn helper_classified_code_block_with_root_map(
        opcode: CoreOpcode,
        owner: CodeBlockId,
    ) -> CodeBlock {
        let helper_index = BytecodeIndex::from_offset(0);
        let destination = VirtualRegister::local(0);
        code_block_from_typed_instructions(vec![
            typed_instruction(0, opcode, vec![Operand::Register(destination)]),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(destination)]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps: vec![complete_root_map(
                BytecodeRootMapId(57),
                Some(owner),
                helper_index,
                vec![BytecodeRootSlotDescriptor::virtual_register(
                    helper_index,
                    destination,
                    BytecodeRootSlotKind::VirtualRegister,
                )],
            )],
            ..LinkedSideTables::default()
        })
    }

    fn typed_instruction(
        offset: u32,
        opcode: CoreOpcode,
        operands: Vec<Operand>,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(offset)),
        }
    }

    fn register_root_slot(
        index: BytecodeIndex,
        register: VirtualRegister,
        kind: BytecodeRootSlotKind,
    ) -> BytecodeRootSlotDescriptor {
        BytecodeRootSlotDescriptor::virtual_register(index, register, kind)
    }

    fn baseline_safepoint_with_roots(
        roots: Vec<CompilerRootSlotDescriptor>,
    ) -> CompilerSafepointDescriptor {
        baseline_safepoint_referencing(BytecodeRootMapId(1), roots)
    }

    fn baseline_safepoint_without_root_map(
        roots: Vec<CompilerRootSlotDescriptor>,
    ) -> CompilerSafepointDescriptor {
        CompilerSafepointDescriptor {
            id: CompilerSafepointId(1),
            owner: Some(CodeBlockId(CellId(7))),
            code: None,
            tier: JitType::Baseline,
            kind: CompilerSafepointKind::Call,
            bytecode_index: Some(BytecodeIndex::from_offset(20)),
            root_map: None,
            roots,
            may_call: true,
            may_allocate: true,
        }
    }

    fn baseline_safepoint_referencing(
        root_map: BytecodeRootMapId,
        roots: Vec<CompilerRootSlotDescriptor>,
    ) -> CompilerSafepointDescriptor {
        CompilerSafepointDescriptor {
            root_map: Some(root_map),
            ..baseline_safepoint_without_root_map(roots)
        }
    }

    fn baseline_vm_register_slot() -> CompilerRootSlotDescriptor {
        CompilerRootSlotDescriptor {
            bytecode_index: Some(BytecodeIndex::from_offset(20)),
            location: CompilerRootSlotLocation::VirtualRegister(VirtualRegister::from_raw(2)),
            slot_kind: BytecodeRootSlotKind::VirtualRegister,
            root_kind: RootKind::VMRegister,
            mutation_authority: RootSetMutationAuthority::VmRegisterFile,
            precise: true,
        }
    }

    fn complete_root_map(
        id: BytecodeRootMapId,
        owner: Option<CodeBlockId>,
        index: BytecodeIndex,
        slots: Vec<BytecodeRootSlotDescriptor>,
    ) -> BytecodeRootMap {
        BytecodeRootMap {
            id,
            owner,
            bytecode_range_start: index,
            bytecode_range_end: index,
            slots,
            complete: true,
        }
    }
}
