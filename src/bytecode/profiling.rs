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

// === Arithmetic profiling (ArithProfile.h) ===
//
// Faithful packed-bitfield port of JSC's arithmetic profiles. `ObservedType`
// (ArithProfile.h:37-67) and `ObservedResults` (ArithProfile.h:69-100) are the
// two bit-packed components; `ArithProfile` (ArithProfile.h:102-182) is the
// shared base holding the low `ObservedResults` bits, and the
// `UnaryArithProfile` (ArithProfile.h:193-263) / `BinaryArithProfile`
// (ArithProfile.h:272-388) subclasses pack their operand `ObservedType`s above
// it. JSC templates `ArithProfile<BitfieldType>` but only instantiates
// `ArithProfile<uint16_t>` (ArithProfile.h:185), so the Rust base fixes the
// storage to `u16`. The bool-field stubs that used to live here were a
// divergence: they could not reproduce JSC's exact bit positions, the
// `observe*` merge order, or the `m_bits` single-store discipline the JIT reads
// concurrently. This port restores all three.

/// `ObservedType` (ArithProfile.h:37-67): 3-bit observed-type lattice for one
/// arithmetic operand.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ObservedType {
    bits: u8,
}

impl ObservedType {
    // ArithProfile.h:58-63.
    const TYPE_EMPTY: u8 = 0x0;
    const TYPE_INT32: u8 = 0x1;
    const TYPE_NUMBER: u8 = 0x2;
    const TYPE_NON_NUMBER: u8 = 0x4;
    const NUM_BITS_NEEDED: u32 = 3;

    // ArithProfile.h:38-40 (ObservedType(uint8_t bits = TypeEmpty)).
    pub const fn new(bits: u8) -> Self {
        Self { bits }
    }

    pub const fn empty() -> Self {
        Self {
            bits: Self::TYPE_EMPTY,
        }
    }

    // ArithProfile.h:42-49.
    pub const fn saw_int32(self) -> bool {
        self.bits & Self::TYPE_INT32 != 0
    }
    pub const fn is_only_int32(self) -> bool {
        self.bits == Self::TYPE_INT32
    }
    pub const fn saw_number(self) -> bool {
        self.bits & Self::TYPE_NUMBER != 0
    }
    pub const fn is_only_number(self) -> bool {
        self.bits == Self::TYPE_NUMBER
    }
    pub const fn saw_non_number(self) -> bool {
        self.bits & Self::TYPE_NON_NUMBER != 0
    }
    pub const fn is_only_non_number(self) -> bool {
        self.bits == Self::TYPE_NON_NUMBER
    }
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }
    pub const fn bits(self) -> u8 {
        self.bits
    }

    // ArithProfile.h:51-54.
    pub const fn with_int32(self) -> Self {
        Self {
            bits: self.bits | Self::TYPE_INT32,
        }
    }
    pub const fn with_number(self) -> Self {
        Self {
            bits: self.bits | Self::TYPE_NUMBER,
        }
    }
    pub const fn with_non_number(self) -> Self {
        Self {
            bits: self.bits | Self::TYPE_NON_NUMBER,
        }
    }
    pub const fn without_non_number(self) -> Self {
        Self {
            bits: self.bits & !Self::TYPE_NON_NUMBER,
        }
    }
}

/// `ObservedResults` (ArithProfile.h:69-100): 7-bit set of result observations
/// shared by the unary and binary profiles.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ObservedResults {
    bits: u8,
}

impl ObservedResults {
    // ArithProfile.h:71-80 (enum Tags + numBitsNeeded).
    const NON_NEG_ZERO_DOUBLE: u8 = 1 << 0;
    const NEG_ZERO_DOUBLE: u8 = 1 << 1;
    const NON_NUMERIC: u8 = 1 << 2;
    const INT32_OVERFLOW: u8 = 1 << 3;
    const INT52_OVERFLOW: u8 = 1 << 4;
    const HEAP_BIG_INT: u8 = 1 << 5;
    const BIG_INT32: u8 = 1 << 6;
    const NUM_BITS_NEEDED: u32 = 7;

    // ArithProfile.h:83-85.
    pub const fn new(bits: u8) -> Self {
        Self { bits }
    }

    // ArithProfile.h:87-96.
    pub const fn did_observe_non_int32(self) -> bool {
        self.bits
            & (Self::NON_NEG_ZERO_DOUBLE
                | Self::NEG_ZERO_DOUBLE
                | Self::NON_NUMERIC
                | Self::HEAP_BIG_INT
                | Self::BIG_INT32)
            != 0
    }
    pub const fn did_observe_double(self) -> bool {
        self.bits & (Self::NON_NEG_ZERO_DOUBLE | Self::NEG_ZERO_DOUBLE) != 0
    }
    pub const fn did_observe_non_neg_zero_double(self) -> bool {
        self.bits & Self::NON_NEG_ZERO_DOUBLE != 0
    }
    pub const fn did_observe_neg_zero_double(self) -> bool {
        self.bits & Self::NEG_ZERO_DOUBLE != 0
    }
    pub const fn did_observe_non_numeric(self) -> bool {
        self.bits & Self::NON_NUMERIC != 0
    }
    pub const fn did_observe_big_int(self) -> bool {
        self.bits & (Self::HEAP_BIG_INT | Self::BIG_INT32) != 0
    }
    pub const fn did_observe_heap_big_int(self) -> bool {
        self.bits & Self::HEAP_BIG_INT != 0
    }
    pub const fn did_observe_big_int32(self) -> bool {
        self.bits & Self::BIG_INT32 != 0
    }
    pub const fn did_observe_int32_overflow(self) -> bool {
        self.bits & Self::INT32_OVERFLOW != 0
    }
    pub const fn did_observe_int52_overflow(self) -> bool {
        self.bits & Self::INT52_OVERFLOW != 0
    }
    pub const fn bits(self) -> u8 {
        self.bits
    }
}

