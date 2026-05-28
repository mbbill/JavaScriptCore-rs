use crate::bytecode::code_block::{BitVectorRef, BytecodeIndex, Checkpoint, RuntimeSlot};
use crate::bytecode::gc::{BytecodeRootMapId, BytecodeRootSlotKind};
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::gc::{RootKind, RootSetMutationAuthority, StructureId};
use crate::value::{EncodedJsValue, JsValue};

pub const VALUE_PROFILE_FIRST_OFFSET: u32 = 1;
pub const VALUE_PROFILE_RAW_BUCKET_BYTES: u32 = core::mem::size_of::<EncodedJsValue>() as u32;
pub const VALUE_PROFILE_PREDICTION_BYTES: u32 = core::mem::size_of::<SpeculatedTypeSet>() as u32;
pub const VALUE_PROFILE_RECORD_BYTES: u32 =
    VALUE_PROFILE_RAW_BUCKET_BYTES + VALUE_PROFILE_PREDICTION_BYTES;
pub const VALUE_PROFILE_FIRST_BUCKET_OFFSET: i32 = 0;
pub const VALUE_PROFILE_LINKING_DATA_BYTES: u32 = 16;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ValueProfileEmissionCapability {
    CannotCompile,
    #[default]
    CanCompile,
    CanCompileAndInline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ValueProfileEmissionPolicy {
    pub capability: ValueProfileEmissionCapability,
    pub should_emit: bool,
}

impl ValueProfileEmissionPolicy {
    pub const fn from_capability(capability: ValueProfileEmissionCapability) -> Self {
        Self {
            capability,
            should_emit: matches!(
                capability,
                ValueProfileEmissionCapability::CanCompile
                    | ValueProfileEmissionCapability::CanCompileAndInline
            ),
        }
    }
}

impl Default for ValueProfileEmissionPolicy {
    fn default() -> Self {
        Self::from_capability(ValueProfileEmissionCapability::CanCompile)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ValueProfileTable {
    pub profiles: Vec<ValueProfile>,
    pub unlinked_predictions: Vec<UnlinkedValueProfile>,
    pub root_metadata: Vec<ValueProfileRootMetadata>,
    pub jit_storage: ValueProfileJitStorage,
    pub emission_policy: ValueProfileEmissionPolicy,
    pub bucket_samples: Vec<ValueProfileBucketSample>,
}

/// Fixed raw-bucket storage that future generated code can address directly.
///
/// The boxed slice is deliberately separate from `bucket_samples`: JSC value
/// profile buckets hold raw `EncodedJSValue` samples, not strong roots or
/// growable telemetry records.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ValueProfileJitStorage {
    pub generation: ValueProfileJitStorageGeneration,
    pub raw_buckets: Box<[EncodedJsValue]>,
    pub bindings: Vec<ValueProfileJitBucketBinding>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct ValueProfileJitStorageGeneration(pub u64);

impl ValueProfileJitStorageGeneration {
    pub const fn next(self) -> Self {
        if self.0 == u64::MAX {
            self
        } else {
            Self(self.0 + 1)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ValueProfileJitBucketBinding {
    pub profile_slot: RuntimeSlot,
    pub bytecode_index: BytecodeIndex,
    pub checkpoint: Checkpoint,
    pub kind: ValueProfileBucketKind,
    pub storage_generation: ValueProfileJitStorageGeneration,
    pub storage_index: u32,
    pub value_profile_offset: u32,
    pub metadata_table_displacement: i32,
    pub emission_policy: ValueProfileEmissionPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ValueProfileJitStoreTarget {
    pub binding: ValueProfileJitBucketBinding,
    pub storage_generation: ValueProfileJitStorageGeneration,
    pub metadata_table_base_address: usize,
    pub raw_bucket_address: usize,
    pub raw_bucket_bytes: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct UnlinkedValueProfile {
    pub prediction: SpeculatedTypeSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueProfileBucketSample {
    pub bytecode_index: BytecodeIndex,
    pub checkpoint: Checkpoint,
    pub bucket: ValueProfileBucket,
    pub value: JsValue,
    pub sample_count: u32,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueProfileSampleError {
    MissingProfile {
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
    },
    MissingBucket {
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        kind: ValueProfileBucketKind,
    },
    MissingJitBucketStorage {
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        slot: RuntimeSlot,
        kind: ValueProfileBucketKind,
    },
    DuplicateJitBucketStorage {
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        slot: RuntimeSlot,
        kind: ValueProfileBucketKind,
    },
    StaleJitBucketStorage {
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        slot: RuntimeSlot,
        kind: ValueProfileBucketKind,
    },
}

impl ValueProfileTable {
    pub fn materialize_jit_storage_from_profiles(&mut self) {
        let generation = self.jit_storage.generation.next();
        self.jit_storage =
            ValueProfileJitStorage::from_profiles(&self.profiles, self.emission_policy, generation);
    }

    pub fn jit_store_target(
        &self,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        kind: ValueProfileBucketKind,
    ) -> Result<ValueProfileJitStoreTarget, ValueProfileSampleError> {
        let bucket = self.sample_bucket(bytecode_index, checkpoint, kind)?;
        let profile_index = self
            .profiles
            .iter()
            .position(|profile| {
                profile.bytecode_index == bytecode_index && profile.checkpoint == checkpoint
            })
            .ok_or(ValueProfileSampleError::MissingProfile {
                bytecode_index,
                checkpoint,
            })?;
        let expected_value_profile_offset =
            VALUE_PROFILE_FIRST_OFFSET.saturating_add(profile_index as u32);
        let target = self.jit_storage.store_target_for_bucket(
            bytecode_index,
            checkpoint,
            bucket.slot,
            bucket.kind,
        )?;
        if target.binding.value_profile_offset != expected_value_profile_offset
            || target.binding.metadata_table_displacement
                != metadata_table_displacement(expected_value_profile_offset)
            || target.binding.emission_policy != self.emission_policy
            || target.binding.storage_generation != self.jit_storage.generation
            || target.storage_generation != self.jit_storage.generation
            || metadata_table_relative_address(
                target.metadata_table_base_address,
                target.binding.metadata_table_displacement,
            ) != Some(target.raw_bucket_address)
            || target.raw_bucket_bytes != VALUE_PROFILE_RAW_BUCKET_BYTES
        {
            return Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot: bucket.slot,
                kind: bucket.kind,
            });
        }
        Ok(target)
    }

    pub fn sample_bucket(
        &self,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        kind: ValueProfileBucketKind,
    ) -> Result<ValueProfileBucket, ValueProfileSampleError> {
        let profile = self
            .profiles
            .iter()
            .find(|profile| {
                profile.bytecode_index == bytecode_index && profile.checkpoint == checkpoint
            })
            .ok_or(ValueProfileSampleError::MissingProfile {
                bytecode_index,
                checkpoint,
            })?;
        profile
            .buckets
            .iter()
            .copied()
            .find(|bucket| bucket.kind == kind)
            .ok_or(ValueProfileSampleError::MissingBucket {
                bytecode_index,
                checkpoint,
                kind,
            })
    }

    pub fn record_sample(
        &mut self,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        kind: ValueProfileBucketKind,
        value: JsValue,
    ) -> Result<ValueProfileBucketSample, ValueProfileSampleError> {
        let bucket = self.sample_bucket(bytecode_index, checkpoint, kind)?;
        if self.jit_storage.raw_buckets.is_empty() && !self.profiles.is_empty() {
            self.materialize_jit_storage_from_profiles();
        }
        self.jit_storage.write_bucket_sample(
            bytecode_index,
            checkpoint,
            bucket.slot,
            bucket.kind,
            value,
        )?;

        let sample = if let Some(existing) = self
            .bucket_samples
            .iter_mut()
            .find(|sample| sample.bucket.slot == bucket.slot)
        {
            existing.bytecode_index = bytecode_index;
            existing.checkpoint = checkpoint;
            existing.bucket = bucket;
            existing.value = value;
            existing.sample_count = existing.sample_count.saturating_add(1);
            *existing
        } else {
            let sample = ValueProfileBucketSample {
                bytecode_index,
                checkpoint,
                bucket,
                value,
                sample_count: 1,
            };
            self.bucket_samples.push(sample);
            sample
        };
        Ok(sample)
    }

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

impl ValueProfileJitStorage {
    pub fn from_profiles(
        profiles: &[ValueProfile],
        emission_policy: ValueProfileEmissionPolicy,
        generation: ValueProfileJitStorageGeneration,
    ) -> Self {
        let mut bindings = Vec::new();
        let profile_count = profiles.len() as u32;
        for (profile_index, profile) in profiles.iter().enumerate() {
            let value_profile_offset =
                VALUE_PROFILE_FIRST_OFFSET.saturating_add(profile_index as u32);
            let storage_index =
                value_profile_bucket_storage_index(profile_count, value_profile_offset);
            for bucket in &profile.buckets {
                bindings.push(ValueProfileJitBucketBinding {
                    profile_slot: bucket.slot,
                    bytecode_index: profile.bytecode_index,
                    checkpoint: profile.checkpoint,
                    kind: bucket.kind,
                    storage_generation: generation,
                    storage_index,
                    value_profile_offset,
                    metadata_table_displacement: metadata_table_displacement(value_profile_offset),
                    emission_policy,
                });
            }
        }
        let raw_slot_count =
            value_profile_metadata_table_backing_slot_count(profiles.len()).unwrap_or(0);
        Self {
            generation,
            raw_buckets: vec![EncodedJsValue::default(); raw_slot_count].into_boxed_slice(),
            bindings,
        }
    }

    pub fn store_target_for_bucket(
        &self,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        slot: RuntimeSlot,
        kind: ValueProfileBucketKind,
    ) -> Result<ValueProfileJitStoreTarget, ValueProfileSampleError> {
        let binding = self.binding_for_bucket(bytecode_index, checkpoint, slot, kind)?;
        if binding.bytecode_index != bytecode_index
            || binding.checkpoint != checkpoint
            || binding.kind != kind
        {
            return Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            });
        }
        let raw_bucket = self.raw_buckets.get(binding.storage_index as usize).ok_or(
            ValueProfileSampleError::MissingJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            },
        )?;
        let metadata_table_base_address = self.metadata_table_base_address();
        let raw_bucket_address = raw_bucket as *const EncodedJsValue as usize;
        if metadata_table_relative_address(
            metadata_table_base_address,
            binding.metadata_table_displacement,
        ) != Some(raw_bucket_address)
        {
            return Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            });
        }
        Ok(ValueProfileJitStoreTarget {
            binding,
            storage_generation: self.generation,
            metadata_table_base_address,
            raw_bucket_address,
            raw_bucket_bytes: VALUE_PROFILE_RAW_BUCKET_BYTES,
        })
    }

    fn metadata_table_base_address(&self) -> usize {
        if self.raw_buckets.is_empty() {
            0
        } else {
            self.raw_buckets
                .as_ptr()
                .wrapping_add(self.raw_buckets.len()) as usize
        }
    }

    pub fn raw_value_for_slot(&self, slot: RuntimeSlot) -> Option<EncodedJsValue> {
        let binding = self
            .binding_for_bucket(
                BytecodeIndex::default(),
                Checkpoint::NONE,
                slot,
                ValueProfileBucketKind::Sample,
            )
            .ok()?;
        self.raw_buckets
            .get(binding.storage_index as usize)
            .copied()
    }

    fn write_bucket_sample(
        &mut self,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        slot: RuntimeSlot,
        kind: ValueProfileBucketKind,
        value: JsValue,
    ) -> Result<(), ValueProfileSampleError> {
        let binding = self.binding_for_bucket(bytecode_index, checkpoint, slot, kind)?;
        if binding.bytecode_index != bytecode_index
            || binding.checkpoint != checkpoint
            || binding.kind != kind
        {
            return Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            });
        }
        let Some(raw_bucket) = self.raw_buckets.get_mut(binding.storage_index as usize) else {
            return Err(ValueProfileSampleError::MissingJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            });
        };
        *raw_bucket = value.encoded();
        Ok(())
    }

    fn binding_for_bucket(
        &self,
        bytecode_index: BytecodeIndex,
        checkpoint: Checkpoint,
        slot: RuntimeSlot,
        kind: ValueProfileBucketKind,
    ) -> Result<ValueProfileJitBucketBinding, ValueProfileSampleError> {
        let mut matches = self
            .bindings
            .iter()
            .copied()
            .filter(|binding| binding.profile_slot == slot);
        let Some(binding) = matches.next() else {
            return Err(ValueProfileSampleError::MissingJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            });
        };
        if matches.next().is_some() {
            return Err(ValueProfileSampleError::DuplicateJitBucketStorage {
                bytecode_index,
                checkpoint,
                slot,
                kind,
            });
        }
        Ok(binding)
    }
}

