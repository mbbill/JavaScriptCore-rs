use crate::bytecode::code_block::{BitVectorRef, BytecodeIndex, Checkpoint, RuntimeSlot};
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;
use crate::gc::StructureId;

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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct UnlinkedValueProfile {
    pub prediction: SpeculatedTypeSet,
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