/// `ArithProfile<uint16_t>` base (ArithProfile.h:102-182): owns the low
/// `ObservedResults` bits shared by the unary and binary subclasses, plus the
/// `observeResult`/`setObserved*` mutators and `observedResults`/`didObserve*`
/// read predicates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArithProfile {
    // ArithProfile.h:181. JSC updates m_bits only in a single store so a
    // concurrent JIT reader never observes a half-written bitfield; the
    // subclass `observe*` methods mirror that by computing into a copy first.
    bits: u16,
}

impl ArithProfile {
    // ArithProfile.h:105-108.
    pub const fn observed_results(self) -> ObservedResults {
        ObservedResults::new((self.bits & ((1u16 << ObservedResults::NUM_BITS_NEEDED) - 1)) as u8)
    }

    // ArithProfile.h:109-118.
    pub const fn did_observe_non_int32(self) -> bool {
        self.observed_results().did_observe_non_int32()
    }
    pub const fn did_observe_double(self) -> bool {
        self.observed_results().did_observe_double()
    }
    pub const fn did_observe_non_neg_zero_double(self) -> bool {
        self.observed_results().did_observe_non_neg_zero_double()
    }
    pub const fn did_observe_neg_zero_double(self) -> bool {
        self.observed_results().did_observe_neg_zero_double()
    }
    pub const fn did_observe_non_numeric(self) -> bool {
        self.observed_results().did_observe_non_numeric()
    }
    pub const fn did_observe_big_int(self) -> bool {
        self.observed_results().did_observe_big_int()
    }
    pub const fn did_observe_heap_big_int(self) -> bool {
        self.observed_results().did_observe_heap_big_int()
    }
    pub const fn did_observe_big_int32(self) -> bool {
        self.observed_results().did_observe_big_int32()
    }
    pub const fn did_observe_int32_overflow(self) -> bool {
        self.observed_results().did_observe_int32_overflow()
    }
    pub const fn did_observe_int52_overflow(self) -> bool {
        self.observed_results().did_observe_int52_overflow()
    }

    // ArithProfile.h:120-126.
    pub fn set_observed_non_neg_zero_double(&mut self) {
        self.set_bit(ObservedResults::NON_NEG_ZERO_DOUBLE as u16);
    }
    pub fn set_observed_neg_zero_double(&mut self) {
        self.set_bit(ObservedResults::NEG_ZERO_DOUBLE as u16);
    }
    pub fn set_observed_non_numeric(&mut self) {
        self.set_bit(ObservedResults::NON_NUMERIC as u16);
    }
    pub fn set_observed_heap_big_int(&mut self) {
        self.set_bit(ObservedResults::HEAP_BIG_INT as u16);
    }
    pub fn set_observed_big_int32(&mut self) {
        self.set_bit(ObservedResults::BIG_INT32 as u16);
    }
    pub fn set_observed_int32_overflow(&mut self) {
        self.set_bit(ObservedResults::INT32_OVERFLOW as u16);
    }
    pub fn set_observed_int52_overflow(&mut self) {
        self.set_bit(ObservedResults::INT52_OVERFLOW as u16);
    }

    // ArithProfile.h:128-145.
    pub fn observe_result(&mut self, value: JsValue) {
        if value.is_int32() {
            return;
        }
        if value.is_number() {
            self.bits |= (ObservedResults::INT32_OVERFLOW
                | ObservedResults::INT52_OVERFLOW
                | ObservedResults::NON_NEG_ZERO_DOUBLE
                | ObservedResults::NEG_ZERO_DOUBLE) as u16;
            return;
        }
        // C++ JSC (ArithProfile.h:136-143) then checks isBigInt32()/isHeapBigInt()
        // and sets BigInt32/HeapBigInt respectively. The Rust value model does not
        // yet represent BigInt (JsValue has no isBigInt32/isHeapBigInt predicate),
        // so those branches are unreachable here and every non-numeric value —
        // including a BigInt cell once it exists — falls through to NonNumeric.
        // This is a transitional value-model gap, not an intentional semantic
        // change; restore the BigInt branches when JsValue gains BigInt
        // classification.
        self.bits |= ObservedResults::NON_NUMERIC as u16;
    }

    // ArithProfile.h:173.
    pub const fn bits(self) -> u16 {
        self.bits
    }

    // ArithProfile.h:178-179. `hasBits` feeds the JIT `emit*` helpers, which are
    // not ported in this UNWIRED unit, so it is exercised only by tests here.
    #[allow(dead_code)]
    const fn has_bits(self, mask: u16) -> bool {
        self.bits & mask != 0
    }
    fn set_bit(&mut self, mask: u16) {
        self.bits |= mask;
    }
}