fn metadata_table_displacement(value_profile_offset: u32) -> i32 {
    let byte_offset = value_profile_offset.saturating_mul(VALUE_PROFILE_RECORD_BYTES);
    -(byte_offset as i32) + VALUE_PROFILE_FIRST_BUCKET_OFFSET
        - VALUE_PROFILE_LINKING_DATA_BYTES as i32
}

fn metadata_table_relative_address(base_address: usize, displacement: i32) -> Option<usize> {
    if displacement >= 0 {
        base_address.checked_add(displacement as usize)
    } else {
        base_address.checked_sub(displacement.unsigned_abs() as usize)
    }
}

fn value_profile_metadata_table_backing_slot_count(profile_count: usize) -> Option<usize> {
    let record_slots = (VALUE_PROFILE_RECORD_BYTES / VALUE_PROFILE_RAW_BUCKET_BYTES) as usize;
    let linking_slots =
        (VALUE_PROFILE_LINKING_DATA_BYTES / VALUE_PROFILE_RAW_BUCKET_BYTES) as usize;
    profile_count
        .checked_mul(record_slots)?
        .checked_add(linking_slots)
}

fn value_profile_bucket_storage_index(profile_count: u32, value_profile_offset: u32) -> u32 {
    let record_slots = VALUE_PROFILE_RECORD_BYTES / VALUE_PROFILE_RAW_BUCKET_BYTES;
    profile_count
        .saturating_sub(value_profile_offset)
        .saturating_mul(record_slots)
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

impl ArrayProfile {
    pub const fn for_bytecode_index(bytecode_index: BytecodeIndex) -> Self {
        Self {
            bytecode_index,
            last_seen_structure: None,
            speculation_failure_structure: None,
            observed_modes: ArrayModes::CLEAR,
            flags: ArrayProfileFlags::CLEAR,
        }
    }

    pub fn observe_indexed_read(&mut self, structure: StructureId, out_of_bounds: bool) {
        self.last_seen_structure = Some(structure);
        if out_of_bounds {
            self.flags.out_of_bounds = true;
        }
    }
}

impl ArrayModes {
    pub const CLEAR: Self = Self(0);

    pub const fn is_clear(self) -> bool {
        self.0 == 0
    }
}

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

impl ArrayProfileFlags {
    pub const CLEAR: Self = Self {
        may_store_hole: false,
        out_of_bounds: false,
        may_be_large_typed_array: false,
        may_intercept_indexed_accesses: false,
        uses_non_original_array_structures: false,
        may_be_resizable_or_growable_shared_typed_array: false,
        did_perform_first_run_pruning: false,
    };
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
    fn array_profile_indexed_read_records_structure_and_out_of_bounds() {
        let index = BytecodeIndex::from_offset(12);
        let structure = StructureId::new(91);
        let mut profile = ArrayProfile::for_bytecode_index(index);

        profile.observe_indexed_read(structure, false);
        assert_eq!(profile.bytecode_index, index);
        assert_eq!(profile.last_seen_structure, Some(structure));
        assert!(!profile.flags.out_of_bounds);
        assert!(profile.observed_modes.is_clear());

        profile.observe_indexed_read(StructureId::new(92), true);
        assert_eq!(profile.last_seen_structure, Some(StructureId::new(92)));
        assert!(profile.flags.out_of_bounds);
        assert!(!profile.flags.may_be_large_typed_array);
    }

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

    #[test]
    fn value_profile_records_and_overwrites_sample_bucket() {
        let index = BytecodeIndex::from_offset(16);
        let slot = RuntimeSlot(1);
        let mut table = ValueProfileTable {
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
            ..ValueProfileTable::default()
        };
        table.materialize_jit_storage_from_profiles();
        let store_target = table
            .jit_store_target(index, Checkpoint::NONE, ValueProfileBucketKind::Sample)
            .expect("jit store target");
        assert_eq!(
            store_target.binding.storage_generation,
            ValueProfileJitStorageGeneration(1)
        );
        assert_eq!(
            store_target.storage_generation,
            ValueProfileJitStorageGeneration(1)
        );
        assert_eq!(store_target.binding.value_profile_offset, 1);
        assert_eq!(store_target.binding.metadata_table_displacement, -32);
        assert_eq!(store_target.raw_bucket_bytes, 8);

        let first = table
            .record_sample(
                index,
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
                JsValue::from_i32(41),
            )
            .expect("first sample");
        assert_eq!(first.value, JsValue::from_i32(41));
        assert_eq!(first.sample_count, 1);
        assert_eq!(
            table.jit_storage.raw_value_for_slot(slot),
            Some(JsValue::from_i32(41).encoded())
        );

        let second = table
            .record_sample(
                index,
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
                JsValue::from_i32(42),
            )
            .expect("second sample");
        assert_eq!(second.value, JsValue::from_i32(42));
        assert_eq!(second.sample_count, 2);
        assert_eq!(table.bucket_samples.len(), 1);
        assert_eq!(table.bucket_samples[0], second);
        let updated_store_target = table
            .jit_store_target(index, Checkpoint::NONE, ValueProfileBucketKind::Sample)
            .expect("jit store target after write");
        assert_eq!(
            updated_store_target.raw_bucket_address, store_target.raw_bucket_address,
            "boxed raw-bucket storage must remain stable across VM-mediated samples"
        );
        assert_eq!(
            table.jit_storage.raw_value_for_slot(slot),
            Some(JsValue::from_i32(42).encoded())
        );
    }

    #[test]
    fn value_profile_records_cell_sample_as_raw_bucket_bits() {
        let index = BytecodeIndex::from_offset(20);
        let slot = RuntimeSlot(1);
        let cell = JsValue::from_encoded(
            crate::value::static_value_representation_layout()
                .encode_cell_payload(0x1234)
                .expect("cell payload encoding"),
        );
        assert!(cell.as_cell().is_some());
        let mut table = ValueProfileTable {
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
            ..ValueProfileTable::default()
        };
        table.materialize_jit_storage_from_profiles();

        let sample = table
            .record_sample(
                index,
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
                cell,
            )
            .expect("raw cell sample");

        assert_eq!(sample.value, cell);
        assert_eq!(
            table.jit_storage.raw_value_for_slot(slot),
            Some(cell.encoded())
        );
        assert!(table.root_metadata.is_empty());
    }

    #[test]
    fn value_profile_rejects_stale_jit_bucket_binding() {
        let index = BytecodeIndex::from_offset(24);
        let slot = RuntimeSlot(1);
        let mut table = ValueProfileTable {
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
            jit_storage: ValueProfileJitStorage {
                generation: ValueProfileJitStorageGeneration(1),
                raw_buckets: vec![EncodedJsValue::default()].into_boxed_slice(),
                bindings: vec![ValueProfileJitBucketBinding {
                    profile_slot: slot,
                    bytecode_index: BytecodeIndex::from_offset(28),
                    checkpoint: Checkpoint::NONE,
                    kind: ValueProfileBucketKind::Sample,
                    storage_generation: ValueProfileJitStorageGeneration(1),
                    storage_index: 0,
                    value_profile_offset: 1,
                    metadata_table_displacement: -32,
                    emission_policy: ValueProfileEmissionPolicy::default(),
                }],
            },
            ..ValueProfileTable::default()
        };

        assert_eq!(
            table.record_sample(
                index,
                Checkpoint::NONE,
                ValueProfileBucketKind::Sample,
                JsValue::from_i32(7),
            ),
            Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index: index,
                checkpoint: Checkpoint::NONE,
                slot,
                kind: ValueProfileBucketKind::Sample,
            })
        );
        assert_eq!(
            table.jit_storage.raw_value_for_slot(slot),
            Some(EncodedJsValue::default())
        );
        assert!(table.bucket_samples.is_empty());
    }

    #[test]
    fn value_profile_jit_storage_uses_one_based_negative_offsets() {
        let first = BytecodeIndex::from_offset(32);
        let second = BytecodeIndex::from_offset(36);
        let mut table = ValueProfileTable {
            profiles: vec![
                ValueProfile {
                    bytecode_index: first,
                    checkpoint: Checkpoint::NONE,
                    operand: None,
                    buckets: vec![ValueProfileBucket {
                        slot: RuntimeSlot(1),
                        kind: ValueProfileBucketKind::Sample,
                    }],
                    prediction: SpeculatedTypeSet(0),
                    update_policy: ProfileUpdatePolicy::ConcurrentBuckets,
                },
                ValueProfile {
                    bytecode_index: second,
                    checkpoint: Checkpoint::NONE,
                    operand: None,
                    buckets: vec![ValueProfileBucket {
                        slot: RuntimeSlot(2),
                        kind: ValueProfileBucketKind::Sample,
                    }],
                    prediction: SpeculatedTypeSet(0),
                    update_policy: ProfileUpdatePolicy::ConcurrentBuckets,
                },
            ],
            ..ValueProfileTable::default()
        };
        table.materialize_jit_storage_from_profiles();

        assert_eq!(
            table
                .jit_storage
                .bindings
                .iter()
                .map(|binding| (
                    binding.profile_slot,
                    binding.storage_generation,
                    binding.value_profile_offset,
                    binding.metadata_table_displacement,
                ))
                .collect::<Vec<_>>(),
            vec![
                (RuntimeSlot(1), ValueProfileJitStorageGeneration(1), 1, -32),
                (RuntimeSlot(2), ValueProfileJitStorageGeneration(1), 2, -48)
            ]
        );
        table.materialize_jit_storage_from_profiles();
        assert_eq!(
            table
                .jit_storage
                .bindings
                .iter()
                .map(|binding| binding.storage_generation)
                .collect::<Vec<_>>(),
            vec![
                ValueProfileJitStorageGeneration(2),
                ValueProfileJitStorageGeneration(2)
            ]
        );
    }

    #[test]
    fn value_profile_emission_policy_matches_jsc_capability_gate() {
        assert_eq!(
            ValueProfileEmissionPolicy::from_capability(
                ValueProfileEmissionCapability::CannotCompile
            ),
            ValueProfileEmissionPolicy {
                capability: ValueProfileEmissionCapability::CannotCompile,
                should_emit: false,
            }
        );
        assert_eq!(
            ValueProfileEmissionPolicy::from_capability(ValueProfileEmissionCapability::CanCompile),
            ValueProfileEmissionPolicy {
                capability: ValueProfileEmissionCapability::CanCompile,
                should_emit: true,
            }
        );
        assert_eq!(
            ValueProfileEmissionPolicy::from_capability(
                ValueProfileEmissionCapability::CanCompileAndInline,
            ),
            ValueProfileEmissionPolicy {
                capability: ValueProfileEmissionCapability::CanCompileAndInline,
                should_emit: true,
            }
        );
    }

    #[test]
    fn value_profile_jit_store_target_rejects_stale_policy_offset_and_duplicate_slot() {
        let index = BytecodeIndex::from_offset(40);
        let slot = RuntimeSlot(1);
        let mut table = ValueProfileTable {
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
            ..ValueProfileTable::default()
        };
        table.materialize_jit_storage_from_profiles();
        table.jit_storage.bindings[0].emission_policy = ValueProfileEmissionPolicy::from_capability(
            ValueProfileEmissionCapability::CannotCompile,
        );
        assert_eq!(
            table.jit_store_target(index, Checkpoint::NONE, ValueProfileBucketKind::Sample),
            Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index: index,
                checkpoint: Checkpoint::NONE,
                slot,
                kind: ValueProfileBucketKind::Sample,
            })
        );

        table.materialize_jit_storage_from_profiles();
        table.jit_storage.bindings[0].value_profile_offset = 2;
        assert_eq!(
            table.jit_store_target(index, Checkpoint::NONE, ValueProfileBucketKind::Sample),
            Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index: index,
                checkpoint: Checkpoint::NONE,
                slot,
                kind: ValueProfileBucketKind::Sample,
            })
        );

        table.materialize_jit_storage_from_profiles();
        table.jit_storage.bindings[0].storage_generation = ValueProfileJitStorageGeneration(1);
        assert_eq!(
            table.jit_store_target(index, Checkpoint::NONE, ValueProfileBucketKind::Sample),
            Err(ValueProfileSampleError::StaleJitBucketStorage {
                bytecode_index: index,
                checkpoint: Checkpoint::NONE,
                slot,
                kind: ValueProfileBucketKind::Sample,
            })
        );

        table.materialize_jit_storage_from_profiles();
        table
            .jit_storage
            .bindings
            .push(table.jit_storage.bindings[0]);
        assert_eq!(
            table.jit_store_target(index, Checkpoint::NONE, ValueProfileBucketKind::Sample),
            Err(ValueProfileSampleError::DuplicateJitBucketStorage {
                bytecode_index: index,
                checkpoint: Checkpoint::NONE,
                slot,
                kind: ValueProfileBucketKind::Sample,
            })
        );
    }
}
