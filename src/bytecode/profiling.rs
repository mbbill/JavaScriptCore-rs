use crate::bytecode::code_block::{BitVectorRef, BytecodeIndex, Checkpoint, RuntimeSlot};
use crate::bytecode::gc::{BytecodeRootMapId, BytecodeRootSlotKind};
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::gc::{RootKind, RootSetMutationAuthority, StructureId};

/// Runtime value profile slot indexed from opcode metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ValueProfile {
    pub bytecode_index: BytecodeIndex,
    pub checkpoint: Checkpoint,
    pub operand: Option<VirtualRegister>,
    pub buckets: Vec<ValueProfileBucket>,
    pub prediction: SpeculatedTypeSet,
    pub update_policy: ProfileUpdatePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ValueProfileBucket {
    pub slot: RuntimeSlot,
    pub kind: ValueProfileBucketKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ValueProfileBucketKind {
    Sample,
    SpeculationFailure,
    Argument,
    CatchValue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct SpeculatedTypeSet(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ProfileUpdatePolicy {
    #[default]
    ConcurrentBuckets,
    MainThreadMerge,
    FrozenFromUnlinked,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ValueProfileTable {
    pub profiles: Vec<ValueProfile>,
    pub unlinked_predictions: Vec<UnlinkedValueProfile>,
    pub root_metadata: Vec<ValueProfileRootMetadata>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct UnlinkedValueProfile {
    pub prediction: SpeculatedTypeSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueProfileRootMetadata {
    pub bytecode_index: BytecodeIndex,
    pub profile_slot: RuntimeSlot,
    pub root_map: Option<BytecodeRootMapId>,
    pub slot_kind: BytecodeRootSlotKind,
    pub root_kind: RootKind,
    pub mutation_authority: RootSetMutationAuthority,
    pub may_hold_cell: bool,
    pub profile_update_policy: ProfileUpdatePolicy,
}

impl ValueProfileRootMetadata {
    pub const fn for_profile_slot(
        bytecode_index: BytecodeIndex,
        profile_slot: RuntimeSlot,
        root_map: Option<BytecodeRootMapId>,
        profile_update_policy: ProfileUpdatePolicy,
    ) -> Self {
        Self {
            bytecode_index,
            profile_slot,
            root_map,
            slot_kind: BytecodeRootSlotKind::ValueProfile,
            root_kind: RootKind::VMRegister,
            mutation_authority: RootSetMutationAuthority::VmRegisterFile,
            may_hold_cell: true,
            profile_update_policy,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueProfileRootValidationError {
    InvalidBytecodeIndex,
    DuplicateProfileRoot(RuntimeSlot),
    RootKindAuthorityMismatch {
        root_kind: RootKind,
        authority: RootSetMutationAuthority,
    },
    ProfileRootMissingProfile {
        bytecode_index: BytecodeIndex,
        profile_slot: RuntimeSlot,
    },
}

impl ValueProfileTable {
    pub fn validate_root_metadata(&self) -> Result<(), ValueProfileRootValidationError> {
        for (index, root) in self.root_metadata.iter().enumerate() {
            if !root.bytecode_index.is_valid() {
                return Err(ValueProfileRootValidationError::InvalidBytecodeIndex);
            }
            if self.root_metadata[..index]
                .iter()
                .any(|previous| previous.profile_slot == root.profile_slot)
            {
                return Err(ValueProfileRootValidationError::DuplicateProfileRoot(
                    root.profile_slot,
                ));
            }
            if !matches!(
                (root.root_kind, root.mutation_authority),
                (
                    RootKind::VMRegister,
                    RootSetMutationAuthority::VmRegisterFile
                ) | (RootKind::JitCode, RootSetMutationAuthority::JitCodeRegistry)
                    | (
                        RootKind::ExplicitRoot,
                        RootSetMutationAuthority::ExplicitRootRegistry
                    )
            ) {
                return Err(ValueProfileRootValidationError::RootKindAuthorityMismatch {
                    root_kind: root.root_kind,
                    authority: root.mutation_authority,
                });
            }
            if !self.profiles.iter().any(|profile| {
                profile.bytecode_index == root.bytecode_index
                    && profile
                        .buckets
                        .iter()
                        .any(|bucket| bucket.slot == root.profile_slot)
            }) {
                return Err(ValueProfileRootValidationError::ProfileRootMissingProfile {
                    bytecode_index: root.bytecode_index,
                    profile_slot: root.profile_slot,
                });
            }
        }
        Ok(())
    }
}

/// Array access profile state shared by LLInt, baseline, and optimizing tiers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArrayProfile {
    pub bytecode_index: BytecodeIndex,
    pub last_seen_structure: Option<StructureId>,
    pub speculation_failure_structure: Option<StructureId>,
    pub observed_modes: ArrayModes,
    pub flags: ArrayProfileFlags,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct ArrayModes(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArrayProfileFlags {
    pub may_store_hole: bool,
    pub out_of_bounds: bool,
    pub may_be_large_typed_array: bool,
    pub may_intercept_indexed_accesses: bool,
    pub uses_non_original_array_structures: bool,
    pub may_be_resizable_or_growable_shared_typed_array: bool,
    pub did_perform_first_run_pruning: bool,
}

/// Arithmetic profile bitfield split into observed result and operand types.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArithProfile {
    pub bytecode_index: BytecodeIndex,
    pub result: ObservedResults,
    pub lhs: ObservedType,
    pub rhs: ObservedType,
    pub special_fast_path_taken: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ObservedResults {
    pub non_negative_zero_double: bool,
    pub negative_zero_double: bool,
    pub non_numeric: bool,
    pub int32_overflow: bool,
    pub int52_overflow: bool,
    pub heap_big_int: bool,
    pub big_int32: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ObservedType {
    pub int32: bool,
    pub number: bool,
    pub non_number: bool,
}

/// Execution counter contract used for LLInt and tier-up thresholds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct BytecodeExecutionCounter {
    pub counter: i32,
    pub total_count: i32,
    pub active_threshold: i32,
    pub variant: CountingVariant,
    pub state: ExecutionCounterState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CountingVariant {
    #[default]
    Baseline,
    UpperTiers,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExecutionCounterState {
    #[default]
    Counting,
    ThresholdCrossed,
    DeferredIndefinitely,
    ForcedSlowPath,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfilingCounterSet {
    pub baseline: BytecodeExecutionCounter,
    pub upper_tier: BytecodeExecutionCounter,
    pub loop_osr: Vec<LoopOsrCounter>,
    pub control_flow: Vec<ControlFlowProfileRecord>,
    pub type_ranges: Vec<TypeProfilerRecord>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct LoopOsrCounter {
    pub bytecode_index: BytecodeIndex,
    pub threshold: i32,
    pub backedge_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TypeProfilerRecord {
    pub origin: CodeOrigin,
    pub divot: u32,
    pub start_offset_from_divot: u32,
    pub end_offset_from_divot: u32,
    pub value_profile: Option<RuntimeSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ControlFlowProfileRecord {
    pub bytecode_index: BytecodeIndex,
    pub block_liveness: Option<BitVectorRef>,
    pub execution_count_slot: Option<RuntimeSlot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_profile_root_metadata_points_at_profile_bucket() {
        let index = BytecodeIndex::from_offset(12);
        let slot = RuntimeSlot(3);
        let table = ValueProfileTable {
            profiles: vec![ValueProfile {
                bytecode_index: index,
                checkpoint: Checkpoint::NONE,
                operand: None,
                buckets: vec![ValueProfileBucket {
                    slot,
                    kind: ValueProfileBucketKind::Sample,
                }],
                prediction: SpeculatedTypeSet(0),
                update_policy: ProfileUpdatePolicy::ConcurrentBuckets,
            }],
            root_metadata: vec![ValueProfileRootMetadata::for_profile_slot(
                index,
                slot,
                Some(BytecodeRootMapId(1)),
                ProfileUpdatePolicy::ConcurrentBuckets,
            )],
            ..ValueProfileTable::default()
        };

        assert_eq!(table.validate_root_metadata(), Ok(()));
    }

    #[test]
    fn value_profile_root_metadata_rejects_unmapped_slot() {
        let index = BytecodeIndex::from_offset(12);
        let table = ValueProfileTable {
            profiles: Vec::new(),
            root_metadata: vec![ValueProfileRootMetadata::for_profile_slot(
                index,
                RuntimeSlot(3),
                None,
                ProfileUpdatePolicy::ConcurrentBuckets,
            )],
            ..ValueProfileTable::default()
        };

        assert_eq!(
            table.validate_root_metadata(),
            Err(ValueProfileRootValidationError::ProfileRootMissingProfile {
                bytecode_index: index,
                profile_slot: RuntimeSlot(3),
            })
        );
    }
}