/// `UnaryArithProfile` (ArithProfile.h:193-263): `ObservedResults` plus one
/// operand `ObservedType` packed into 16 bits.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct UnaryArithProfile {
    base: ArithProfile,
}

#[allow(dead_code)]
impl UnaryArithProfile {
    // ArithProfile.h:194.
    const ARG_OBSERVED_TYPE_SHIFT: u32 = ObservedResults::NUM_BITS_NEEDED;
    // ArithProfile.h:198.
    const CLEAR_ARG_OBSERVED_TYPE_BIT_MASK: u16 = !(0b111u16 << Self::ARG_OBSERVED_TYPE_SHIFT);
    // ArithProfile.h:200.
    const OBSERVED_TYPE_MASK: u16 = (1u16 << ObservedType::NUM_BITS_NEEDED) - 1;

    pub const fn bits(self) -> u16 {
        self.base.bits()
    }
    pub fn arith(&self) -> &ArithProfile {
        &self.base
    }
    pub fn arith_mut(&mut self) -> &mut ArithProfile {
        &mut self.base
    }

    // ArithProfile.h:210-227.
    pub const fn observed_int_bits() -> u16 {
        (ObservedType::empty().with_int32().bits() as u16) << Self::ARG_OBSERVED_TYPE_SHIFT
    }
    pub const fn observed_number_bits() -> u16 {
        (ObservedType::empty().with_number().bits() as u16) << Self::ARG_OBSERVED_TYPE_SHIFT
    }
    pub const fn observed_non_number_bits() -> u16 {
        (ObservedType::empty().with_non_number().bits() as u16) << Self::ARG_OBSERVED_TYPE_SHIFT
    }

    // ArithProfile.h:229.
    pub const fn arg_observed_type(self) -> ObservedType {
        ObservedType::new(
            ((self.base.bits() >> Self::ARG_OBSERVED_TYPE_SHIFT) & Self::OBSERVED_TYPE_MASK) as u8,
        )
    }
    // ArithProfile.h:230-237.
    pub fn set_arg_observed_type(&mut self, ty: ObservedType) {
        let mut bits = self.base.bits();
        bits &= Self::CLEAR_ARG_OBSERVED_TYPE_BIT_MASK;
        bits |= (ty.bits() as u16) << Self::ARG_OBSERVED_TYPE_SHIFT;
        self.base.bits = bits;
    }

    // ArithProfile.h:239-241.
    pub fn arg_saw_int32(&mut self) {
        let ty = self.arg_observed_type().with_int32();
        self.set_arg_observed_type(ty);
    }
    pub fn arg_saw_number(&mut self) {
        let ty = self.arg_observed_type().with_number();
        self.set_arg_observed_type(ty);
    }
    pub fn arg_saw_non_number(&mut self) {
        let ty = self.arg_observed_type().with_non_number();
        self.set_arg_observed_type(ty);
    }

    // ArithProfile.h:243-255.
    pub fn observe_arg(&mut self, arg: JsValue) {
        let mut new_profile = *self;
        if arg.is_number() {
            if arg.is_int32() {
                new_profile.arg_saw_int32();
            } else {
                new_profile.arg_saw_number();
            }
        } else {
            new_profile.arg_saw_non_number();
        }
        self.base.bits = new_profile.bits();
    }

    // ArithProfile.h:257-260.
    pub fn is_observed_type_empty(self) -> bool {
        self.arg_observed_type().is_empty()
    }
}

const _: () = {
    // ArithProfile.h:196 (Should fit in the type of the underlying bitfield).
    assert!(UnaryArithProfile::ARG_OBSERVED_TYPE_SHIFT + ObservedType::NUM_BITS_NEEDED <= 16);
};

/// `BinaryArithProfile` (ArithProfile.h:272-388): `ObservedResults` plus rhs/lhs
/// `ObservedType`s plus the division special-fast-path bit, packed into 16 bits.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct BinaryArithProfile {
    base: ArithProfile,
}

#[allow(dead_code)]
impl BinaryArithProfile {
    // ArithProfile.h:273-274.
    const RHS_OBSERVED_TYPE_SHIFT: u32 = ObservedResults::NUM_BITS_NEEDED;
    const LHS_OBSERVED_TYPE_SHIFT: u32 =
        Self::RHS_OBSERVED_TYPE_SHIFT + ObservedType::NUM_BITS_NEEDED;
    // ArithProfile.h:277-278.
    const CLEAR_RHS_OBSERVED_TYPE_BIT_MASK: u16 = !(0b111u16 << Self::RHS_OBSERVED_TYPE_SHIFT);
    const CLEAR_LHS_OBSERVED_TYPE_BIT_MASK: u16 = !(0b111u16 << Self::LHS_OBSERVED_TYPE_SHIFT);
    // ArithProfile.h:280.
    const OBSERVED_TYPE_MASK: u16 = (1u16 << ObservedType::NUM_BITS_NEEDED) - 1;
    // ArithProfile.h:283.
    pub const SPECIAL_FAST_PATH_BIT: u16 =
        1u16 << (Self::LHS_OBSERVED_TYPE_SHIFT + ObservedType::NUM_BITS_NEEDED);

    pub const fn bits(self) -> u16 {
        self.base.bits()
    }
    pub fn arith(&self) -> &ArithProfile {
        &self.base
    }
    pub fn arith_mut(&mut self) -> &mut ArithProfile {
        &mut self.base
    }

    // ArithProfile.h:296-321.
    pub const fn observed_int_int_bits() -> u16 {
        ((ObservedType::empty().with_int32().bits() as u16) << Self::LHS_OBSERVED_TYPE_SHIFT)
            | ((ObservedType::empty().with_int32().bits() as u16) << Self::RHS_OBSERVED_TYPE_SHIFT)
    }
    pub const fn observed_number_int_bits() -> u16 {
        ((ObservedType::empty().with_number().bits() as u16) << Self::LHS_OBSERVED_TYPE_SHIFT)
            | ((ObservedType::empty().with_int32().bits() as u16) << Self::RHS_OBSERVED_TYPE_SHIFT)
    }
    pub const fn observed_int_number_bits() -> u16 {
        ((ObservedType::empty().with_int32().bits() as u16) << Self::LHS_OBSERVED_TYPE_SHIFT)
            | ((ObservedType::empty().with_number().bits() as u16) << Self::RHS_OBSERVED_TYPE_SHIFT)
    }
    pub const fn observed_number_number_bits() -> u16 {
        ((ObservedType::empty().with_number().bits() as u16) << Self::LHS_OBSERVED_TYPE_SHIFT)
            | ((ObservedType::empty().with_number().bits() as u16) << Self::RHS_OBSERVED_TYPE_SHIFT)
    }

    // ArithProfile.h:323-324.
    pub const fn lhs_observed_type(self) -> ObservedType {
        ObservedType::new(
            ((self.base.bits() >> Self::LHS_OBSERVED_TYPE_SHIFT) & Self::OBSERVED_TYPE_MASK) as u8,
        )
    }
    pub const fn rhs_observed_type(self) -> ObservedType {
        ObservedType::new(
            ((self.base.bits() >> Self::RHS_OBSERVED_TYPE_SHIFT) & Self::OBSERVED_TYPE_MASK) as u8,
        )
    }

    // ArithProfile.h:325-332.
    pub fn set_lhs_observed_type(&mut self, ty: ObservedType) {
        let mut bits = self.base.bits();
        bits &= Self::CLEAR_LHS_OBSERVED_TYPE_BIT_MASK;
        bits |= (ty.bits() as u16) << Self::LHS_OBSERVED_TYPE_SHIFT;
        self.base.bits = bits;
    }
    // ArithProfile.h:334-341.
    pub fn set_rhs_observed_type(&mut self, ty: ObservedType) {
        let mut bits = self.base.bits();
        bits &= Self::CLEAR_RHS_OBSERVED_TYPE_BIT_MASK;
        bits |= (ty.bits() as u16) << Self::RHS_OBSERVED_TYPE_SHIFT;
        self.base.bits = bits;
    }

    // ArithProfile.h:343.
    pub const fn took_special_fast_path(self) -> bool {
        self.base.bits() & Self::SPECIAL_FAST_PATH_BIT != 0
    }

    // ArithProfile.h:345-350.
    pub fn lhs_saw_int32(&mut self) {
        let ty = self.lhs_observed_type().with_int32();
        self.set_lhs_observed_type(ty);
    }
    pub fn lhs_saw_number(&mut self) {
        let ty = self.lhs_observed_type().with_number();
        self.set_lhs_observed_type(ty);
    }
    pub fn lhs_saw_non_number(&mut self) {
        let ty = self.lhs_observed_type().with_non_number();
        self.set_lhs_observed_type(ty);
    }
    pub fn rhs_saw_int32(&mut self) {
        let ty = self.rhs_observed_type().with_int32();
        self.set_rhs_observed_type(ty);
    }
    pub fn rhs_saw_number(&mut self) {
        let ty = self.rhs_observed_type().with_number();
        self.set_rhs_observed_type(ty);
    }
    pub fn rhs_saw_non_number(&mut self) {
        let ty = self.rhs_observed_type().with_non_number();
        self.set_rhs_observed_type(ty);
    }

    // ArithProfile.h:352-364.
    pub fn observe_lhs(&mut self, lhs: JsValue) {
        let mut new_profile = *self;
        if lhs.is_number() {
            if lhs.is_int32() {
                new_profile.lhs_saw_int32();
            } else {
                new_profile.lhs_saw_number();
            }
        } else {
            new_profile.lhs_saw_non_number();
        }
        self.base.bits = new_profile.bits();
    }

    // ArithProfile.h:366-380.
    pub fn observe_lhs_and_rhs(&mut self, lhs: JsValue, rhs: JsValue) {
        self.observe_lhs(lhs);

        let mut new_profile = *self;
        if rhs.is_number() {
            if rhs.is_int32() {
                new_profile.rhs_saw_int32();
            } else {
                new_profile.rhs_saw_number();
            }
        } else {
            new_profile.rhs_saw_non_number();
        }
        self.base.bits = new_profile.bits();
    }

    // ArithProfile.h:382-385.
    pub fn is_observed_type_empty(self) -> bool {
        self.lhs_observed_type().is_empty() && self.rhs_observed_type().is_empty()
    }
}

const _: () = {
    // ArithProfile.h:284-287 (static_asserts pinning the special-fast-path bit).
    assert!(BinaryArithProfile::LHS_OBSERVED_TYPE_SHIFT + ObservedType::NUM_BITS_NEEDED + 1 <= 16);
    assert!(
        BinaryArithProfile::SPECIAL_FAST_PATH_BIT
            & !BinaryArithProfile::CLEAR_LHS_OBSERVED_TYPE_BIT_MASK
            == 0
    );
    assert!(
        BinaryArithProfile::SPECIAL_FAST_PATH_BIT
            & BinaryArithProfile::CLEAR_LHS_OBSERVED_TYPE_BIT_MASK
            != 0
    );
    assert!(
        BinaryArithProfile::SPECIAL_FAST_PATH_BIT
            > !BinaryArithProfile::CLEAR_LHS_OBSERVED_TYPE_BIT_MASK
    );
};

// === Execution / tier-up counter (ExecutionCounter.{h,cpp}) ===

/// `CountingVariant` (ExecutionCounter.h:37-40). JSC uses this as a template
/// parameter on `ExecutionCounter`; the Rust port carries it as a field
/// (matching the `BaselineExecutionCounter`/`UpperTierExecutionCounter`
/// typedefs, ExecutionCounter.h:106-107).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CountingVariant {
    /// `CountingForBaseline`.
    #[default]
    Baseline,
    /// `CountingForUpperTiers`.
    UpperTiers,
}

/// Lifecycle labels retained for the existing `bytecode` re-export. JSC's
/// `ExecutionCounter` (ExecutionCounter.{h,cpp}) does NOT store an explicit
/// state; its lifecycle is encoded in the counter/threshold sentinel values
/// (`forceSlowPathConcurrently` -> m_counter = 0; `deferIndefinitely` ->
/// m_activeThreshold = INT32_MAX, m_counter = INT32_MIN). The faithful counter
/// below therefore carries no `state` field. This enum is kept (not deleted)
/// because it is a cross-module re-export; unifying it with the live tier-up
/// state machine in jit/tiering.rs is flagged as a serial coupling.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExecutionCounterState {
    #[default]
    Counting,
    ThresholdCrossed,
    DeferredIndefinitely,
    ForcedSlowPath,
}

/// The two `CodeBlock*`-derived inputs JSC's `ExecutionCounter` methods read.
/// `applyMemoryUsageHeuristics` (ExecutionCounter.cpp:76-88) multiplies a
/// threshold by `ExecutableAllocator::memoryPressureMultiplier` (>= 1.0; 1.0
/// when `codeBlock` is null), and `maximumExecutionCountsBetweenCheckpoints`
/// (ExecutionCounter.cpp:102-127) resolves the per-variant `Options` cap. This
/// unit is self-contained and UNWIRED (no CodeBlock/Options/ExecutableAllocator
/// yet), so the caller supplies the already-resolved values instead of passing a
/// `CodeBlock*`.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionCounterEnvironment {
    pub memory_usage_multiplier: f64,
    pub maximum_execution_counts_between_checkpoints: i32,
}

impl ExecutionCounterEnvironment {
    /// The `codeBlock == nullptr` defaults: multiplier 1.0
    /// (ExecutionCounter.cpp:78) with the supplied per-variant cap.
    #[allow(dead_code)]
    pub const fn with_max_counts(maximum_execution_counts_between_checkpoints: i32) -> Self {
        Self {
            memory_usage_multiplier: 1.0,
            maximum_execution_counts_between_checkpoints,
        }
    }
}

/// `formattedTotalExecutionCount` (ExecutionCounter.h:46-54): reinterpret the
/// float total-count bits as int32 (the in-memory form machine code adds to).
#[allow(dead_code)]
pub fn formatted_total_execution_count(value: f32) -> i32 {
    value.to_bits() as i32
}

/// `applyMemoryUsageHeuristics` (ExecutionCounter.cpp:76-88).
fn apply_memory_usage_heuristics(value: i32, multiplier: f64) -> f64 {
    debug_assert!(multiplier >= 1.0);
    multiplier * value as f64
}

/// `applyMemoryUsageHeuristicsAndConvertToInt` (ExecutionCounter.cpp:90-100).
#[allow(dead_code)]
pub fn apply_memory_usage_heuristics_and_convert_to_int(value: i32, multiplier: f64) -> i32 {
    let double_result = apply_memory_usage_heuristics(value, multiplier);
    debug_assert!(double_result >= 0.0);
    if double_result > i32::MAX as f64 {
        return i32::MAX;
    }
    double_result as i32
}

/// `ExecutionCounter<countingVariant>` (ExecutionCounter.h:56-101): a down-
/// counter the JIT/LLInt increments toward zero, the float running total, and
/// the original (uncorrected) target threshold.
///
/// `m_totalCount` is `float` in JSC (ExecutionCounter.h:96); the port mirrors
/// that with `f32`, which is why this struct cannot derive `Eq`/`Hash`. The
/// prior stub used `i32` (a divergence) and added a `state` field with no JSC
/// counterpart. The derived `Default` reproduces the C++ constructor, which
/// calls `reset()` -> all zero (ExecutionCounter.cpp:36-40, 199-205).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BytecodeExecutionCounter {
    /// ExecutionCounter.h:90 (m_counter): negative, counted up toward zero.
    pub counter: i32,
    /// ExecutionCounter.h:96 (m_totalCount).
    pub total_count: f32,
    /// ExecutionCounter.h:100 (m_activeThreshold): uncorrected target.
    pub active_threshold: i32,
    /// Mirrors the `countingVariant` template parameter (ExecutionCounter.h:56).
    pub variant: CountingVariant,
}

impl BytecodeExecutionCounter {
    /// `ExecutionCounter()` (ExecutionCounter.cpp:36-40) -> `reset()`.
    #[allow(dead_code)]
    pub fn new(variant: CountingVariant) -> Self {
        let mut counter = Self {
            variant,
            ..Self::default()
        };
        counter.reset();
        counter
    }

    /// `forceSlowPathConcurrently` (ExecutionCounter.cpp:42-46).
    #[allow(dead_code)]
    pub fn force_slow_path_concurrently(&mut self) {
        self.counter = 0;
    }

    /// `checkIfThresholdCrossedAndSet` (ExecutionCounter.cpp:48-58).
    #[allow(dead_code)]
    pub fn check_if_threshold_crossed_and_set(&mut self, env: ExecutionCounterEnvironment) -> bool {
        if self.has_crossed_threshold(env) {
            return true;
        }
        if self.set_threshold(env) {
            return true;
        }
        false
    }

    /// `setNewThreshold` (ExecutionCounter.cpp:60-66).
    #[allow(dead_code)]
    pub fn set_new_threshold(&mut self, threshold: i32, env: ExecutionCounterEnvironment) {
        self.reset();
        self.active_threshold = threshold;
        self.set_threshold(env);
    }

    /// `deferIndefinitely` (ExecutionCounter.cpp:68-74).
    #[allow(dead_code)]
    pub fn defer_indefinitely(&mut self) {
        self.total_count = 0.0;
        self.active_threshold = i32::MAX;
        self.counter = i32::MIN;
    }

    /// `count()` (ExecutionCounter.h:66): total executions = m_totalCount +
    /// m_counter (m_counter's negative threshold cancels the seeded total).
    #[allow(dead_code)]
    pub fn count(&self) -> f64 {
        self.total_count as f64 + self.counter as f64
    }

    /// `clippedThreshold` (ExecutionCounter.h:69-76): cap at the per-variant
    /// maximum execution counts between checkpoints.
    #[allow(dead_code)]
    pub fn clipped_threshold(
        threshold: f64,
        maximum_execution_counts_between_checkpoints: i32,
    ) -> f64 {
        let max_threshold = maximum_execution_counts_between_checkpoints as f64;
        if threshold > max_threshold {
            max_threshold
        } else {
            threshold
        }
    }

    /// `hasCrossedThreshold` (ExecutionCounter.cpp:129-161): declare victory a
    /// bit early (within half the original threshold) to avoid thrashing.
    fn has_crossed_threshold(&self, env: ExecutionCounterEnvironment) -> bool {
        let modified_threshold =
            apply_memory_usage_heuristics(self.active_threshold, env.memory_usage_multiplier);
        let actual_count = self.total_count as f64 + self.counter as f64;
        let desired_count = modified_threshold
            - (self
                .active_threshold
                .min(env.maximum_execution_counts_between_checkpoints) as f64)
                / 2.0;
        actual_count >= desired_count
    }

    /// `setThreshold` (ExecutionCounter.cpp:163-197): re-seed `m_counter` so the
    /// JIT/LLInt counts up by `threshold` more executions before re-checking.
    fn set_threshold(&mut self, env: ExecutionCounterEnvironment) -> bool {
        if self.active_threshold == i32::MAX {
            self.defer_indefinitely();
            return false;
        }

        let true_total_count = self.count();
        let mut threshold =
            apply_memory_usage_heuristics(self.active_threshold, env.memory_usage_multiplier);
        debug_assert!(threshold >= 0.0);
        threshold -= true_total_count;

        if threshold <= 0.0 {
            self.counter = 0;
            self.total_count = true_total_count as f32;
            return true;
        }

        threshold =
            Self::clipped_threshold(threshold, env.maximum_execution_counts_between_checkpoints);
        self.counter = (-threshold) as i32;
        self.total_count = (true_total_count + threshold) as f32;
        false
    }

    /// `reset()` (ExecutionCounter.cpp:199-205).
    fn reset(&mut self) {
        self.counter = 0;
        self.total_count = 0.0;
        self.active_threshold = 0;
    }
}

// `BytecodeExecutionCounter` carries a faithful `f32` `total_count`
// (ExecutionCounter.h:96), so neither it nor this aggregate can derive `Eq`. The
// live `CodeBlockTierState` compares `profiling_counters` via a hand-written
// `PartialEq`, so `PartialEq` is retained.
#[derive(Clone, Debug, Default, PartialEq)]
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

    // === ArithProfile (ArithProfile.h) ===

    #[test]
    fn observed_type_bit_layout_matches_arith_profile_header() {
        // ArithProfile.h:58-63.
        assert_eq!(ObservedType::TYPE_EMPTY, 0x0);
        assert_eq!(ObservedType::TYPE_INT32, 0x1);
        assert_eq!(ObservedType::TYPE_NUMBER, 0x2);
        assert_eq!(ObservedType::TYPE_NON_NUMBER, 0x4);
        assert_eq!(ObservedType::NUM_BITS_NEEDED, 3);

        // ArithProfile.h:42-54 round-trips.
        let t = ObservedType::empty().with_int32().with_number();
        assert!(t.saw_int32() && t.saw_number() && !t.saw_non_number());
        assert!(!t.is_only_int32());
        assert!(ObservedType::empty().with_int32().is_only_int32());
        assert!(ObservedType::empty().is_empty());
        assert_eq!(
            ObservedType::empty().with_non_number().without_non_number(),
            ObservedType::empty()
        );
    }

    #[test]
    fn observed_results_tag_bits_and_predicates_match_header() {
        // ArithProfile.h:71-80.
        assert_eq!(ObservedResults::NON_NEG_ZERO_DOUBLE, 1 << 0);
        assert_eq!(ObservedResults::NEG_ZERO_DOUBLE, 1 << 1);
        assert_eq!(ObservedResults::NON_NUMERIC, 1 << 2);
        assert_eq!(ObservedResults::INT32_OVERFLOW, 1 << 3);
        assert_eq!(ObservedResults::INT52_OVERFLOW, 1 << 4);
        assert_eq!(ObservedResults::HEAP_BIG_INT, 1 << 5);
        assert_eq!(ObservedResults::BIG_INT32, 1 << 6);
        assert_eq!(ObservedResults::NUM_BITS_NEEDED, 7);

        // ArithProfile.h:87-96.
        let r = ObservedResults::new(ObservedResults::NEG_ZERO_DOUBLE);
        assert!(
            r.did_observe_double() && r.did_observe_neg_zero_double() && r.did_observe_non_int32()
        );
        assert!(!r.did_observe_non_neg_zero_double());
        let big = ObservedResults::new(ObservedResults::HEAP_BIG_INT);
        assert!(
            big.did_observe_big_int()
                && big.did_observe_heap_big_int()
                && !big.did_observe_big_int32()
        );
        assert!(big.did_observe_non_int32());
        let overflow = ObservedResults::new(ObservedResults::INT32_OVERFLOW);
        assert!(overflow.did_observe_int32_overflow() && !overflow.did_observe_non_int32());
    }

    #[test]
    fn arith_profile_observe_result_sets_double_or_non_numeric_bits() {
        // ArithProfile.h:128-145, low-7-bit read ArithProfile.h:105-108.
        let mut p = ArithProfile::default();
        p.observe_result(JsValue::from_i32(7));
        assert_eq!(
            p.bits(),
            0,
            "int32 takes the early return, recording no bits"
        );

        p.observe_result(JsValue::from_double(1.5));
        // Int32Overflow|Int52Overflow|NonNegZeroDouble|NegZeroDouble.
        assert_eq!(p.bits(), 0b0001_1011);
        let r = p.observed_results();
        assert!(
            r.did_observe_int32_overflow()
                && r.did_observe_int52_overflow()
                && r.did_observe_non_neg_zero_double()
                && r.did_observe_neg_zero_double()
        );
        assert!(!r.did_observe_non_numeric());

        let mut q = ArithProfile::default();
        q.observe_result(JsValue::undefined());
        assert!(q.observed_results().did_observe_non_numeric());
        assert_eq!(q.bits(), ObservedResults::NON_NUMERIC as u16);
    }

    #[test]
    fn arith_profile_set_observed_bits_are_independent() {
        // ArithProfile.h:120-126 (setObserved* / setBit) + observedResults read.
        let mut p = ArithProfile::default();
        p.set_observed_int52_overflow();
        assert!(p.has_bits(ObservedResults::INT52_OVERFLOW as u16));
        assert_eq!(p.bits(), ObservedResults::INT52_OVERFLOW as u16);
        p.set_observed_non_numeric();
        assert_eq!(
            p.bits(),
            (ObservedResults::INT52_OVERFLOW | ObservedResults::NON_NUMERIC) as u16
        );
    }

    #[test]
    fn unary_arith_profile_packs_arg_type_above_results() {
        // ArithProfile.h:194 (shift), :229 (read), :243-255 (observe).
        assert_eq!(UnaryArithProfile::ARG_OBSERVED_TYPE_SHIFT, 7);

        let mut p = UnaryArithProfile::default();
        p.observe_arg(JsValue::from_i32(3));
        assert!(p.arg_observed_type().saw_int32());
        // TypeInt32 (0x1) packed at bit 7.
        assert_eq!(p.bits(), 0x1 << 7);

        // ArithProfile.h:210-221.
        assert_eq!(UnaryArithProfile::observed_int_bits(), 0x1 << 7);
        assert_eq!(UnaryArithProfile::observed_number_bits(), 0x2 << 7);
        assert_eq!(UnaryArithProfile::observed_non_number_bits(), 0x4 << 7);

        // A double records Number (not Int32) in the arg slot.
        let mut d = UnaryArithProfile::default();
        assert!(d.is_observed_type_empty());
        d.observe_arg(JsValue::from_double(2.5));
        assert!(d.arg_observed_type().saw_number() && !d.arg_observed_type().saw_int32());
        assert!(!d.is_observed_type_empty());
    }

    #[test]
    fn binary_arith_profile_packs_lhs_rhs_and_special_fast_path() {
        // ArithProfile.h:273-283.
        assert_eq!(BinaryArithProfile::RHS_OBSERVED_TYPE_SHIFT, 7);
        assert_eq!(BinaryArithProfile::LHS_OBSERVED_TYPE_SHIFT, 10);
        assert_eq!(BinaryArithProfile::SPECIAL_FAST_PATH_BIT, 1 << 13);

        // ArithProfile.h:296-321 static bit patterns.
        assert_eq!(
            BinaryArithProfile::observed_int_int_bits(),
            (0x1 << 10) | (0x1 << 7)
        );
        assert_eq!(
            BinaryArithProfile::observed_number_int_bits(),
            (0x2 << 10) | (0x1 << 7)
        );
        assert_eq!(
            BinaryArithProfile::observed_int_number_bits(),
            (0x1 << 10) | (0x2 << 7)
        );
        assert_eq!(
            BinaryArithProfile::observed_number_number_bits(),
            (0x2 << 10) | (0x2 << 7)
        );

        // ArithProfile.h:366-380 observeLHSAndRHS round-trip: lhs int32, rhs double.
        let mut p = BinaryArithProfile::default();
        assert!(p.is_observed_type_empty());
        p.observe_lhs_and_rhs(JsValue::from_i32(1), JsValue::from_double(2.5));
        assert!(p.lhs_observed_type().saw_int32());
        assert!(p.rhs_observed_type().saw_number() && !p.rhs_observed_type().saw_int32());
        assert_eq!(p.bits(), (0x1 << 10) | (0x2 << 7));
        // ArithProfile.h:343: the special-fast-path bit is not touched by observe*.
        assert!(!p.took_special_fast_path());
    }

    // === ExecutionCounter (ExecutionCounter.{h,cpp}) ===

    #[test]
    fn formatted_total_execution_count_reinterprets_float_bits() {
        // ExecutionCounter.h:46-54 (union { int32_t i; float f; }).
        assert_eq!(formatted_total_execution_count(0.0), 0);
        assert_eq!(
            formatted_total_execution_count(1000.0),
            1000.0_f32.to_bits() as i32
        );
    }

    #[test]
    fn execution_counter_count_is_total_plus_counter() {
        // ExecutionCounter.h:66 (count = m_totalCount + m_counter).
        let counter = BytecodeExecutionCounter {
            counter: -600,
            total_count: 1000.0,
            active_threshold: 1000,
            variant: CountingVariant::Baseline,
        };
        assert_eq!(counter.count(), 400.0);
    }

    #[test]
    fn execution_counter_set_new_threshold_seeds_down_counter() {
        // ExecutionCounter.cpp:60-66 (setNewThreshold) + 163-197 (setThreshold).
        let env = ExecutionCounterEnvironment::with_max_counts(1_000_000);
        let mut counter = BytecodeExecutionCounter::new(CountingVariant::Baseline);
        counter.set_new_threshold(1000, env);
        assert_eq!(counter.counter, -1000);
        assert_eq!(counter.total_count, 1000.0_f32);
        // No real executions have happened yet.
        assert_eq!(counter.count(), 0.0);
    }

    #[test]
    fn execution_counter_crosses_threshold_at_half_via_memory_heuristics() {
        // ExecutionCounter.cpp:150-156: desiredCount = modifiedThreshold
        //   - min(activeThreshold, maxCounts) / 2 -> early victory at the half mark.
        let env = ExecutionCounterEnvironment::with_max_counts(1_000_000);
        let mut counter = BytecodeExecutionCounter::new(CountingVariant::Baseline);
        counter.set_new_threshold(1000, env);

        // Counted up 400 of 1000 (counter -1000 -> -600): below desiredCount 500,
        // not crossed; setThreshold reseeds the down-counter to the same value.
        counter.counter = -600;
        assert!(!counter.check_if_threshold_crossed_and_set(env));
        assert_eq!(counter.counter, -600);

        // Counted up to the half mark (actualCount 500 >= desiredCount 500):
        // crossed.
        counter.counter = -500;
        assert!(counter.check_if_threshold_crossed_and_set(env));
    }

    #[test]
    fn execution_counter_defer_indefinitely_sets_sentinels() {
        // ExecutionCounter.cpp:68-74.
        let mut counter = BytecodeExecutionCounter::new(CountingVariant::UpperTiers);
        counter.defer_indefinitely();
        assert_eq!(counter.total_count, 0.0_f32);
        assert_eq!(counter.active_threshold, i32::MAX);
        assert_eq!(counter.counter, i32::MIN);
    }

    #[test]
    fn execution_counter_force_slow_path_zeroes_counter() {
        // ExecutionCounter.cpp:42-46.
        let mut counter = BytecodeExecutionCounter::new(CountingVariant::Baseline);
        counter.set_new_threshold(500, ExecutionCounterEnvironment::with_max_counts(1_000_000));
        counter.force_slow_path_concurrently();
        assert_eq!(counter.counter, 0);
    }

    #[test]
    fn apply_memory_usage_heuristics_scales_and_clamps() {
        // ExecutionCounter.cpp:76-100.
        assert_eq!(apply_memory_usage_heuristics(100, 2.0), 200.0);
        assert_eq!(
            apply_memory_usage_heuristics_and_convert_to_int(100, 2.0),
            200
        );
        assert_eq!(
            apply_memory_usage_heuristics_and_convert_to_int(i32::MAX, 2.0),
            i32::MAX
        );
    }

    #[test]
    fn clipped_threshold_caps_at_maximum_counts() {
        // ExecutionCounter.h:69-76.
        assert_eq!(
            BytecodeExecutionCounter::clipped_threshold(5000.0, 1000),
            1000.0
        );
        assert_eq!(
            BytecodeExecutionCounter::clipped_threshold(500.0, 1000),
            500.0
        );
    }
}
