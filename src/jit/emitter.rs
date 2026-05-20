//! Non-executable baseline byte emitters.
//!
//! This module owns construction of machine-byte images for future baseline
//! backends. It deliberately stays separate from `baseline`, which is the typed
//! executor stand-in, and from `emission`, which records byte provenance. The
//! bytes produced here are not callable authority and are not executable
//! readiness.

use crate::assembler::{
    freeze_assembler_byte_image, link_assembler_byte_image, plan_link_buffer_layout,
    AssemblerArchitecture, AssemblerBufferBuilder, AssemblerBufferDescriptor, AssemblerBufferId,
    AssemblerBufferLifecycle, AssemblerByteImage, AssemblerByteImageId, AssemblerDataKind,
    AssemblerValidationError, LinkBufferProfile, LinkBufferState, LinkedAssemblerByteImage,
};
use crate::bytecode::{
    Checkpoint, CodeBlock, CoreOpcode, DecodedInstruction, InstructionDecodeError, Opcode, Operand,
    OperandAccessError, OperandWidth, RegisterClass, ThisArgumentOffset, VirtualRegister,
};
use crate::jit::abi::{
    AbiValue, BaselineAbiValidationError, BaselineReturnCarrier, EntryAbi, EntrypointKind,
    RegisterBinding, RegisterRole, BASELINE_ABI_DESCRIPTOR,
};
use crate::jit::code::{BaselineEntryArtifact, JitType as CodeJitType};
use crate::jit::emission::{
    record_baseline_machine_code_emission, BaselineMachineCodeEmissionRecord,
    BaselineMachineCodeEmissionRequest, BaselineMachineCodeEmissionValidationError,
    BaselineMachineCodeEmitterKind,
};
use crate::jit::plan::{
    bind_baseline_bytecode_proof_owner, validate_baseline_bytecode_proof_code_block_snapshot,
    validate_baseline_generated_property_handoff_site_metadata, BaselineBytecodeEligibilityProof,
    BaselineBytecodeProofBindingError, BaselineBytecodeRange, BaselineBytecodeSnapshotFingerprint,
    BaselineExceptionMetadataPresence, BaselineGeneratedEffectContract,
    BaselineGeneratedJsCallNativeExitPlanMetadata, BaselineGeneratedPropertyHandoffPlanMetadata,
    BaselineGeneratedPropertyHandoffSite, BaselineGeneratedRuntimeBoundaryProof,
    BaselineGeneratedRuntimeHelperPlanMetadata, BaselineSupportedOpcodeSubset,
    JitPlanValidationError,
};
use crate::runtime::CodeBlockId;
use crate::strings::{AtomId, Identifier, PropertyKey};
use crate::value::{
    static_value_representation_layout, ImmediateKind, ValueRepresentationValidationError,
};
use std::fmt::Debug;

const P6_X86_64_NON_CALLABLE_RETURN_STUB_BYTES: &[u8] = &[0x55, 0x48, 0x89, 0xe5, 0x5d, 0xc3];
const P6_X86_64_CALLABLE_PROLOGUE_BYTES: &[u8] = &[
    0x55, // push rbp
    0x41, 0x57, // push r15
    0x48, 0x89, 0xf5, // mov rbp, rsi
    0x49, 0x89, 0xff, // mov r15, rdi
];
const P6_X86_64_CALLABLE_EPILOGUE_BYTES: &[u8] = &[
    0x41, 0x5f, // pop r15
    0x5d, // pop rbp
    0xc3, // ret
];
// Legacy VM discriminator kept exported while callable stubs move to identity payloads.
pub const P6_X86_64_BASELINE_CALLABLE_SIDE_EXIT_SENTINEL: u64 = 0;
pub const P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG: u8 = 0xff;
pub const P6_X86_64_BASELINE_RUNTIME_HELPER_NATIVE_EXIT_PAYLOAD_INDEX_BASE: u32 = 0x8000_0000;
pub const P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG: u8 = 0xfe;
pub const P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG: u8 = 0xfd;
pub const P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG: u8 = 0xfc;
pub const P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSideExitReturnPayload(u64);

impl P6X86_64BaselineSideExitReturnPayload {
    pub const fn encode(side_exit_index: u32) -> Self {
        Self(
            ((side_exit_index as u64) << 8)
                | P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG as u64,
        )
    }

    pub const fn decode(raw_bits: u64) -> Option<Self> {
        if (raw_bits & 0xff) != P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG as u64 {
            return None;
        }
        if (raw_bits >> 8) > u32::MAX as u64 {
            return None;
        }
        Some(Self(raw_bits))
    }

    pub const fn raw_bits(self) -> u64 {
        self.0
    }

    pub const fn side_exit_index(self) -> u32 {
        (self.0 >> 8) as u32
    }

    pub const fn low_tag(self) -> u8 {
        (self.0 & 0xff) as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P9X86_64BaselineJsCallNativeExitReturnPayload(u64);

impl P9X86_64BaselineJsCallNativeExitReturnPayload {
    pub const fn encode(call_exit_index: u32) -> Self {
        Self(
            ((call_exit_index as u64) << 8)
                | P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG as u64,
        )
    }

    pub const fn decode(raw_bits: u64) -> Option<Self> {
        if (raw_bits & 0xff) != P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG as u64
        {
            return None;
        }
        if (raw_bits >> 8) > u32::MAX as u64 {
            return None;
        }
        Some(Self(raw_bits))
    }

    pub const fn raw_bits(self) -> u64 {
        self.0
    }

    pub const fn call_exit_index(self) -> u32 {
        (self.0 >> 8) as u32
    }

    pub const fn low_tag(self) -> u8 {
        (self.0 & 0xff) as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P10X86_64BaselinePropertyNativeExitReturnPayload(u64);

impl P10X86_64BaselinePropertyNativeExitReturnPayload {
    pub const fn encode(property_exit_index: u32) -> Self {
        Self(
            ((property_exit_index as u64) << 8)
                | P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG as u64,
        )
    }

    pub const fn decode(raw_bits: u64) -> Option<Self> {
        if (raw_bits & 0xff)
            != P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG as u64
        {
            return None;
        }
        if (raw_bits >> 8) > u32::MAX as u64 {
            return None;
        }
        Some(Self(raw_bits))
    }

    pub const fn raw_bits(self) -> u64 {
        self.0
    }

    pub const fn property_exit_index(self) -> u32 {
        (self.0 >> 8) as u32
    }

    pub const fn low_tag(self) -> u8 {
        (self.0 & 0xff) as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P14X86_64BaselineLoopBackedgeReturnPayload(u64);

impl P14X86_64BaselineLoopBackedgeReturnPayload {
    pub const fn encode(backedge_index: u32) -> Self {
        Self(
            ((backedge_index as u64) << 8)
                | P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG as u64,
        )
    }

    pub const fn decode(raw_bits: u64) -> Option<Self> {
        if (raw_bits & 0xff) != P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG as u64 {
            return None;
        }
        if (raw_bits >> 8) > u32::MAX as u64 {
            return None;
        }
        Some(Self(raw_bits))
    }

    pub const fn raw_bits(self) -> u64 {
        self.0
    }

    pub const fn backedge_index(self) -> u32 {
        (self.0 >> 8) as u32
    }

    pub const fn low_tag(self) -> u8 {
        (self.0 & 0xff) as u8
    }
}

#[derive(Clone, Copy, Debug)]
pub struct P6X86_64BaselineLoweringRequest<'a> {
    pub owner: CodeBlockId,
    pub code_block: &'a CodeBlock,
    pub eligibility_proof: BaselineBytecodeEligibilityProof,
    pub(crate) backedge_safepoints: &'a [P14X86_64BaselineBackedgeSafepointAuthority],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct P14X86_64BaselineBackedgeSafepointAuthority {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    pub(crate) source_bytecode_index: crate::bytecode::BytecodeIndex,
    pub(crate) target_bytecode_index: crate::bytecode::BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) kind: P6X86_64BaselineBytecodeBranchKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineLoweringResult {
    pub emitter_kind: BaselineMachineCodeEmitterKind,
    pub byte_emission: P6X86_64BaselineLoweringByteEmission,
    pub callable_authority: P6X86_64BaselineLoweringCallableAuthority,
    pub plan: P6X86_64BaselineLoweringPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineLoweringPlan {
    pub owner: CodeBlockId,
    pub bytecode: BaselineBytecodeRange,
    pub opcode_subset: BaselineSupportedOpcodeSubset,
    pub effect_contract: BaselineGeneratedEffectContract,
    pub operations: Vec<P6X86_64BaselineLoweredInstruction>,
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineLoweredInstruction {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub width: OperandWidth,
    pub operation: P6X86_64BaselineLoweredOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineLoweredOperation {
    LoadUndefined {
        destination: VirtualRegister,
    },
    LoadNull {
        destination: VirtualRegister,
    },
    LoadBool {
        destination: VirtualRegister,
        raw_immediate: u32,
        value: bool,
    },
    LoadInt32 {
        destination: VirtualRegister,
        value: i32,
    },
    Move {
        destination: VirtualRegister,
        source: VirtualRegister,
    },
    Return {
        source: VirtualRegister,
    },
    AddInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    SubInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    MulInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    Jump {
        target: crate::bytecode::BytecodeIndex,
    },
    JumpIfNotNullish {
        source: VirtualRegister,
        target: crate::bytecode::BytecodeIndex,
    },
    JumpIfFalse {
        source: VirtualRegister,
        target: crate::bytecode::BytecodeIndex,
    },
    RuntimeHelperNativeExit {
        opcode: CoreOpcode,
        safepoint: crate::jit::plan::CompilerSafepointId,
        root_map: crate::bytecode::BytecodeRootMapId,
        root_count: usize,
        requires_no_gc_exit_reentry: bool,
        may_throw: bool,
        encoded_payload: P6X86_64BaselineSideExitReturnPayload,
    },
    JsCallNativeExit {
        opcode: CoreOpcode,
        destination: VirtualRegister,
        callee: VirtualRegister,
        this_register: Option<VirtualRegister>,
        provided_argument_count: u32,
        argument_register_count: u16,
        argument_registers: [Option<VirtualRegister>;
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS],
        resume_bytecode_index: Option<crate::bytecode::BytecodeIndex>,
        requires_no_gc_exit_reentry: bool,
        may_throw: bool,
        encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload,
    },
    PropertyNativeExit {
        site: BaselineGeneratedPropertyHandoffSite,
        operands: P10X86_64BaselinePropertyNativeExitOperands,
        encoded_payload: P10X86_64BaselinePropertyNativeExitReturnPayload,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P10X86_64BaselinePropertyNativeExitOperands {
    GetByName {
        destination: VirtualRegister,
        base: VirtualRegister,
    },
    PutByName {
        base: VirtualRegister,
        value: VirtualRegister,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineLoweringByteEmission {
    NotGenerated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineLoweringCallableAuthority {
    NoCallableAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineBackendContractRecord {
    pub owner: CodeBlockId,
    pub bytecode: BaselineBytecodeRange,
    pub opcode_subset: BaselineSupportedOpcodeSubset,
    pub effect_contract: BaselineGeneratedEffectContract,
    pub emitter_kind: BaselineMachineCodeEmitterKind,
    pub architecture: AssemblerArchitecture,
    pub value_layout: P6X86_64BaselineValueLayoutContract,
    pub frame_layout: P6X86_64BaselineFrameLayoutContract,
    pub abi: P6X86_64BaselineSymbolicAbiContract,
    pub instructions: Vec<P6X86_64BaselineBackendInstructionContract>,
    pub byte_emission: P6X86_64BaselineLoweringByteEmission,
    pub callable_authority: P6X86_64BaselineLoweringCallableAuthority,
    pub artifact_contract: P6X86_64BaselineBackendArtifactContract,
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineValueLayoutContract {
    pub layout_name: &'static str,
    pub storage_bits: u8,
    pub slot_width_bytes: u8,
    pub tag_mask: u64,
    pub payload_shift: u8,
    pub immediate_undefined_tag: u64,
    pub immediate_null_tag: u64,
    pub immediate_false_tag: u64,
    pub immediate_true_tag: u64,
    pub immediate_int32_tag: u64,
    pub immediate_double_tag: u64,
    pub cell_tag: u64,
    pub double_tag: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineFrameLayoutContract {
    pub value_slot_width_bytes: u8,
    pub local_zero_slot_index: u32,
    pub local_slot_stride_bytes: u8,
    pub this_argument_offset: ThisArgumentOffset,
    pub header_registers_below_this_are_value_addressable: bool,
    pub constants: P6X86_64BaselineConstantsLocationContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineConstantsLocationContract {
    ReadOnlyOutOfFrame,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSymbolicAbiContract {
    pub descriptor_name: &'static str,
    pub entry_kind: EntrypointKind,
    pub entry_abi: EntryAbi,
    pub frame_abi: EntryAbi,
    pub pinned_vm: RegisterBinding,
    pub pinned_call_frame: RegisterBinding,
    pub js_value_return: P6X86_64BaselineReturnRegisterContract,
    pub stack_alignment_bytes: u16,
    pub stack_alignment_applies_at_entry: bool,
    pub stack_alignment_applies_at_runtime_calls: bool,
    pub runtime_clobbered_roles: Vec<RegisterRole>,
    pub runtime_preserved_roles: Vec<RegisterRole>,
    pub runtime_clobbers_condition_flags: bool,
    pub runtime_clobbers_stack_argument_area: bool,
    pub runtime_call_may_allocate: bool,
    pub runtime_call_may_throw: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineReturnRegisterContract {
    pub role: RegisterRole,
    pub value: AbiValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineBackendInstructionContract {
    pub lowered: P6X86_64BaselineLoweredInstruction,
    pub operand_locations: Vec<P6X86_64BaselineOperandLocationRecord>,
    pub arithmetic_exit_policy: Option<P6X86_64BaselineInt32ArithmeticExitPolicy>,
    pub branch_target: Option<P6X86_64BaselineControlFlowBranchContract>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineOperandLocationRecord {
    pub role: P6X86_64BaselineOperandRole,
    pub location: P6X86_64BaselineOperandLocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineOperandRole {
    Destination,
    Source,
    Left,
    Right,
    ReturnValue,
    BranchTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineOperandLocation {
    FrameLocal {
        local_index: u32,
        slot_index: u32,
        byte_offset: u64,
    },
    FrameArgument {
        argument_index_including_this: u32,
        raw_virtual_register: i32,
        byte_offset_from_argument_base: u64,
    },
    Constant {
        constant_index: u32,
        read_only: bool,
    },
    ReturnCarrier {
        role: RegisterRole,
        value: AbiValue,
    },
    Immediate(P6X86_64BaselineImmediateOperand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineImmediateOperand {
    Undefined,
    Null,
    Boolean(bool),
    Int32(i32),
    BytecodeIndex(crate::bytecode::BytecodeIndex),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineInt32ArithmeticExitPolicy {
    pub operation: P6X86_64BaselineInt32ArithmeticOperation,
    pub operand_guard: P6X86_64BaselineInt32OperandGuard,
    pub checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
    pub non_int32_exit: P6X86_64BaselineArithmeticSideExitContract,
    pub overflow_exit: P6X86_64BaselineArithmeticSideExitContract,
    pub negative_zero_policy: P6X86_64BaselineMulNegativeZeroPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineControlFlowBranchContract {
    pub kind: P6X86_64BaselineBytecodeBranchKind,
    pub source_bytecode_index: crate::bytecode::BytecodeIndex,
    pub target_bytecode_index: crate::bytecode::BytecodeIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineBytecodeBranchKind {
    UnconditionalJump,
    JumpIfNotNullishTaken,
    JumpIfFalseTaken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInt32ArithmeticOperation {
    Add,
    Sub,
    Mul,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInt32OperandGuard {
    GuardBothOperandsWithInt32Tag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineCheckedInt32Arithmetic {
    CheckedAdd,
    CheckedSub,
    CheckedMul,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineArithmeticSideExitContract {
    pub reason: P6X86_64BaselineArithmeticSideExitReason,
    pub destination: P6X86_64BaselineSideExitDestinationEffect,
    pub retained_bytecode_index: crate::bytecode::BytecodeIndex,
    pub may_throw: bool,
    pub runtime_call: bool,
    pub heap_allocation: bool,
    pub touches_gc_roots: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineArithmeticSideExitReason {
    NonInt32Operand,
    Overflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSideExitDestinationEffect {
    DestinationUnchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineMulNegativeZeroPolicy {
    NotApplicable,
    NegativeZeroSideExitRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineBackendArtifactContract {
    pub assembler_byte_images: P6X86_64BaselineBackendArtifactPresence,
    pub machine_handles: P6X86_64BaselineBackendArtifactPresence,
    pub native_ids: P6X86_64BaselineBackendArtifactPresence,
    pub jit_artifacts: P6X86_64BaselineBackendArtifactPresence,
    pub vm_materialization: P6X86_64BaselineBackendArtifactPresence,
    pub vm_readiness: P6X86_64BaselineBackendArtifactPresence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineBackendArtifactPresence {
    Absent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineInstructionSelectionPlan {
    pub owner: CodeBlockId,
    pub bytecode: BaselineBytecodeRange,
    pub opcode_subset: BaselineSupportedOpcodeSubset,
    pub effect_contract: BaselineGeneratedEffectContract,
    pub emitter_kind: BaselineMachineCodeEmitterKind,
    pub architecture: AssemblerArchitecture,
    pub byte_emission: P6X86_64BaselineInstructionSelectionByteEmission,
    pub callable_authority: P6X86_64BaselineInstructionSelectionCallableAuthority,
    pub artifact_contract: P6X86_64BaselineBackendArtifactContract,
    pub readiness: P6X86_64BaselineInstructionSelectionReadiness,
    pub instructions: Vec<P6X86_64BaselineSelectedInstruction>,
    proof: P6X86_64BaselineInstructionSelectionProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInstructionSelectionByteEmission {
    NotGenerated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInstructionSelectionCallableAuthority {
    NoCallableAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInstructionSelectionReadiness {
    NoArtifactsNoVmPlatformExecutableReadiness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSelectedInstruction {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub lowered: P6X86_64BaselineLoweredInstruction,
    pub operand_locations: Vec<P6X86_64BaselineOperandLocationRecord>,
    pub effects: P6X86_64BaselineSelectedInstructionEffects,
    pub machine_instructions: Vec<P6X86_64BaselineMachineInstruction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSelectedInstructionEffects {
    pub may_throw: bool,
    pub runtime_call: bool,
    pub heap_allocation: bool,
    pub touches_gc_roots: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSymbolicRegister {
    ReturnGpr,
    Scratch0,
    Scratch1,
    Scratch2,
    PinnedCallFrameBase,
    PinnedVm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineMachineOperand {
    Register(P6X86_64BaselineSymbolicRegister),
    Immediate64(u64),
    Memory(P6X86_64BaselineMachineMemoryOperand),
    ReturnCarrier(P6X86_64BaselineReturnRegisterContract),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineMachineMemoryOperand {
    pub base: P6X86_64BaselineSymbolicRegister,
    pub location: P6X86_64BaselineOperandLocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineMachineInstruction {
    MoveQ {
        destination: P6X86_64BaselineMachineOperand,
        source: P6X86_64BaselineMachineOperand,
    },
    LoadQ {
        destination: P6X86_64BaselineSymbolicRegister,
        source: P6X86_64BaselineMachineOperand,
    },
    StoreQ {
        destination: P6X86_64BaselineMachineOperand,
        source: P6X86_64BaselineSymbolicRegister,
    },
    CheckTagEquals {
        value: P6X86_64BaselineSymbolicRegister,
        tag_mask: u64,
        expected_tag: u64,
        on_not_equal: P6X86_64BaselineSideExitLabel,
    },
    ExtractInt32Payload {
        destination: P6X86_64BaselineSymbolicRegister,
        source: P6X86_64BaselineSymbolicRegister,
        payload_shift: u8,
    },
    CheckedInt32Arithmetic {
        operation: P6X86_64BaselineInt32ArithmeticOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        on_overflow: P6X86_64BaselineSideExitLabel,
    },
    CheckMulNegativeZero {
        result: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        on_negative_zero: P6X86_64BaselineSideExitLabel,
    },
    RetagInt32 {
        destination: P6X86_64BaselineSymbolicRegister,
        payload: P6X86_64BaselineSymbolicRegister,
        payload_shift: u8,
        tag: u64,
    },
    ReturnRuntimeHelperNativeExitPayload {
        encoded_payload: P6X86_64BaselineSideExitReturnPayload,
    },
    ReturnJsCallNativeExitPayload {
        encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload,
    },
    ReturnPropertyNativeExitPayload {
        encoded_payload: P10X86_64BaselinePropertyNativeExitReturnPayload,
    },
    Jump {
        target: P6X86_64BaselineControlFlowBranchContract,
    },
    BranchIfNotNullish {
        value: P6X86_64BaselineSymbolicRegister,
        undefined_tag: u64,
        null_tag: u64,
        target: P6X86_64BaselineControlFlowBranchContract,
    },
    BranchIfFalsePrimitive {
        value: P6X86_64BaselineSymbolicRegister,
        undefined_tag: u64,
        null_tag: u64,
        false_tag: u64,
        true_tag: u64,
        int32_tag: u64,
        unsupported_exit: P6X86_64BaselineSideExitLabel,
        target: P6X86_64BaselineControlFlowBranchContract,
    },
    SetReturnCarrier {
        carrier: P6X86_64BaselineReturnRegisterContract,
        source: P6X86_64BaselineSymbolicRegister,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSideExitLabel {
    pub reason: P6X86_64BaselineSelectedSideExitReason,
    pub retained_bytecode_index: crate::bytecode::BytecodeIndex,
    pub destination: P6X86_64BaselineSideExitDestinationEffect,
    pub may_throw: bool,
    pub runtime_call: bool,
    pub heap_allocation: bool,
    pub touches_gc_roots: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSelectedSideExitReason {
    NonInt32Operand,
    Overflow,
    NegativeZero,
    UnsupportedTruthinessOperand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P6X86_64BaselineInstructionSelectionProof {
    contract_fingerprint: u128,
    selection_fingerprint: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInstructionSelectionError {
    Contract {
        error: P6X86_64BaselineBackendContractError,
    },
    UnexpectedArtifactContract {
        actual: P6X86_64BaselineBackendArtifactContract,
    },
    UnexpectedSelectionByteEmission {
        actual: P6X86_64BaselineInstructionSelectionByteEmission,
    },
    UnexpectedSelectionCallableAuthority {
        actual: P6X86_64BaselineInstructionSelectionCallableAuthority,
    },
    UnexpectedSelectionReadiness {
        actual: P6X86_64BaselineInstructionSelectionReadiness,
    },
    SelectionProofMismatch,
    PlanContractMismatch {
        field: &'static str,
    },
    BackendContractMismatch {
        field: &'static str,
    },
    InstructionBytecodeRangeMismatch {
        field: &'static str,
        expected: crate::bytecode::BytecodeIndex,
        actual: crate::bytecode::BytecodeIndex,
    },
    InstructionBytecodeOrderMismatch {
        previous: crate::bytecode::BytecodeIndex,
        actual: crate::bytecode::BytecodeIndex,
    },
    InstructionCountMismatch {
        expected: usize,
        actual: usize,
    },
    UnexpectedOperandRoles {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected: Vec<P6X86_64BaselineOperandRole>,
        actual: Vec<P6X86_64BaselineOperandRole>,
    },
    InvalidOperandLocation {
        bytecode_index: crate::bytecode::BytecodeIndex,
        role: P6X86_64BaselineOperandRole,
        location: P6X86_64BaselineOperandLocation,
        reason: P6X86_64BaselineInstructionSelectionOperandLocationError,
    },
    OperandLocationMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        role: P6X86_64BaselineOperandRole,
        expected: P6X86_64BaselineOperandLocation,
        actual: P6X86_64BaselineOperandLocation,
    },
    ImmediateSourceMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected: P6X86_64BaselineImmediateOperand,
        actual: P6X86_64BaselineOperandLocation,
    },
    ReturnCarrierMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected: P6X86_64BaselineReturnRegisterContract,
        actual: P6X86_64BaselineOperandLocation,
    },
    MissingArithmeticPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnexpectedArithmeticPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
        actual: P6X86_64BaselineInt32ArithmeticExitPolicy,
    },
    ArithmeticPolicyMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_operation: P6X86_64BaselineInt32ArithmeticOperation,
        expected_checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
        actual: P6X86_64BaselineInt32ArithmeticExitPolicy,
    },
    SideExitContractMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_reason: P6X86_64BaselineArithmeticSideExitReason,
        actual: P6X86_64BaselineArithmeticSideExitContract,
    },
    NegativeZeroPolicyMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected: P6X86_64BaselineMulNegativeZeroPolicy,
        actual: P6X86_64BaselineMulNegativeZeroPolicy,
    },
    MissingBranchTargetContract {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnexpectedBranchTargetContract {
        bytecode_index: crate::bytecode::BytecodeIndex,
        actual: P6X86_64BaselineControlFlowBranchContract,
    },
    BranchTargetContractMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected: P6X86_64BaselineControlFlowBranchContract,
        actual: P6X86_64BaselineControlFlowBranchContract,
    },
    SelectedInstructionMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInstructionSelectionOperandLocationError {
    ExpectedWritableValueSlot,
    ExpectedReadableValueSource,
    ConstantSourceMustBeReadOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineBackendContractError {
    UnexpectedEmitterKind {
        expected: BaselineMachineCodeEmitterKind,
        actual: BaselineMachineCodeEmitterKind,
    },
    UnexpectedByteEmission {
        actual: P6X86_64BaselineLoweringByteEmission,
    },
    UnexpectedCallableAuthority {
        actual: P6X86_64BaselineLoweringCallableAuthority,
    },
    UnexpectedArchitecture {
        expected: AssemblerArchitecture,
        actual: AssemblerArchitecture,
    },
    UnsupportedOpcodeSubset {
        emitter: BaselineMachineCodeEmitterKind,
        expected: BaselineSupportedOpcodeSubset,
        actual: BaselineSupportedOpcodeSubset,
    },
    UnsupportedEffectContract {
        emitter: BaselineMachineCodeEmitterKind,
        expected: BaselineGeneratedEffectContract,
        actual: BaselineGeneratedEffectContract,
    },
    ValueLayoutInvalid {
        error: ValueRepresentationValidationError,
    },
    UnsupportedValueLayout {
        layout_name: &'static str,
        storage_bits: u8,
        slot_width_bytes: u8,
        tag_mask: u64,
        payload_shift: u8,
    },
    MissingImmediateTag {
        canonical_name: &'static str,
    },
    UnexpectedImmediateTagKind {
        canonical_name: &'static str,
        expected: ImmediateKind,
        actual: ImmediateKind,
    },
    DoubleImmediateTagMismatch {
        immediate_double_tag: u64,
        double_tag: u64,
    },
    AbiDescriptorInvalid {
        error: BaselineAbiValidationError,
    },
    MissingPinnedRegister {
        role: RegisterRole,
    },
    MissingReturnConvention,
    UnsupportedReturnCarrier,
    UnexpectedReturnValue {
        expected: AbiValue,
        actual: AbiValue,
    },
    OperandLocation {
        bytecode_index: crate::bytecode::BytecodeIndex,
        role: P6X86_64BaselineOperandRole,
        register: VirtualRegister,
        error: P6X86_64BaselineOperandLocationError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineOperandLocationError {
    InvalidRegister,
    HeaderAsValueOperandUnsupported { raw_slot: u32 },
    ConstantDestination { constant_index: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineLoweringRequirement {
    MatchingCodeBlockSnapshotNoRootsNoExceptionHandlers,
    MatchingCodeBlockSnapshotWithRuntimeHelperNativeExitRootMapsNoExceptionHandlers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineLoweringValidationShape {
    pub bytecode: BaselineBytecodeRange,
    pub proof_root_map_count: usize,
    pub proof_safepoint_count: usize,
    pub proof_complete_safepoint_root_map_count: usize,
    pub proof_exception_metadata: BaselineExceptionMetadataPresence,
    pub code_block_linked_root_map_count: usize,
    pub code_block_unlinked_root_map_count: usize,
    pub code_block_linked_handler_count: usize,
    pub code_block_unlinked_handler_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineLoweringError {
    UnsupportedOpcodeSubset {
        emitter: BaselineMachineCodeEmitterKind,
        expected: BaselineSupportedOpcodeSubset,
        actual: BaselineSupportedOpcodeSubset,
    },
    UnsupportedEffectContract {
        emitter: BaselineMachineCodeEmitterKind,
        expected: BaselineGeneratedEffectContract,
        actual: BaselineGeneratedEffectContract,
    },
    OwnerWitnessMismatch {
        owner_witness: CodeBlockId,
        proof_owner: CodeBlockId,
    },
    ProofSnapshotOwnerMismatch {
        owner: CodeBlockId,
        snapshot_owner: CodeBlockId,
    },
    ProofSnapshotTierMismatch {
        from_tier: CodeJitType,
        to_tier: CodeJitType,
    },
    UnsupportedValidationShape {
        emitter: BaselineMachineCodeEmitterKind,
        requirement: P6X86_64BaselineLoweringRequirement,
        actual: P6X86_64BaselineLoweringValidationShape,
    },
    CodeBlockSnapshotInvalid {
        error: JitPlanValidationError,
    },
    CodeBlockSnapshotMismatch {
        owner: CodeBlockId,
    },
    InstructionDecode {
        error: InstructionDecodeError,
    },
    EmptyCodeBlock,
    BytecodeRangeMismatch {
        expected: BaselineBytecodeRange,
        actual: BaselineBytecodeRange,
    },
    UnsupportedOpcode {
        bytecode_index: crate::bytecode::BytecodeIndex,
        opcode: Opcode,
        core_opcode: Option<CoreOpcode>,
    },
    UnexpectedOperandCount {
        bytecode_index: crate::bytecode::BytecodeIndex,
        opcode: CoreOpcode,
        expected: usize,
        actual: usize,
    },
    UnsupportedOperandShape {
        bytecode_index: crate::bytecode::BytecodeIndex,
        opcode: CoreOpcode,
        error: OperandAccessError,
    },
    InvalidBranchTarget {
        bytecode_index: crate::bytecode::BytecodeIndex,
        opcode: CoreOpcode,
        target: crate::bytecode::BytecodeIndex,
        reason: P6X86_64BaselineBranchTargetRejectionReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineBranchTargetRejectionReason {
    InvalidBytecodeIndex,
    CheckpointedTarget,
    OutOfRange,
    SparseInstructionStart,
    SelfBranch,
    BackwardWithoutSafepointAuthority,
    RuntimeHelperNativeExit,
}

fn p6_lowering_error_from_proof_binding(
    error: BaselineBytecodeProofBindingError,
) -> P6X86_64BaselineLoweringError {
    match error {
        BaselineBytecodeProofBindingError::OwnerWitnessMismatch {
            owner_witness,
            proof_owner,
        } => P6X86_64BaselineLoweringError::OwnerWitnessMismatch {
            owner_witness,
            proof_owner,
        },
        BaselineBytecodeProofBindingError::ProofSnapshotOwnerMismatch {
            proof_owner,
            snapshot_owner,
        } => P6X86_64BaselineLoweringError::ProofSnapshotOwnerMismatch {
            owner: proof_owner,
            snapshot_owner,
        },
        BaselineBytecodeProofBindingError::ProofSnapshotTierMismatch { from_tier, to_tier } => {
            P6X86_64BaselineLoweringError::ProofSnapshotTierMismatch { from_tier, to_tier }
        }
        BaselineBytecodeProofBindingError::CodeBlockSnapshotInvalid { error } => {
            P6X86_64BaselineLoweringError::CodeBlockSnapshotInvalid { error }
        }
        BaselineBytecodeProofBindingError::CodeBlockSnapshotMismatch { owner } => {
            P6X86_64BaselineLoweringError::CodeBlockSnapshotMismatch { owner }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineMachineCodeByteGenerationRequest {
    pub entry_artifact: BaselineEntryArtifact,
    pub eligibility_proof: BaselineBytecodeEligibilityProof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineMachineCodeByteGenerationResult {
    pub emitter_kind: BaselineMachineCodeEmitterKind,
    pub byte_shape: BaselineMachineCodeByteGenerationShape,
    pub authority: BaselineMachineCodeByteGenerationAuthority,
    pub source_buffer: AssemblerBufferDescriptor,
    pub source_image: AssemblerByteImage,
    pub linked_image: LinkedAssemblerByteImage,
    pub emission: BaselineMachineCodeEmissionRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineMachineCodeByteGenerationShape {
    P6X86_64NonCallableReturnStub,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineMachineCodeByteGenerationAuthority {
    NonExecutableByteProvenanceOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineMachineCodeByteGenerationProofRequirement {
    SingleInstructionSameIndexNoCheckpointNoRootsNoExceptionHandlers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineMachineCodeByteGenerationProofShape {
    pub bytecode: BaselineBytecodeRange,
    pub root_map_count: usize,
    pub safepoint_count: usize,
    pub complete_safepoint_root_map_count: usize,
    pub exception_metadata: BaselineExceptionMetadataPresence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaselineMachineCodeByteGenerationError {
    Emission {
        reason: BaselineMachineCodeEmissionValidationError,
    },
    SourceBufferInvalid {
        error: AssemblerValidationError,
    },
    SourceImageInvalid {
        error: AssemblerValidationError,
    },
    LinkBufferLayoutInvalid {
        error: AssemblerValidationError,
    },
    LinkedImageInvalid {
        error: AssemblerValidationError,
    },
    UnsupportedProofShape {
        emitter: BaselineMachineCodeEmitterKind,
        requirement: BaselineMachineCodeByteGenerationProofRequirement,
        actual: BaselineMachineCodeByteGenerationProofShape,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSemanticByteEmissionResult {
    pub emitter_kind: BaselineMachineCodeEmitterKind,
    pub byte_shape: P6X86_64BaselineSemanticByteEmissionShape,
    pub authority: P6X86_64BaselineSemanticByteEmissionAuthority,
    pub physical_registers: P6X86_64BaselinePhysicalRegisterMap,
    pub terminal_policy: P6X86_64BaselineTerminalPolicyRecord,
    pub callable_prologue: Option<P6X86_64BaselineCallablePrologueRecord>,
    pub callable_normal_epilogue: Option<P6X86_64BaselineCallableEpilogueRecord>,
    pub instruction_bytes: Vec<P6X86_64BaselineInstructionByteRecord>,
    pub bytecode_branches: Vec<P6X86_64BaselineBytecodeBranchRecord>,
    pub side_exit_placeholders: Vec<P6X86_64BaselineSideExitPlaceholderRecord>,
    pub side_exit_return_stubs: Vec<P6X86_64BaselineSideExitReturnStubRecord>,
    pub loop_backedge_safepoint_stubs: Vec<P14X86_64BaselineLoopBackedgeSafepointStubRecord>,
    pub runtime_helper_native_exit_stubs: Vec<P6X86_64BaselineRuntimeHelperNativeExitStubRecord>,
    pub js_call_native_exit_stubs: Vec<P9X86_64BaselineJsCallNativeExitStubRecord>,
    pub property_native_exit_stubs: Vec<P10X86_64BaselinePropertyNativeExitStubRecord>,
    pub source_buffer: AssemblerBufferDescriptor,
    pub source_image: AssemblerByteImage,
    pub linked_image: LinkedAssemblerByteImage,
    pub entry_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSemanticByteEmissionShape {
    P2aSemanticX86_64FromAcceptedP6Selection,
    P3bCallableCAbiSemanticX86_64FromAcceptedP6Selection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSemanticByteEmissionAuthority {
    NonExecutableNonCallableSemanticBytesOnly,
    NonExecutableCallableSemanticBytesOnlyNoVmOrPlatformAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselinePhysicalRegisterMap {
    pub bindings: [P6X86_64BaselinePhysicalRegisterBinding; 6],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselinePhysicalRegisterBinding {
    pub symbolic: P6X86_64BaselineSymbolicRegister,
    pub physical: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineTerminalPolicy {
    SingleFinalNormalReturnRetThenInlineUd2SideExits,
    CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs,
    BytecodeBranchesSharedNormalReturnRetThenInlineUd2SideExits,
    CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineTerminalPolicyRecord {
    pub policy: P6X86_64BaselineTerminalPolicy,
    pub return_bytecode_index: crate::bytecode::BytecodeIndex,
    pub ret_offset: u32,
    pub normal_path_end_offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineInstructionByteRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub start_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub machine_instruction_count: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineBytecodeBranchRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub kind: P6X86_64BaselineBytecodeBranchKind,
    pub branch_offset: u32,
    pub rel32_offset: u32,
    pub branch_end_offset: u32,
    pub target_bytecode_index: crate::bytecode::BytecodeIndex,
    pub target_offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineCallablePrologueRecord {
    pub start_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineCallableEpilogueRecord {
    pub start_offset: u32,
    pub ret_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSideExitPlaceholderRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub reason: P6X86_64BaselineSelectedSideExitReason,
    pub branch_offset: u32,
    pub branch_end_offset: u32,
    pub target_offset: u32,
    pub placeholder_bytes: [u8; 2],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineSideExitReturnStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub reason: P6X86_64BaselineSelectedSideExitReason,
    pub side_exit_index: u32,
    pub branch_offset: u32,
    pub branch_end_offset: u32,
    pub target_offset: u32,
    pub stub_end_offset: u32,
    pub byte_len: u32,
    pub encoded_payload: P6X86_64BaselineSideExitReturnPayload,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P14X86_64BaselineLoopBackedgeSafepointStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub opcode: CoreOpcode,
    pub kind: P6X86_64BaselineBytecodeBranchKind,
    pub backedge_index: u32,
    pub branch_offset: u32,
    pub branch_end_offset: u32,
    pub safepoint_stub_offset: u32,
    pub stub_end_offset: u32,
    pub byte_len: u32,
    pub target_bytecode_index: crate::bytecode::BytecodeIndex,
    pub target_instruction_offset: u32,
    pub encoded_payload: P14X86_64BaselineLoopBackedgeReturnPayload,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineRuntimeHelperNativeExitStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub opcode: CoreOpcode,
    pub safepoint: crate::jit::plan::CompilerSafepointId,
    pub root_map: crate::bytecode::BytecodeRootMapId,
    pub root_count: usize,
    pub requires_no_gc_exit_reentry: bool,
    pub may_throw: bool,
    pub start_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub encoded_payload: P6X86_64BaselineSideExitReturnPayload,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P9X86_64BaselineJsCallNativeExitStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub opcode: CoreOpcode,
    pub destination: VirtualRegister,
    pub callee: VirtualRegister,
    pub this_register: Option<VirtualRegister>,
    pub provided_argument_count: u32,
    pub argument_registers: Vec<VirtualRegister>,
    pub resume_bytecode_index: Option<crate::bytecode::BytecodeIndex>,
    pub requires_no_gc_exit_reentry: bool,
    pub may_throw: bool,
    pub start_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P10X86_64BaselinePropertyNativeExitStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub site: BaselineGeneratedPropertyHandoffSite,
    pub operands: P10X86_64BaselinePropertyNativeExitOperands,
    pub start_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub encoded_payload: P10X86_64BaselinePropertyNativeExitReturnPayload,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSemanticByteEmissionError {
    Selection {
        error: P6X86_64BaselineInstructionSelectionError,
    },
    UnexpectedInstructionEffects {
        bytecode_index: crate::bytecode::BytecodeIndex,
        effects: P6X86_64BaselineSelectedInstructionEffects,
    },
    UnexpectedSideExitEffects {
        bytecode_index: crate::bytecode::BytecodeIndex,
        reason: P6X86_64BaselineSelectedSideExitReason,
    },
    MissingReturn,
    MultipleReturns {
        first: crate::bytecode::BytecodeIndex,
        second: crate::bytecode::BytecodeIndex,
    },
    NonFinalReturn {
        bytecode_index: crate::bytecode::BytecodeIndex,
        next_bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnsupportedOperandLocation {
        bytecode_index: crate::bytecode::BytecodeIndex,
        location: P6X86_64BaselineOperandLocation,
        reason: P6X86_64BaselineSemanticOperandRejectionReason,
    },
    UnsupportedMemoryBase {
        bytecode_index: crate::bytecode::BytecodeIndex,
        base: P6X86_64BaselineSymbolicRegister,
    },
    UnsupportedMachineInstruction {
        bytecode_index: crate::bytecode::BytecodeIndex,
        instruction: P6X86_64BaselineMachineInstruction,
    },
    UnsupportedImmediateTag {
        bytecode_index: crate::bytecode::BytecodeIndex,
        tag: u64,
    },
    UnsupportedValueLayout {
        field: &'static str,
        actual: u64,
    },
    FrameOffsetOutOfDisp32 {
        bytecode_index: crate::bytecode::BytecodeIndex,
        location: P6X86_64BaselineOperandLocation,
        byte_offset: u64,
    },
    ImageLengthExceedsU32 {
        actual: usize,
    },
    BranchDisplacementOutOfRange {
        branch_offset: u32,
        branch_end_offset: u32,
        target_offset: u32,
    },
    SideExitIndexExceedsPayloadCapacity {
        side_exit_index: usize,
    },
    BranchPatchOutOfRange {
        branch_offset: u32,
    },
    BranchTargetMissing {
        bytecode_index: crate::bytecode::BytecodeIndex,
        target: crate::bytecode::BytecodeIndex,
    },
    RuntimeHelperNativeExitRequiresCallable {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    JsCallNativeExitRequiresCallable {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    PropertyNativeExitRequiresCallable {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    MalformedJsCallNativeExit {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    SourceBufferInvalid {
        error: AssemblerValidationError,
    },
    SourceImageInvalid {
        error: AssemblerValidationError,
    },
    LinkBufferLayoutInvalid {
        error: AssemblerValidationError,
    },
    LinkedImageInvalid {
        error: AssemblerValidationError,
    },
    SourceBufferInvariant {
        field: &'static str,
    },
    SourceImageInvariant {
        field: &'static str,
    },
    LinkedImageInvariant {
        field: &'static str,
    },
    EntryOffsetOutOfRange {
        entry_offset: u32,
        image_size_bytes: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSemanticOperandRejectionReason {
    FrameArgumentUnsupported,
    ConstantMemoryUnsupported,
    ReturnCarrierMetadataOnly,
    ExpectedFrameLocalMemory,
}

impl BaselineMachineCodeByteGenerationRequest {
    pub const fn new(
        entry_artifact: BaselineEntryArtifact,
        eligibility_proof: BaselineBytecodeEligibilityProof,
    ) -> Self {
        Self {
            entry_artifact,
            eligibility_proof,
        }
    }
}

impl<'a> P6X86_64BaselineLoweringRequest<'a> {
    pub const fn new(
        owner: CodeBlockId,
        code_block: &'a CodeBlock,
        eligibility_proof: BaselineBytecodeEligibilityProof,
    ) -> Self {
        Self {
            owner,
            code_block,
            eligibility_proof,
            backedge_safepoints: &[],
        }
    }

    pub(crate) const fn new_with_backedge_safepoints(
        owner: CodeBlockId,
        code_block: &'a CodeBlock,
        eligibility_proof: BaselineBytecodeEligibilityProof,
        backedge_safepoints: &'a [P14X86_64BaselineBackedgeSafepointAuthority],
    ) -> Self {
        Self {
            owner,
            code_block,
            eligibility_proof,
            backedge_safepoints,
        }
    }
}

impl P6X86_64BaselineLoweringValidationShape {
    pub(crate) fn from_code_block_and_proof(
        code_block: &CodeBlock,
        proof: &BaselineBytecodeEligibilityProof,
    ) -> Self {
        let root_map_requirements = proof.root_map_requirements();
        let linked_side_tables = code_block.side_tables();
        let unlinked_side_tables = code_block.unlinked().side_tables();
        Self {
            bytecode: proof.bytecode(),
            proof_root_map_count: root_map_requirements.root_map_count,
            proof_safepoint_count: root_map_requirements.safepoint_count,
            proof_complete_safepoint_root_map_count: root_map_requirements
                .complete_safepoint_root_map_count,
            proof_exception_metadata: proof.exception_metadata(),
            code_block_linked_root_map_count: linked_side_tables.root_maps.len(),
            code_block_unlinked_root_map_count: unlinked_side_tables.root_maps.len(),
            code_block_linked_handler_count: linked_side_tables
                .handlers
                .len()
                .saturating_add(linked_side_tables.exception_handlers.handlers.len()),
            code_block_unlinked_handler_count: unlinked_side_tables
                .handlers
                .len()
                .saturating_add(unlinked_side_tables.exception_handlers.handlers.len()),
        }
    }
}

impl P6X86_64BaselineBackendContractRecord {
    pub fn from_lowering_result(
        lowering: &P6X86_64BaselineLoweringResult,
    ) -> Result<Self, P6X86_64BaselineBackendContractError> {
        let expected_emitter = p6_x86_64_emitter_kind_for_subset(lowering.plan.opcode_subset);
        if lowering.emitter_kind != expected_emitter {
            return Err(
                P6X86_64BaselineBackendContractError::UnexpectedEmitterKind {
                    expected: expected_emitter,
                    actual: lowering.emitter_kind,
                },
            );
        }
        if lowering.byte_emission != P6X86_64BaselineLoweringByteEmission::NotGenerated {
            return Err(
                P6X86_64BaselineBackendContractError::UnexpectedByteEmission {
                    actual: lowering.byte_emission,
                },
            );
        }
        if lowering.callable_authority
            != P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority
        {
            return Err(
                P6X86_64BaselineBackendContractError::UnexpectedCallableAuthority {
                    actual: lowering.callable_authority,
                },
            );
        }

        Self::from_lowering_plan(&lowering.plan)
    }

    pub fn from_lowering_plan(
        plan: &P6X86_64BaselineLoweringPlan,
    ) -> Result<Self, P6X86_64BaselineBackendContractError> {
        let emitter_kind = p6_x86_64_emitter_kind_for_subset(plan.opcode_subset);
        validate_p6_x86_64_backend_contract_subset_and_effect_contract(
            emitter_kind,
            plan.opcode_subset,
            plan.effect_contract,
        )?;

        let architecture = emitter_kind.expected_architecture();
        if architecture != AssemblerArchitecture::X86_64 {
            return Err(
                P6X86_64BaselineBackendContractError::UnexpectedArchitecture {
                    expected: AssemblerArchitecture::X86_64,
                    actual: architecture,
                },
            );
        }

        let value_layout = p6_x86_64_baseline_value_layout_contract()?;
        let frame_layout = p6_x86_64_baseline_frame_layout_contract(value_layout);
        let abi = p6_x86_64_baseline_symbolic_abi_contract()?;
        let return_carrier = abi.js_value_return;
        let instructions = plan
            .operations
            .iter()
            .copied()
            .map(|lowered| {
                p6_x86_64_baseline_backend_instruction_contract(
                    lowered,
                    frame_layout,
                    return_carrier,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            owner: plan.owner,
            bytecode: plan.bytecode,
            opcode_subset: plan.opcode_subset,
            effect_contract: plan.effect_contract,
            emitter_kind,
            architecture,
            value_layout,
            frame_layout,
            abi,
            instructions,
            byte_emission: P6X86_64BaselineLoweringByteEmission::NotGenerated,
            callable_authority: P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority,
            artifact_contract: P6X86_64BaselineBackendArtifactContract::absent(),
            bytecode_snapshot: plan.bytecode_snapshot,
        })
    }
}

impl P6X86_64BaselineBackendArtifactContract {
    pub const fn absent() -> Self {
        Self {
            assembler_byte_images: P6X86_64BaselineBackendArtifactPresence::Absent,
            machine_handles: P6X86_64BaselineBackendArtifactPresence::Absent,
            native_ids: P6X86_64BaselineBackendArtifactPresence::Absent,
            jit_artifacts: P6X86_64BaselineBackendArtifactPresence::Absent,
            vm_materialization: P6X86_64BaselineBackendArtifactPresence::Absent,
            vm_readiness: P6X86_64BaselineBackendArtifactPresence::Absent,
        }
    }
}

impl P6X86_64BaselineSelectedInstructionEffects {
    pub const fn no_runtime_allocation_or_roots() -> Self {
        Self {
            may_throw: false,
            runtime_call: false,
            heap_allocation: false,
            touches_gc_roots: false,
        }
    }
}

impl P6X86_64BaselineInstructionSelectionPlan {
    pub fn validate_against(
        &self,
        contract: &P6X86_64BaselineBackendContractRecord,
    ) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
        validate_p6_x86_64_instruction_selection_contract(contract)?;

        if self.proof != self.expected_proof(contract) {
            return Err(P6X86_64BaselineInstructionSelectionError::SelectionProofMismatch);
        }
        if self.owner != contract.owner {
            return Err(
                P6X86_64BaselineInstructionSelectionError::PlanContractMismatch { field: "owner" },
            );
        }
        if self.bytecode != contract.bytecode {
            return Err(
                P6X86_64BaselineInstructionSelectionError::PlanContractMismatch {
                    field: "bytecode",
                },
            );
        }
        if self.opcode_subset != contract.opcode_subset {
            return Err(
                P6X86_64BaselineInstructionSelectionError::PlanContractMismatch {
                    field: "opcode_subset",
                },
            );
        }
        if self.effect_contract != contract.effect_contract {
            return Err(
                P6X86_64BaselineInstructionSelectionError::PlanContractMismatch {
                    field: "effect_contract",
                },
            );
        }
        if self.emitter_kind != contract.emitter_kind {
            return Err(
                P6X86_64BaselineInstructionSelectionError::PlanContractMismatch {
                    field: "emitter_kind",
                },
            );
        }
        if self.architecture != contract.architecture {
            return Err(
                P6X86_64BaselineInstructionSelectionError::PlanContractMismatch {
                    field: "architecture",
                },
            );
        }
        if self.byte_emission != P6X86_64BaselineInstructionSelectionByteEmission::NotGenerated {
            return Err(
                P6X86_64BaselineInstructionSelectionError::UnexpectedSelectionByteEmission {
                    actual: self.byte_emission,
                },
            );
        }
        if self.callable_authority
            != P6X86_64BaselineInstructionSelectionCallableAuthority::NoCallableAuthority
        {
            return Err(
                P6X86_64BaselineInstructionSelectionError::UnexpectedSelectionCallableAuthority {
                    actual: self.callable_authority,
                },
            );
        }
        if self.readiness
            != P6X86_64BaselineInstructionSelectionReadiness::NoArtifactsNoVmPlatformExecutableReadiness
        {
            return Err(
                P6X86_64BaselineInstructionSelectionError::UnexpectedSelectionReadiness {
                    actual: self.readiness,
                },
            );
        }
        if self.artifact_contract != P6X86_64BaselineBackendArtifactContract::absent() {
            return Err(
                P6X86_64BaselineInstructionSelectionError::UnexpectedArtifactContract {
                    actual: self.artifact_contract,
                },
            );
        }
        if self.instructions.len() != contract.instructions.len() {
            return Err(
                P6X86_64BaselineInstructionSelectionError::InstructionCountMismatch {
                    expected: contract.instructions.len(),
                    actual: self.instructions.len(),
                },
            );
        }
        if self.instructions.len() != contract.bytecode.instruction_count as usize {
            return Err(
                P6X86_64BaselineInstructionSelectionError::InstructionCountMismatch {
                    expected: contract.bytecode.instruction_count as usize,
                    actual: self.instructions.len(),
                },
            );
        }

        for (selected, contract_instruction) in
            self.instructions.iter().zip(contract.instructions.iter())
        {
            let expected = p6_select_x86_64_instruction(contract, contract_instruction)?;
            if selected != &expected {
                return Err(
                    P6X86_64BaselineInstructionSelectionError::SelectedInstructionMismatch {
                        bytecode_index: contract_instruction.lowered.bytecode_index,
                    },
                );
            }
        }

        Ok(())
    }

    fn expected_proof(
        &self,
        contract: &P6X86_64BaselineBackendContractRecord,
    ) -> P6X86_64BaselineInstructionSelectionProof {
        P6X86_64BaselineInstructionSelectionProof {
            contract_fingerprint: p6_selection_fingerprint(contract),
            selection_fingerprint: p6_selection_plan_fingerprint(self),
        }
    }
}

pub fn record_p6_x86_64_baseline_backend_contract(
    lowering: &P6X86_64BaselineLoweringResult,
) -> Result<P6X86_64BaselineBackendContractRecord, P6X86_64BaselineBackendContractError> {
    P6X86_64BaselineBackendContractRecord::from_lowering_result(lowering)
}

pub fn record_p6_x86_64_baseline_backend_contract_from_plan(
    plan: &P6X86_64BaselineLoweringPlan,
) -> Result<P6X86_64BaselineBackendContractRecord, P6X86_64BaselineBackendContractError> {
    P6X86_64BaselineBackendContractRecord::from_lowering_plan(plan)
}

pub fn select_p6_x86_64_baseline_instructions(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> Result<P6X86_64BaselineInstructionSelectionPlan, P6X86_64BaselineInstructionSelectionError> {
    validate_p6_x86_64_instruction_selection_contract(contract)?;

    let instructions = contract
        .instructions
        .iter()
        .map(|instruction| p6_select_x86_64_instruction(contract, instruction))
        .collect::<Result<Vec<_>, _>>()?;

    let mut plan = P6X86_64BaselineInstructionSelectionPlan {
        owner: contract.owner,
        bytecode: contract.bytecode,
        opcode_subset: contract.opcode_subset,
        effect_contract: contract.effect_contract,
        emitter_kind: contract.emitter_kind,
        architecture: contract.architecture,
        byte_emission: P6X86_64BaselineInstructionSelectionByteEmission::NotGenerated,
        callable_authority:
            P6X86_64BaselineInstructionSelectionCallableAuthority::NoCallableAuthority,
        artifact_contract: P6X86_64BaselineBackendArtifactContract::absent(),
        readiness: P6X86_64BaselineInstructionSelectionReadiness::NoArtifactsNoVmPlatformExecutableReadiness,
        instructions,
        proof: P6X86_64BaselineInstructionSelectionProof {
            contract_fingerprint: 0,
            selection_fingerprint: 0,
        },
    };
    plan.proof = plan.expected_proof(contract);
    plan.validate_against(contract)?;
    Ok(plan)
}

impl BaselineMachineCodeByteGenerationProofShape {
    pub(crate) fn from_proof(proof: &BaselineBytecodeEligibilityProof) -> Self {
        let root_map_requirements = proof.root_map_requirements();
        Self {
            bytecode: proof.bytecode(),
            root_map_count: root_map_requirements.root_map_count,
            safepoint_count: root_map_requirements.safepoint_count,
            complete_safepoint_root_map_count: root_map_requirements
                .complete_safepoint_root_map_count,
            exception_metadata: proof.exception_metadata(),
        }
    }
}

pub fn plan_p6_x86_64_baseline_lowering(
    request: P6X86_64BaselineLoweringRequest<'_>,
) -> Result<P6X86_64BaselineLoweringResult, P6X86_64BaselineLoweringError> {
    let emitter_kind = p6_x86_64_emitter_kind_for_subset(request.eligibility_proof.opcode_subset());
    let binding = bind_baseline_bytecode_proof_owner(request.owner, &request.eligibility_proof)
        .map_err(p6_lowering_error_from_proof_binding)?;
    validate_p6_x86_64_lowering_subset_and_effect_contract(
        emitter_kind,
        request.eligibility_proof.opcode_subset(),
        request.eligibility_proof.generated_effect_contract(),
    )?;
    validate_p6_x86_64_lowering_shape(
        emitter_kind,
        request.code_block,
        &request.eligibility_proof,
    )?;
    validate_baseline_bytecode_proof_code_block_snapshot(request.code_block, &binding)
        .map_err(p6_lowering_error_from_proof_binding)?;

    let (bytecode, operations) = collect_p6_x86_64_lowering_operations(
        request.code_block,
        request.eligibility_proof.opcode_subset(),
    )?;
    validate_p6_x86_64_branch_targets(
        &operations,
        binding.owner,
        binding.bytecode,
        binding.bytecode_snapshot,
        request.backedge_safepoints,
    )?;
    if bytecode != binding.bytecode {
        return Err(P6X86_64BaselineLoweringError::BytecodeRangeMismatch {
            expected: binding.bytecode,
            actual: bytecode,
        });
    }

    Ok(P6X86_64BaselineLoweringResult {
        emitter_kind,
        byte_emission: P6X86_64BaselineLoweringByteEmission::NotGenerated,
        callable_authority: P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority,
        plan: P6X86_64BaselineLoweringPlan {
            owner: binding.owner,
            bytecode,
            opcode_subset: request.eligibility_proof.opcode_subset(),
            effect_contract: request.eligibility_proof.generated_effect_contract(),
            operations,
            bytecode_snapshot: binding.bytecode_snapshot,
        },
    })
}

#[cfg(test)]
pub(crate) fn plan_p6_x86_64_baseline_lowering_with_runtime_helper_native_exits(
    request: P6X86_64BaselineLoweringRequest<'_>,
    runtime_helper_plan: &BaselineGeneratedRuntimeHelperPlanMetadata,
) -> Result<P6X86_64BaselineLoweringResult, P6X86_64BaselineLoweringError> {
    plan_p6_x86_64_baseline_lowering_with_native_exits(
        request,
        Some(runtime_helper_plan),
        None,
        None,
    )
}

pub(crate) fn plan_p6_x86_64_baseline_lowering_with_native_exits(
    request: P6X86_64BaselineLoweringRequest<'_>,
    runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    js_call_native_exit_plan: Option<&BaselineGeneratedJsCallNativeExitPlanMetadata>,
    property_native_exit_plan: Option<&BaselineGeneratedPropertyHandoffPlanMetadata>,
) -> Result<P6X86_64BaselineLoweringResult, P6X86_64BaselineLoweringError> {
    let emitter_kind = p6_x86_64_emitter_kind_for_subset(request.eligibility_proof.opcode_subset());
    let binding = bind_baseline_bytecode_proof_owner(request.owner, &request.eligibility_proof)
        .map_err(p6_lowering_error_from_proof_binding)?;
    validate_p6_x86_64_lowering_subset_and_effect_contract(
        emitter_kind,
        request.eligibility_proof.opcode_subset(),
        request.eligibility_proof.generated_effect_contract(),
    )?;
    validate_p6_x86_64_lowering_shape_with_runtime_helper_native_exits(
        emitter_kind,
        request.code_block,
        &request.eligibility_proof,
        runtime_helper_plan,
        js_call_native_exit_plan,
        property_native_exit_plan,
    )?;
    validate_baseline_bytecode_proof_code_block_snapshot(request.code_block, &binding)
        .map_err(p6_lowering_error_from_proof_binding)?;
    if runtime_helper_plan
        .map(|plan| plan.bytecode_snapshot())
        .is_some_and(|snapshot| snapshot != binding.bytecode_snapshot)
        || js_call_native_exit_plan
            .map(|plan| plan.bytecode_snapshot())
            .is_some_and(|snapshot| snapshot != binding.bytecode_snapshot)
        || property_native_exit_plan
            .map(|plan| plan.bytecode_snapshot())
            .is_some_and(|snapshot| snapshot != binding.bytecode_snapshot)
    {
        return Err(P6X86_64BaselineLoweringError::CodeBlockSnapshotMismatch {
            owner: binding.owner,
        });
    }

    let (bytecode, operations) = collect_p6_x86_64_lowering_operations_with_native_exits(
        request.code_block,
        request.eligibility_proof.opcode_subset(),
        runtime_helper_plan,
        js_call_native_exit_plan,
        property_native_exit_plan,
    )?;
    validate_p6_x86_64_branch_targets(
        &operations,
        binding.owner,
        binding.bytecode,
        binding.bytecode_snapshot,
        request.backedge_safepoints,
    )?;
    if bytecode != binding.bytecode {
        return Err(P6X86_64BaselineLoweringError::BytecodeRangeMismatch {
            expected: binding.bytecode,
            actual: bytecode,
        });
    }

    Ok(P6X86_64BaselineLoweringResult {
        emitter_kind,
        byte_emission: P6X86_64BaselineLoweringByteEmission::NotGenerated,
        callable_authority: P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority,
        plan: P6X86_64BaselineLoweringPlan {
            owner: binding.owner,
            bytecode,
            opcode_subset: request.eligibility_proof.opcode_subset(),
            effect_contract: request.eligibility_proof.generated_effect_contract(),
            operations,
            bytecode_snapshot: binding.bytecode_snapshot,
        },
    })
}

pub fn emit_p6_x86_64_baseline_semantic_bytes(
    contract: P6X86_64BaselineBackendContractRecord,
    selection: P6X86_64BaselineInstructionSelectionPlan,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    selection
        .validate_against(&contract)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::Selection { error })?;
    validate_p6_x86_64_semantic_selection_effects(&selection)?;
    validate_p6_x86_64_non_callable_has_no_runtime_helper_native_exits(&selection)?;
    let terminal = validate_p6_x86_64_semantic_terminal_policy(&selection)?;
    let encoded = encode_p6_x86_64_semantic_selection(&contract, &selection, terminal)?;
    finish_p6_x86_64_semantic_byte_emission(
        &contract,
        encoded,
        P6X86_64BaselineSemanticByteEmissionShape::P2aSemanticX86_64FromAcceptedP6Selection,
        P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableNonCallableSemanticBytesOnly,
        p6_x86_64_semantic_source_buffer_id(&contract),
        p6_x86_64_semantic_source_image_id(&contract),
        0,
    )
}

pub fn emit_p6_x86_64_baseline_callable_semantic_bytes(
    contract: P6X86_64BaselineBackendContractRecord,
    selection: P6X86_64BaselineInstructionSelectionPlan,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    selection
        .validate_against(&contract)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::Selection { error })?;
    validate_p6_x86_64_semantic_selection_effects(&selection)?;
    let terminal = validate_p6_x86_64_semantic_terminal_policy(&selection)?;
    let encoded = encode_p6_x86_64_callable_semantic_selection(&contract, &selection, terminal)?;
    finish_p6_x86_64_semantic_byte_emission(
        &contract,
        encoded,
        P6X86_64BaselineSemanticByteEmissionShape::P3bCallableCAbiSemanticX86_64FromAcceptedP6Selection,
        P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableCallableSemanticBytesOnlyNoVmOrPlatformAuthority,
        p6_x86_64_callable_semantic_source_buffer_id(&contract),
        p6_x86_64_callable_semantic_source_image_id(&contract),
        0,
    )
}

fn finish_p6_x86_64_semantic_byte_emission(
    contract: &P6X86_64BaselineBackendContractRecord,
    encoded: P6X86_64SemanticEncodedSelection,
    byte_shape: P6X86_64BaselineSemanticByteEmissionShape,
    authority: P6X86_64BaselineSemanticByteEmissionAuthority,
    source_buffer_id: AssemblerBufferId,
    source_image_id: AssemblerByteImageId,
    entry_offset: u32,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    let P6X86_64SemanticEncodedSelection {
        bytes,
        terminal_policy,
        callable_prologue,
        callable_normal_epilogue,
        instruction_bytes,
        bytecode_branches,
        side_exit_placeholders,
        side_exit_return_stubs,
        loop_backedge_safepoint_stubs,
        runtime_helper_native_exit_stubs,
        js_call_native_exit_stubs,
        property_native_exit_stubs,
    } = encoded;
    let byte_len = p6_x86_64_checked_byte_len(bytes.len())?;

    let source_buffer = AssemblerBufferBuilder::new(source_buffer_id)
        .architecture(AssemblerArchitecture::X86_64)
        .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
        .capacity(byte_len, byte_len)
        .inline_capacity(byte_len)
        .build()
        .map_err(
            |error| P6X86_64BaselineSemanticByteEmissionError::SourceBufferInvalid { error },
        )?;
    let source_image = freeze_assembler_byte_image(&source_buffer, source_image_id, bytes)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::SourceImageInvalid { error })?;
    let layout = plan_link_buffer_layout(&source_buffer, LinkBufferProfile::Baseline, None)
        .map_err(
            |error| P6X86_64BaselineSemanticByteEmissionError::LinkBufferLayoutInvalid { error },
        )?;
    let linked_image = link_assembler_byte_image(&source_image, &layout)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvalid { error })?;
    validate_p6_x86_64_semantic_byte_images(
        &source_buffer,
        &source_image,
        &linked_image,
        byte_len,
        entry_offset,
    )?;

    Ok(P6X86_64BaselineSemanticByteEmissionResult {
        emitter_kind: contract.emitter_kind,
        byte_shape,
        authority,
        physical_registers: p6_x86_64_semantic_physical_register_map(),
        terminal_policy,
        callable_prologue,
        callable_normal_epilogue,
        instruction_bytes,
        bytecode_branches,
        side_exit_placeholders,
        side_exit_return_stubs,
        loop_backedge_safepoint_stubs,
        runtime_helper_native_exit_stubs,
        js_call_native_exit_stubs,
        property_native_exit_stubs,
        source_buffer,
        source_image,
        linked_image,
        entry_offset,
    })
}

pub fn emit_p6_x86_64_non_callable_return_stub(
    request: BaselineMachineCodeByteGenerationRequest,
) -> Result<BaselineMachineCodeByteGenerationResult, BaselineMachineCodeByteGenerationError> {
    let emitter_kind = BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset;
    validate_emitter_subset_and_effect_contract(
        emitter_kind,
        request.eligibility_proof.opcode_subset(),
        request.eligibility_proof.generated_effect_contract(),
    )?;
    validate_p6_x86_64_non_callable_return_stub_proof_shape(&request.eligibility_proof)?;

    let bytes = p6_x86_64_non_callable_return_stub_bytes();
    let source_buffer =
        AssemblerBufferBuilder::new(p6_x86_64_source_buffer_id(&request.entry_artifact))
            .architecture(emitter_kind.expected_architecture())
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(bytes.len() as u32, bytes.len() as u32)
            .inline_capacity(bytes.len() as u32)
            .build()
            .map_err(
                |error| BaselineMachineCodeByteGenerationError::SourceBufferInvalid { error },
            )?;
    let source_image = freeze_assembler_byte_image(
        &source_buffer,
        p6_x86_64_source_image_id(&request.entry_artifact),
        bytes.to_vec(),
    )
    .map_err(|error| BaselineMachineCodeByteGenerationError::SourceImageInvalid { error })?;
    let layout = plan_link_buffer_layout(
        &source_buffer,
        LinkBufferProfile::Baseline,
        Some(request.entry_artifact.machine_code.allocation),
    )
    .map_err(|error| BaselineMachineCodeByteGenerationError::LinkBufferLayoutInvalid { error })?;
    let linked_image = link_assembler_byte_image(&source_image, &layout)
        .map_err(|error| BaselineMachineCodeByteGenerationError::LinkedImageInvalid { error })?;
    let emission = record_baseline_machine_code_emission(BaselineMachineCodeEmissionRequest {
        emitter_kind,
        entry_artifact: &request.entry_artifact,
        eligibility_proof: &request.eligibility_proof,
        source_buffer: &source_buffer,
        source_image: &source_image,
        linked_image: Some(&linked_image),
        entry_offset: request.entry_artifact.machine_code.range.start_offset,
    })
    .map_err(|reason| BaselineMachineCodeByteGenerationError::Emission { reason })?;

    Ok(BaselineMachineCodeByteGenerationResult {
        emitter_kind,
        byte_shape: BaselineMachineCodeByteGenerationShape::P6X86_64NonCallableReturnStub,
        authority: BaselineMachineCodeByteGenerationAuthority::NonExecutableByteProvenanceOnly,
        source_buffer,
        source_image,
        linked_image,
        emission,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct P6X86_64SemanticTerminalSelection {
    return_instruction_indices: Vec<usize>,
    final_return_instruction_index: usize,
    return_bytecode_index: crate::bytecode::BytecodeIndex,
    branch_aware_returns: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct P6X86_64SemanticEncodedSelection {
    bytes: Vec<u8>,
    terminal_policy: P6X86_64BaselineTerminalPolicyRecord,
    callable_prologue: Option<P6X86_64BaselineCallablePrologueRecord>,
    callable_normal_epilogue: Option<P6X86_64BaselineCallableEpilogueRecord>,
    instruction_bytes: Vec<P6X86_64BaselineInstructionByteRecord>,
    bytecode_branches: Vec<P6X86_64BaselineBytecodeBranchRecord>,
    side_exit_placeholders: Vec<P6X86_64BaselineSideExitPlaceholderRecord>,
    side_exit_return_stubs: Vec<P6X86_64BaselineSideExitReturnStubRecord>,
    loop_backedge_safepoint_stubs: Vec<P14X86_64BaselineLoopBackedgeSafepointStubRecord>,
    runtime_helper_native_exit_stubs: Vec<P6X86_64BaselineRuntimeHelperNativeExitStubRecord>,
    js_call_native_exit_stubs: Vec<P9X86_64BaselineJsCallNativeExitStubRecord>,
    property_native_exit_stubs: Vec<P10X86_64BaselinePropertyNativeExitStubRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum P6X86_64SemanticBranchEmissionMode {
    DirectBytecodeTargets,
    CallableLoopBackedgeSafepoints,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P6X86_64PendingSideExitBranch {
    label: P6X86_64BaselineSideExitLabel,
    branch_offset: u32,
    rel32_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P6X86_64PendingInternalBranch {
    branch_offset: u32,
    rel32_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P6X86_64PendingBytecodeBranch {
    source: P6X86_64BaselineControlFlowBranchContract,
    branch_offset: u32,
    rel32_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P14X86_64PendingLoopBackedgeSafepointBranch {
    source: P6X86_64BaselineControlFlowBranchContract,
    opcode: CoreOpcode,
    branch_offset: u32,
    rel32_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct P6X86_64SemanticByteBuilder {
    bytes: Vec<u8>,
    pending_side_exits: Vec<P6X86_64PendingSideExitBranch>,
    pending_bytecode_branches: Vec<P6X86_64PendingBytecodeBranch>,
    pending_loop_backedges: Vec<P14X86_64PendingLoopBackedgeSafepointBranch>,
}

impl P6X86_64SemanticByteBuilder {
    fn offset(&self) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
        p6_x86_64_checked_byte_len(self.bytes.len())
    }

    fn emit(&mut self, bytes: &[u8]) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let next_len = self.bytes.len().checked_add(bytes.len()).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 { actual: usize::MAX },
        )?;
        p6_x86_64_checked_byte_len(next_len)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn emit_u8(&mut self, byte: u8) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
        let offset = self.offset()?;
        self.emit(&[byte])?;
        Ok(offset)
    }

    fn emit_i32_le(&mut self, value: i32) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        self.emit(&value.to_le_bytes())
    }

    fn emit_u64_le(&mut self, value: u64) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        self.emit(&value.to_le_bytes())
    }

    fn emit_callable_prologue(
        &mut self,
    ) -> Result<P6X86_64BaselineCallablePrologueRecord, P6X86_64BaselineSemanticByteEmissionError>
    {
        let start_offset = self.offset()?;
        self.emit(P6_X86_64_CALLABLE_PROLOGUE_BYTES)?;
        let end_offset = self.offset()?;
        Ok(P6X86_64BaselineCallablePrologueRecord {
            start_offset,
            end_offset,
            byte_len: end_offset.checked_sub(start_offset).ok_or(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: self.bytes.len(),
                },
            )?,
            bytes: self.bytes_for_range(start_offset, end_offset)?,
        })
    }

    fn emit_callable_epilogue(
        &mut self,
    ) -> Result<P6X86_64BaselineCallableEpilogueRecord, P6X86_64BaselineSemanticByteEmissionError>
    {
        let start_offset = self.offset()?;
        self.emit(P6_X86_64_CALLABLE_EPILOGUE_BYTES)?;
        let end_offset = self.offset()?;
        let ret_offset = end_offset.checked_sub(1).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: self.bytes.len(),
            },
        )?;
        Ok(P6X86_64BaselineCallableEpilogueRecord {
            start_offset,
            ret_offset,
            end_offset,
            byte_len: end_offset.checked_sub(start_offset).ok_or(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: self.bytes.len(),
                },
            )?,
            bytes: self.bytes_for_range(start_offset, end_offset)?,
        })
    }

    fn emit_callable_loop_reentry_prologue(
        &mut self,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        self.emit(P6_X86_64_CALLABLE_PROLOGUE_BYTES)
    }

    fn emit_jcc_rel32_side_exit(
        &mut self,
        bytecode_index: crate::bytecode::BytecodeIndex,
        opcode: u8,
        label: P6X86_64BaselineSideExitLabel,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        validate_p6_x86_64_semantic_side_exit_label(bytecode_index, label)?;
        let branch_offset = self.offset()?;
        self.emit(&[0x0f, opcode])?;
        let rel32_offset = self.offset()?;
        self.emit(&[0, 0, 0, 0])?;
        let branch_end_offset = self.offset()?;
        self.pending_side_exits.push(P6X86_64PendingSideExitBranch {
            label,
            branch_offset,
            rel32_offset,
            branch_end_offset,
        });
        Ok(())
    }

    fn emit_jne_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x85)
    }

    fn emit_je_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x84)
    }

    fn emit_jcc_rel32_internal(
        &mut self,
        opcode: u8,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        let branch_offset = self.offset()?;
        self.emit(&[0x0f, opcode])?;
        let rel32_offset = self.offset()?;
        self.emit(&[0, 0, 0, 0])?;
        let branch_end_offset = self.offset()?;
        Ok(P6X86_64PendingInternalBranch {
            branch_offset,
            rel32_offset,
            branch_end_offset,
        })
    }

    fn emit_jmp_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        let branch_offset = self.offset()?;
        self.emit(&[0xe9])?;
        let rel32_offset = self.offset()?;
        self.emit(&[0, 0, 0, 0])?;
        let branch_end_offset = self.offset()?;
        Ok(P6X86_64PendingInternalBranch {
            branch_offset,
            rel32_offset,
            branch_end_offset,
        })
    }

    fn patch_internal_branch_to_current(
        &mut self,
        branch: P6X86_64PendingInternalBranch,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let target_offset = self.offset()?;
        self.patch_internal_branch_to_target(branch, target_offset)
    }

    fn patch_internal_branch_to_target(
        &mut self,
        branch: P6X86_64PendingInternalBranch,
        target_offset: u32,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let displacement = p6_x86_64_rel32_displacement(
            branch.branch_offset,
            branch.branch_end_offset,
            target_offset,
        )?;
        self.patch_rel32(branch.rel32_offset, branch.branch_offset, displacement)
    }

    fn emit_jmp_rel32_bytecode_branch(
        &mut self,
        target: P6X86_64BaselineControlFlowBranchContract,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let branch_offset = self.offset()?;
        self.emit(&[0xe9])?;
        let rel32_offset = self.offset()?;
        self.emit(&[0, 0, 0, 0])?;
        let branch_end_offset = self.offset()?;
        self.pending_bytecode_branches
            .push(P6X86_64PendingBytecodeBranch {
                source: target,
                branch_offset,
                rel32_offset,
                branch_end_offset,
            });
        Ok(())
    }

    fn emit_jmp_rel32_loop_backedge_safepoint(
        &mut self,
        opcode: CoreOpcode,
        target: P6X86_64BaselineControlFlowBranchContract,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let branch_offset = self.offset()?;
        self.emit(&[0xe9])?;
        let rel32_offset = self.offset()?;
        self.emit(&[0, 0, 0, 0])?;
        let branch_end_offset = self.offset()?;
        self.pending_loop_backedges
            .push(P14X86_64PendingLoopBackedgeSafepointBranch {
                source: target,
                opcode,
                branch_offset,
                rel32_offset,
                branch_end_offset,
            });
        Ok(())
    }

    fn finish_bytecode_branches(
        &mut self,
        instruction_bytes: &[P6X86_64BaselineInstructionByteRecord],
    ) -> Result<Vec<P6X86_64BaselineBytecodeBranchRecord>, P6X86_64BaselineSemanticByteEmissionError>
    {
        let pending = self.pending_bytecode_branches.clone();
        let mut records = Vec::with_capacity(pending.len());
        for branch in pending {
            let target = instruction_bytes
                .iter()
                .find(|record| record.bytecode_index == branch.source.target_bytecode_index)
                .ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::BranchTargetMissing {
                        bytecode_index: branch.source.source_bytecode_index,
                        target: branch.source.target_bytecode_index,
                    },
                )?;
            let target_offset = target.start_offset;
            let displacement = p6_x86_64_rel32_displacement(
                branch.branch_offset,
                branch.branch_end_offset,
                target_offset,
            )?;
            self.patch_rel32(branch.rel32_offset, branch.branch_offset, displacement)?;
            records.push(P6X86_64BaselineBytecodeBranchRecord {
                bytecode_index: branch.source.source_bytecode_index,
                kind: branch.source.kind,
                branch_offset: branch.branch_offset,
                rel32_offset: branch.rel32_offset,
                branch_end_offset: branch.branch_end_offset,
                target_bytecode_index: branch.source.target_bytecode_index,
                target_offset,
            });
        }
        self.pending_bytecode_branches.clear();
        Ok(records)
    }

    fn finish_side_exit_placeholders(
        &mut self,
    ) -> Result<
        Vec<P6X86_64BaselineSideExitPlaceholderRecord>,
        P6X86_64BaselineSemanticByteEmissionError,
    > {
        let pending = self.pending_side_exits.clone();
        let mut records = Vec::with_capacity(pending.len());
        for branch in pending {
            let target_offset = self.offset()?;
            self.emit(&[0x0f, 0x0b])?;
            let displacement = p6_x86_64_rel32_displacement(
                branch.branch_offset,
                branch.branch_end_offset,
                target_offset,
            )?;
            self.patch_rel32(branch.rel32_offset, branch.branch_offset, displacement)?;
            records.push(P6X86_64BaselineSideExitPlaceholderRecord {
                bytecode_index: branch.label.retained_bytecode_index,
                reason: branch.label.reason,
                branch_offset: branch.branch_offset,
                branch_end_offset: branch.branch_end_offset,
                target_offset,
                placeholder_bytes: [0x0f, 0x0b],
            });
        }
        self.pending_side_exits.clear();
        Ok(records)
    }

    fn finish_side_exit_return_stubs(
        &mut self,
    ) -> Result<
        Vec<P6X86_64BaselineSideExitReturnStubRecord>,
        P6X86_64BaselineSemanticByteEmissionError,
    > {
        let pending = self.pending_side_exits.clone();
        let mut records = Vec::with_capacity(pending.len());
        for (side_exit_index, branch) in pending.into_iter().enumerate() {
            let side_exit_index = p6_x86_64_checked_side_exit_index(side_exit_index)?;
            let encoded_payload = P6X86_64BaselineSideExitReturnPayload::encode(side_exit_index);
            let stub_bytes =
                p6_x86_64_callable_side_exit_return_stub_bytes(encoded_payload.raw_bits());
            let target_offset = self.offset()?;
            self.emit(&stub_bytes)?;
            let stub_end_offset = self.offset()?;
            let displacement = p6_x86_64_rel32_displacement(
                branch.branch_offset,
                branch.branch_end_offset,
                target_offset,
            )?;
            self.patch_rel32(branch.rel32_offset, branch.branch_offset, displacement)?;
            records.push(P6X86_64BaselineSideExitReturnStubRecord {
                bytecode_index: branch.label.retained_bytecode_index,
                reason: branch.label.reason,
                side_exit_index,
                branch_offset: branch.branch_offset,
                branch_end_offset: branch.branch_end_offset,
                target_offset,
                stub_end_offset,
                byte_len: stub_end_offset.checked_sub(target_offset).ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                        actual: self.bytes.len(),
                    },
                )?,
                encoded_payload,
                bytes: self.bytes_for_range(target_offset, stub_end_offset)?,
            });
        }
        self.pending_side_exits.clear();
        Ok(records)
    }

    fn finish_loop_backedge_safepoint_stubs(
        &mut self,
        instruction_bytes: &[P6X86_64BaselineInstructionByteRecord],
        loop_backedge_reentry_offsets: &[(crate::bytecode::BytecodeIndex, u32)],
    ) -> Result<
        Vec<P14X86_64BaselineLoopBackedgeSafepointStubRecord>,
        P6X86_64BaselineSemanticByteEmissionError,
    > {
        let pending = self.pending_loop_backedges.clone();
        let mut records = Vec::with_capacity(pending.len());
        for (backedge_index, branch) in pending.into_iter().enumerate() {
            let backedge_index = p6_x86_64_checked_side_exit_index(backedge_index)?;
            let target_instruction = instruction_bytes
                .iter()
                .find(|record| record.bytecode_index == branch.source.target_bytecode_index)
                .ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::BranchTargetMissing {
                        bytecode_index: branch.source.source_bytecode_index,
                        target: branch.source.target_bytecode_index,
                    },
                )?;
            let target_instruction_offset = loop_backedge_reentry_offsets
                .iter()
                .find(|(bytecode_index, _)| *bytecode_index == branch.source.target_bytecode_index)
                .map(|(_, offset)| *offset)
                .unwrap_or(target_instruction.start_offset);
            let encoded_payload =
                P14X86_64BaselineLoopBackedgeReturnPayload::encode(backedge_index);
            let stub_bytes =
                p14_x86_64_callable_loop_backedge_return_stub_bytes(encoded_payload.raw_bits());
            let safepoint_stub_offset = self.offset()?;
            self.emit(&stub_bytes)?;
            let stub_end_offset = self.offset()?;
            let displacement = p6_x86_64_rel32_displacement(
                branch.branch_offset,
                branch.branch_end_offset,
                safepoint_stub_offset,
            )?;
            self.patch_rel32(branch.rel32_offset, branch.branch_offset, displacement)?;
            records.push(P14X86_64BaselineLoopBackedgeSafepointStubRecord {
                bytecode_index: branch.source.source_bytecode_index,
                opcode: branch.opcode,
                kind: branch.source.kind,
                backedge_index,
                branch_offset: branch.branch_offset,
                branch_end_offset: branch.branch_end_offset,
                safepoint_stub_offset,
                stub_end_offset,
                byte_len: stub_end_offset.checked_sub(safepoint_stub_offset).ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                        actual: self.bytes.len(),
                    },
                )?,
                target_bytecode_index: branch.source.target_bytecode_index,
                target_instruction_offset,
                encoded_payload,
                bytes: self.bytes_for_range(safepoint_stub_offset, stub_end_offset)?,
            });
        }
        self.pending_loop_backedges.clear();
        Ok(records)
    }

    fn patch_rel32(
        &mut self,
        rel32_offset: u32,
        branch_offset: u32,
        displacement: i32,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let start = rel32_offset as usize;
        let Some(end) = start.checked_add(4) else {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange { branch_offset },
            );
        };
        let Some(slice) = self.bytes.get_mut(start..end) else {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange { branch_offset },
            );
        };
        slice.copy_from_slice(&displacement.to_le_bytes());
        Ok(())
    }

    fn bytes_for_range(
        &self,
        start_offset: u32,
        end_offset: u32,
    ) -> Result<Vec<u8>, P6X86_64BaselineSemanticByteEmissionError> {
        let start = start_offset as usize;
        let end = end_offset as usize;
        let Some(slice) = self.bytes.get(start..end) else {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange {
                    branch_offset: start_offset,
                },
            );
        };
        Ok(slice.to_vec())
    }
}

fn p6_x86_64_semantic_physical_register_map() -> P6X86_64BaselinePhysicalRegisterMap {
    P6X86_64BaselinePhysicalRegisterMap {
        bindings: [
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::ReturnGpr,
                physical: "rax",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch0,
                physical: "r10",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch1,
                physical: "r11",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch2,
                physical: "ecx/rcx",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                physical: "rbp",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedVm,
                physical: "r15",
            },
        ],
    }
}

fn validate_p6_x86_64_semantic_selection_effects(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    for instruction in &selection.instructions {
        if instruction.effects
            != P6X86_64BaselineSelectedInstructionEffects::no_runtime_allocation_or_roots()
        {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::UnexpectedInstructionEffects {
                    bytecode_index: instruction.bytecode_index,
                    effects: instruction.effects,
                },
            );
        }
    }
    Ok(())
}

fn validate_p6_x86_64_non_callable_has_no_runtime_helper_native_exits(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    for instruction in &selection.instructions {
        match instruction.lowered.operation {
            P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit { .. } => {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::RuntimeHelperNativeExitRequiresCallable {
                        bytecode_index: instruction.bytecode_index,
                    },
                );
            }
            P6X86_64BaselineLoweredOperation::JsCallNativeExit { .. } => {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::JsCallNativeExitRequiresCallable {
                        bytecode_index: instruction.bytecode_index,
                    },
                );
            }
            P6X86_64BaselineLoweredOperation::PropertyNativeExit { .. } => {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::PropertyNativeExitRequiresCallable {
                        bytecode_index: instruction.bytecode_index,
                    },
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_p6_x86_64_semantic_terminal_policy(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> Result<P6X86_64SemanticTerminalSelection, P6X86_64BaselineSemanticByteEmissionError> {
    let branch_aware_returns = p6_x86_64_selection_uses_bytecode_branches(selection);
    let mut return_instruction_indices: Vec<usize> = Vec::new();
    for (index, instruction) in selection.instructions.iter().enumerate() {
        if matches!(
            instruction.lowered.operation,
            P6X86_64BaselineLoweredOperation::Return { .. }
        ) {
            if !branch_aware_returns && !return_instruction_indices.is_empty() {
                let first = selection.instructions[return_instruction_indices[0]].bytecode_index;
                return Err(P6X86_64BaselineSemanticByteEmissionError::MultipleReturns {
                    first,
                    second: instruction.bytecode_index,
                });
            }
            return_instruction_indices.push(index);
        }
    }

    let Some(&final_return_instruction_index) = return_instruction_indices.last() else {
        return Err(P6X86_64BaselineSemanticByteEmissionError::MissingReturn);
    };
    let return_bytecode_index =
        selection.instructions[final_return_instruction_index].bytecode_index;
    let last_index = selection.instructions.len().saturating_sub(1);
    if final_return_instruction_index != last_index {
        let next_bytecode_index =
            selection.instructions[final_return_instruction_index + 1].bytecode_index;
        return Err(P6X86_64BaselineSemanticByteEmissionError::NonFinalReturn {
            bytecode_index: return_bytecode_index,
            next_bytecode_index,
        });
    }

    Ok(P6X86_64SemanticTerminalSelection {
        return_instruction_indices,
        final_return_instruction_index,
        return_bytecode_index,
        branch_aware_returns,
    })
}

fn p6_x86_64_selection_uses_bytecode_branches(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> bool {
    selection.instructions.iter().any(|instruction| {
        matches!(
            instruction.lowered.operation,
            P6X86_64BaselineLoweredOperation::Jump { .. }
                | P6X86_64BaselineLoweredOperation::JumpIfNotNullish { .. }
                | P6X86_64BaselineLoweredOperation::JumpIfFalse { .. }
        )
    })
}

fn p14_x86_64_callable_loop_backedge_targets(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> Vec<crate::bytecode::BytecodeIndex> {
    let mut targets = Vec::new();
    for instruction in &selection.instructions {
        let target = match instruction.lowered.operation {
            P6X86_64BaselineLoweredOperation::Jump { target } => target,
            P6X86_64BaselineLoweredOperation::JumpIfNotNullish { target, .. } => target,
            P6X86_64BaselineLoweredOperation::JumpIfFalse { target, .. } => target,
            _ => continue,
        };
        if target < instruction.bytecode_index && !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

fn encode_p6_x86_64_semantic_selection(
    contract: &P6X86_64BaselineBackendContractRecord,
    selection: &P6X86_64BaselineInstructionSelectionPlan,
    terminal: P6X86_64SemanticTerminalSelection,
) -> Result<P6X86_64SemanticEncodedSelection, P6X86_64BaselineSemanticByteEmissionError> {
    validate_p6_x86_64_semantic_value_layout(contract.value_layout)?;

    let mut builder = P6X86_64SemanticByteBuilder::default();
    let mut instruction_bytes = Vec::with_capacity(selection.instructions.len());
    let mut ret_offset = 0;
    let mut pending_return_branches = Vec::new();
    for (index, instruction) in selection.instructions.iter().enumerate() {
        let start_offset = builder.offset()?;
        for machine_instruction in &instruction.machine_instructions {
            emit_p6_x86_64_semantic_machine_instruction(
                &mut builder,
                contract,
                instruction.bytecode_index,
                *machine_instruction,
                P6X86_64SemanticBranchEmissionMode::DirectBytecodeTargets,
            )?;
        }
        if terminal.return_instruction_indices.contains(&index) {
            if terminal.branch_aware_returns && index != terminal.final_return_instruction_index {
                pending_return_branches.push(builder.emit_jmp_rel32_internal()?);
            } else {
                ret_offset = builder.emit_u8(0xc3)?;
                for branch in pending_return_branches.drain(..) {
                    builder.patch_internal_branch_to_target(branch, ret_offset)?;
                }
            }
        }
        let end_offset = builder.offset()?;
        let byte_len = end_offset.checked_sub(start_offset).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: builder.bytes.len(),
            },
        )?;
        instruction_bytes.push(P6X86_64BaselineInstructionByteRecord {
            bytecode_index: instruction.bytecode_index,
            start_offset,
            end_offset,
            byte_len,
            machine_instruction_count: instruction.machine_instructions.len() as u32,
            bytes: builder.bytes_for_range(start_offset, end_offset)?,
        });
    }

    let normal_path_end_offset = builder.offset()?;
    let bytecode_branches = builder.finish_bytecode_branches(&instruction_bytes)?;
    let side_exit_placeholders = builder.finish_side_exit_placeholders()?;
    for record in &mut instruction_bytes {
        record.bytes = builder.bytes_for_range(record.start_offset, record.end_offset)?;
    }
    Ok(P6X86_64SemanticEncodedSelection {
        bytes: builder.bytes,
        terminal_policy: P6X86_64BaselineTerminalPolicyRecord {
            policy: if terminal.branch_aware_returns {
                P6X86_64BaselineTerminalPolicy::BytecodeBranchesSharedNormalReturnRetThenInlineUd2SideExits
            } else {
                P6X86_64BaselineTerminalPolicy::SingleFinalNormalReturnRetThenInlineUd2SideExits
            },
            return_bytecode_index: terminal.return_bytecode_index,
            ret_offset,
            normal_path_end_offset,
        },
        callable_prologue: None,
        callable_normal_epilogue: None,
        instruction_bytes,
        bytecode_branches,
        side_exit_placeholders,
        side_exit_return_stubs: Vec::new(),
        loop_backedge_safepoint_stubs: Vec::new(),
        runtime_helper_native_exit_stubs: Vec::new(),
        js_call_native_exit_stubs: Vec::new(),
        property_native_exit_stubs: Vec::new(),
    })
}

fn encode_p6_x86_64_callable_semantic_selection(
    contract: &P6X86_64BaselineBackendContractRecord,
    selection: &P6X86_64BaselineInstructionSelectionPlan,
    terminal: P6X86_64SemanticTerminalSelection,
) -> Result<P6X86_64SemanticEncodedSelection, P6X86_64BaselineSemanticByteEmissionError> {
    validate_p6_x86_64_semantic_value_layout(contract.value_layout)?;

    let mut builder = P6X86_64SemanticByteBuilder::default();
    let callable_prologue = builder.emit_callable_prologue()?;
    let mut instruction_bytes = Vec::with_capacity(selection.instructions.len());
    let mut runtime_helper_native_exit_stubs = Vec::new();
    let mut js_call_native_exit_stubs = Vec::new();
    let mut property_native_exit_stubs = Vec::new();
    let mut normal_epilogue = None;
    let mut pending_return_branches = Vec::new();
    let loop_backedge_targets = p14_x86_64_callable_loop_backedge_targets(selection);
    let mut loop_backedge_reentry_offsets = Vec::new();
    for (index, instruction) in selection.instructions.iter().enumerate() {
        let start_offset = builder.offset()?;
        if loop_backedge_targets.contains(&instruction.bytecode_index) {
            let normal_flow_skip = builder.emit_jmp_rel32_internal()?;
            let reentry_offset = builder.offset()?;
            builder.emit_callable_loop_reentry_prologue()?;
            loop_backedge_reentry_offsets.push((instruction.bytecode_index, reentry_offset));
            let body_offset = builder.offset()?;
            builder.patch_internal_branch_to_target(normal_flow_skip, body_offset)?;
        }
        for machine_instruction in &instruction.machine_instructions {
            emit_p6_x86_64_semantic_machine_instruction(
                &mut builder,
                contract,
                instruction.bytecode_index,
                *machine_instruction,
                P6X86_64SemanticBranchEmissionMode::CallableLoopBackedgeSafepoints,
            )?;
        }
        if terminal.return_instruction_indices.contains(&index) {
            if terminal.branch_aware_returns && index != terminal.final_return_instruction_index {
                pending_return_branches.push(builder.emit_jmp_rel32_internal()?);
            } else {
                let epilogue = builder.emit_callable_epilogue()?;
                for branch in pending_return_branches.drain(..) {
                    builder.patch_internal_branch_to_target(branch, epilogue.start_offset)?;
                }
                normal_epilogue = Some(epilogue);
            }
        }
        let end_offset = builder.offset()?;
        let byte_len = end_offset.checked_sub(start_offset).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: builder.bytes.len(),
            },
        )?;
        if let P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit {
            opcode,
            safepoint,
            root_map,
            root_count,
            requires_no_gc_exit_reentry,
            may_throw,
            encoded_payload,
        } = instruction.lowered.operation
        {
            runtime_helper_native_exit_stubs.push(
                P6X86_64BaselineRuntimeHelperNativeExitStubRecord {
                    bytecode_index: instruction.bytecode_index,
                    opcode,
                    safepoint,
                    root_map,
                    root_count,
                    requires_no_gc_exit_reentry,
                    may_throw,
                    start_offset,
                    end_offset,
                    byte_len,
                    encoded_payload,
                    bytes: builder.bytes_for_range(start_offset, end_offset)?,
                },
            );
        }
        if let P6X86_64BaselineLoweredOperation::JsCallNativeExit {
            opcode,
            destination,
            callee,
            this_register,
            provided_argument_count,
            argument_register_count,
            argument_registers,
            resume_bytecode_index,
            requires_no_gc_exit_reentry,
            may_throw,
            encoded_payload,
        } = instruction.lowered.operation
        {
            js_call_native_exit_stubs.push(P9X86_64BaselineJsCallNativeExitStubRecord {
                bytecode_index: instruction.bytecode_index,
                opcode,
                destination,
                callee,
                this_register,
                provided_argument_count,
                argument_registers: p9_js_call_argument_registers_from_lowered(
                    instruction.bytecode_index,
                    argument_register_count,
                    argument_registers,
                )?,
                resume_bytecode_index,
                requires_no_gc_exit_reentry,
                may_throw,
                start_offset,
                end_offset,
                byte_len,
                encoded_payload,
                bytes: builder.bytes_for_range(start_offset, end_offset)?,
            });
        }
        if let P6X86_64BaselineLoweredOperation::PropertyNativeExit {
            site,
            operands,
            encoded_payload,
        } = instruction.lowered.operation
        {
            property_native_exit_stubs.push(P10X86_64BaselinePropertyNativeExitStubRecord {
                bytecode_index: instruction.bytecode_index,
                site,
                operands,
                start_offset,
                end_offset,
                byte_len,
                encoded_payload,
                bytes: builder.bytes_for_range(start_offset, end_offset)?,
            });
        }
        instruction_bytes.push(P6X86_64BaselineInstructionByteRecord {
            bytecode_index: instruction.bytecode_index,
            start_offset,
            end_offset,
            byte_len,
            machine_instruction_count: instruction.machine_instructions.len() as u32,
            bytes: builder.bytes_for_range(start_offset, end_offset)?,
        });
    }

    let normal_path_end_offset = builder.offset()?;
    let normal_epilogue =
        normal_epilogue.ok_or(P6X86_64BaselineSemanticByteEmissionError::MissingReturn)?;
    let bytecode_branches = builder.finish_bytecode_branches(&instruction_bytes)?;
    let side_exit_return_stubs = builder.finish_side_exit_return_stubs()?;
    let loop_backedge_safepoint_stubs = builder
        .finish_loop_backedge_safepoint_stubs(&instruction_bytes, &loop_backedge_reentry_offsets)?;
    for record in &mut instruction_bytes {
        record.bytes = builder.bytes_for_range(record.start_offset, record.end_offset)?;
    }
    Ok(P6X86_64SemanticEncodedSelection {
        bytes: builder.bytes,
        terminal_policy: P6X86_64BaselineTerminalPolicyRecord {
            policy: if terminal.branch_aware_returns {
                P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs
            } else {
                P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs
            },
            return_bytecode_index: terminal.return_bytecode_index,
            ret_offset: normal_epilogue.ret_offset,
            normal_path_end_offset,
        },
        callable_prologue: Some(callable_prologue),
        callable_normal_epilogue: Some(normal_epilogue),
        instruction_bytes,
        bytecode_branches,
        side_exit_placeholders: Vec::new(),
        side_exit_return_stubs,
        loop_backedge_safepoint_stubs,
        runtime_helper_native_exit_stubs,
        js_call_native_exit_stubs,
        property_native_exit_stubs,
    })
}

fn emit_p6_x86_64_semantic_machine_instruction(
    builder: &mut P6X86_64SemanticByteBuilder,
    contract: &P6X86_64BaselineBackendContractRecord,
    bytecode_index: crate::bytecode::BytecodeIndex,
    instruction: P6X86_64BaselineMachineInstruction,
    branch_mode: P6X86_64SemanticBranchEmissionMode,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    match instruction {
        P6X86_64BaselineMachineInstruction::MoveQ {
            destination:
                P6X86_64BaselineMachineOperand::Register(P6X86_64BaselineSymbolicRegister::Scratch0),
            source: P6X86_64BaselineMachineOperand::Immediate64(value),
        } => {
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(value)
        }
        P6X86_64BaselineMachineInstruction::MoveQ { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination,
            source,
        } => {
            let disp32 = p6_x86_64_semantic_frame_local_disp32(bytecode_index, source)?;
            match destination {
                P6X86_64BaselineSymbolicRegister::Scratch0 => {
                    builder.emit(&[0x4c, 0x8b, 0x95])?;
                    builder.emit_i32_le(disp32)
                }
                P6X86_64BaselineSymbolicRegister::Scratch1 => {
                    builder.emit(&[0x4c, 0x8b, 0x9d])?;
                    builder.emit_i32_le(disp32)
                }
                P6X86_64BaselineSymbolicRegister::ReturnGpr => {
                    builder.emit(&[0x48, 0x8b, 0x85])?;
                    builder.emit_i32_le(disp32)
                }
                _ => Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction,
                    },
                ),
            }
        }
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        } => {
            let disp32 = p6_x86_64_semantic_frame_local_disp32(bytecode_index, destination)?;
            builder.emit(&[0x4c, 0x89, 0x95])?;
            builder.emit_i32_le(disp32)
        }
        P6X86_64BaselineMachineInstruction::StoreQ { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::CheckTagEquals {
            value,
            tag_mask,
            expected_tag,
            on_not_equal,
        } => {
            if tag_mask != contract.value_layout.tag_mask || tag_mask != 0xff {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                        field: "tag_mask",
                        actual: tag_mask,
                    },
                );
            }
            let tag = p6_x86_64_semantic_u8_tag(bytecode_index, expected_tag)?;
            match value {
                P6X86_64BaselineSymbolicRegister::Scratch0 => {
                    builder.emit(&[0x41, 0x80, 0xfa, tag])?
                }
                P6X86_64BaselineSymbolicRegister::Scratch1 => {
                    builder.emit(&[0x41, 0x80, 0xfb, tag])?
                }
                _ => {
                    return Err(
                        P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                            bytecode_index,
                            instruction,
                        },
                    );
                }
            }
            builder.emit_jcc_rel32_side_exit(bytecode_index, 0x85, on_not_equal)
        }
        P6X86_64BaselineMachineInstruction::ExtractInt32Payload {
            destination,
            source,
            payload_shift,
        } => {
            if payload_shift != contract.value_layout.payload_shift || payload_shift != 8 {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                        field: "payload_shift",
                        actual: u64::from(payload_shift),
                    },
                );
            }
            match (destination, source) {
                (
                    P6X86_64BaselineSymbolicRegister::Scratch0,
                    P6X86_64BaselineSymbolicRegister::Scratch0,
                ) => builder.emit(&[0x49, 0xc1, 0xea, 0x08]),
                (
                    P6X86_64BaselineSymbolicRegister::Scratch1,
                    P6X86_64BaselineSymbolicRegister::Scratch1,
                ) => builder.emit(&[0x49, 0xc1, 0xeb, 0x08]),
                _ => Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction,
                    },
                ),
            }
        }
        P6X86_64BaselineMachineInstruction::CheckedInt32Arithmetic {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch2,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            on_overflow,
        } => {
            builder.emit(&[0x44, 0x89, 0xd1])?;
            match operation {
                P6X86_64BaselineInt32ArithmeticOperation::Add => {
                    builder.emit(&[0x44, 0x01, 0xd9])?
                }
                P6X86_64BaselineInt32ArithmeticOperation::Sub => {
                    builder.emit(&[0x44, 0x29, 0xd9])?
                }
                P6X86_64BaselineInt32ArithmeticOperation::Mul => {
                    builder.emit(&[0x41, 0x0f, 0xaf, 0xcb])?
                }
            }
            builder.emit_jcc_rel32_side_exit(bytecode_index, 0x80, on_overflow)
        }
        P6X86_64BaselineMachineInstruction::CheckedInt32Arithmetic { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::CheckMulNegativeZero {
            result: P6X86_64BaselineSymbolicRegister::Scratch2,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            on_negative_zero,
        } => {
            builder.emit(&[0x85, 0xc9])?;
            let product_non_zero = builder.emit_jne_rel32_internal()?;
            builder.emit(&[0x45, 0x31, 0xda])?;
            builder.emit(&[0x45, 0x85, 0xd2])?;
            builder.emit_jcc_rel32_side_exit(bytecode_index, 0x88, on_negative_zero)?;
            builder.patch_internal_branch_to_current(product_non_zero)
        }
        P6X86_64BaselineMachineInstruction::CheckMulNegativeZero { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::RetagInt32 {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            payload: P6X86_64BaselineSymbolicRegister::Scratch2,
            payload_shift,
            tag,
        } => {
            if payload_shift != contract.value_layout.payload_shift || payload_shift != 8 {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                        field: "payload_shift",
                        actual: u64::from(payload_shift),
                    },
                );
            }
            let tag = p6_x86_64_semantic_u8_tag(bytecode_index, tag)?;
            builder.emit(&[0x41, 0x89, 0xca])?;
            builder.emit(&[0x49, 0xc1, 0xe2, 0x08])?;
            builder.emit(&[0x41, 0x80, 0xca, tag])
        }
        P6X86_64BaselineMachineInstruction::RetagInt32 { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::ReturnRuntimeHelperNativeExitPayload {
            encoded_payload,
        } => builder.emit(&p6_x86_64_callable_payload_return_stub_bytes(
            encoded_payload.raw_bits(),
        )),
        P6X86_64BaselineMachineInstruction::ReturnJsCallNativeExitPayload { encoded_payload } => {
            builder.emit(&p9_x86_64_callable_js_call_native_exit_return_stub_bytes(
                encoded_payload.raw_bits(),
            ))
        }
        P6X86_64BaselineMachineInstruction::ReturnPropertyNativeExitPayload { encoded_payload } => {
            builder.emit(&p10_x86_64_callable_property_native_exit_return_stub_bytes(
                encoded_payload.raw_bits(),
            ))
        }
        P6X86_64BaselineMachineInstruction::Jump { target } => {
            if target.kind != P6X86_64BaselineBytecodeBranchKind::UnconditionalJump
                || target.source_bytecode_index != bytecode_index
            {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction,
                    },
                );
            }
            emit_p6_x86_64_semantic_taken_branch(
                builder,
                bytecode_index,
                instruction,
                CoreOpcode::Jump,
                target,
                branch_mode,
            )
        }
        P6X86_64BaselineMachineInstruction::BranchIfNotNullish {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag,
            null_tag,
            target,
        } => {
            if target.kind != P6X86_64BaselineBytecodeBranchKind::JumpIfNotNullishTaken
                || target.source_bytecode_index != bytecode_index
            {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction,
                    },
                );
            }
            let undefined_tag = p6_x86_64_semantic_u8_tag(bytecode_index, undefined_tag)?;
            let null_tag = p6_x86_64_semantic_u8_tag(bytecode_index, null_tag)?;
            builder.emit(&[0x41, 0x80, 0xfa, undefined_tag])?;
            let undefined_fallthrough = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, null_tag])?;
            let null_fallthrough = builder.emit_je_rel32_internal()?;
            emit_p6_x86_64_semantic_taken_branch(
                builder,
                bytecode_index,
                instruction,
                CoreOpcode::JumpIfNotNullish,
                target,
                branch_mode,
            )?;
            builder.patch_internal_branch_to_current(undefined_fallthrough)?;
            builder.patch_internal_branch_to_current(null_fallthrough)
        }
        P6X86_64BaselineMachineInstruction::BranchIfNotNullish { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag,
            null_tag,
            false_tag,
            true_tag,
            int32_tag,
            unsupported_exit,
            target,
        } => {
            if target.kind != P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
                || target.source_bytecode_index != bytecode_index
            {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction,
                    },
                );
            }
            let undefined_tag = p6_x86_64_semantic_u8_tag(bytecode_index, undefined_tag)?;
            let null_tag = p6_x86_64_semantic_u8_tag(bytecode_index, null_tag)?;
            let false_tag = p6_x86_64_semantic_u8_tag(bytecode_index, false_tag)?;
            let true_tag = p6_x86_64_semantic_u8_tag(bytecode_index, true_tag)?;
            let int32_tag = p6_x86_64_semantic_u8_tag(bytecode_index, int32_tag)?;

            builder.emit(&[0x41, 0x80, 0xfa, undefined_tag])?;
            let undefined_taken = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, null_tag])?;
            let null_taken = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, false_tag])?;
            let false_taken = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, true_tag])?;
            let true_fallthrough = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, int32_tag])?;
            builder.emit_jcc_rel32_side_exit(bytecode_index, 0x85, unsupported_exit)?;
            builder.emit(&[0x49, 0xc1, 0xea, 0x08])?;
            builder.emit(&[0x45, 0x85, 0xd2])?;
            let non_zero_fallthrough = builder.emit_jne_rel32_internal()?;

            let taken_offset = builder.offset()?;
            emit_p6_x86_64_semantic_taken_branch(
                builder,
                bytecode_index,
                instruction,
                CoreOpcode::JumpIfFalse,
                target,
                branch_mode,
            )?;
            let fallthrough_offset = builder.offset()?;

            builder.patch_internal_branch_to_target(undefined_taken, taken_offset)?;
            builder.patch_internal_branch_to_target(null_taken, taken_offset)?;
            builder.patch_internal_branch_to_target(false_taken, taken_offset)?;
            builder.patch_internal_branch_to_target(true_fallthrough, fallthrough_offset)?;
            builder.patch_internal_branch_to_target(non_zero_fallthrough, fallthrough_offset)
        }
        P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::SetReturnCarrier { carrier, source } => {
            if carrier == contract.abi.js_value_return
                && source == P6X86_64BaselineSymbolicRegister::ReturnGpr
            {
                Ok(())
            } else {
                Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction,
                    },
                )
            }
        }
    }
}

fn validate_p6_x86_64_semantic_side_exit_label(
    bytecode_index: crate::bytecode::BytecodeIndex,
    label: P6X86_64BaselineSideExitLabel,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    if label.retained_bytecode_index != bytecode_index
        || label.destination != P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged
        || label.may_throw
        || label.runtime_call
        || label.heap_allocation
        || label.touches_gc_roots
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnexpectedSideExitEffects {
                bytecode_index,
                reason: label.reason,
            },
        );
    }
    Ok(())
}

fn p6_x86_64_semantic_frame_local_disp32(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operand: P6X86_64BaselineMachineOperand,
) -> Result<i32, P6X86_64BaselineSemanticByteEmissionError> {
    let memory =
        match operand {
            P6X86_64BaselineMachineOperand::Memory(memory) => memory,
            P6X86_64BaselineMachineOperand::ReturnCarrier(carrier) => {
                return Err(P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location: P6X86_64BaselineOperandLocation::ReturnCarrier {
                    role: carrier.role,
                    value: carrier.value,
                },
                reason: P6X86_64BaselineSemanticOperandRejectionReason::ReturnCarrierMetadataOnly,
            });
            }
            _ => {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                        bytecode_index,
                        instruction: P6X86_64BaselineMachineInstruction::LoadQ {
                            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                            source: operand,
                        },
                    },
                );
            }
        };
    let disp32 = p6_x86_64_disp32_for_frame_local(bytecode_index, memory.location)?;
    if memory.base != P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMemoryBase {
                bytecode_index,
                base: memory.base,
            },
        );
    }
    Ok(disp32)
}

fn p6_x86_64_disp32_for_frame_local(
    bytecode_index: crate::bytecode::BytecodeIndex,
    location: P6X86_64BaselineOperandLocation,
) -> Result<i32, P6X86_64BaselineSemanticByteEmissionError> {
    match location {
        P6X86_64BaselineOperandLocation::FrameLocal { byte_offset, .. } => {
            if byte_offset <= i32::MAX as u64 {
                Ok(byte_offset as i32)
            } else {
                Err(
                    P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                        bytecode_index,
                        location,
                        byte_offset,
                    },
                )
            }
        }
        P6X86_64BaselineOperandLocation::FrameArgument { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location,
                reason: P6X86_64BaselineSemanticOperandRejectionReason::FrameArgumentUnsupported,
            },
        ),
        P6X86_64BaselineOperandLocation::Constant { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location,
                reason: P6X86_64BaselineSemanticOperandRejectionReason::ConstantMemoryUnsupported,
            },
        ),
        P6X86_64BaselineOperandLocation::ReturnCarrier { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location,
                reason: P6X86_64BaselineSemanticOperandRejectionReason::ReturnCarrierMetadataOnly,
            },
        ),
        P6X86_64BaselineOperandLocation::Immediate(_) => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location,
                reason: P6X86_64BaselineSemanticOperandRejectionReason::ExpectedFrameLocalMemory,
            },
        ),
    }
}

fn emit_p6_x86_64_semantic_taken_branch(
    builder: &mut P6X86_64SemanticByteBuilder,
    bytecode_index: crate::bytecode::BytecodeIndex,
    instruction: P6X86_64BaselineMachineInstruction,
    opcode: CoreOpcode,
    target: P6X86_64BaselineControlFlowBranchContract,
    branch_mode: P6X86_64SemanticBranchEmissionMode,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    if target.source_bytecode_index != bytecode_index
        || target.target_bytecode_index == target.source_bytecode_index
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        );
    }

    if branch_mode == P6X86_64SemanticBranchEmissionMode::CallableLoopBackedgeSafepoints
        && target.target_bytecode_index < target.source_bytecode_index
    {
        builder.emit_jmp_rel32_loop_backedge_safepoint(opcode, target)
    } else {
        builder.emit_jmp_rel32_bytecode_branch(target)
    }
}

fn p6_x86_64_semantic_u8_tag(
    bytecode_index: crate::bytecode::BytecodeIndex,
    tag: u64,
) -> Result<u8, P6X86_64BaselineSemanticByteEmissionError> {
    if tag <= u64::from(u8::MAX) {
        Ok(tag as u8)
    } else {
        Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedImmediateTag {
                bytecode_index,
                tag,
            },
        )
    }
}

fn validate_p6_x86_64_semantic_value_layout(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    if value_layout.tag_mask != 0xff {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "tag_mask",
                actual: value_layout.tag_mask,
            },
        );
    }
    if value_layout.payload_shift != 8 {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "payload_shift",
                actual: u64::from(value_layout.payload_shift),
            },
        );
    }
    if p6_x86_64_value_layout_uses_side_exit_return_payload_tag(value_layout) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "side_exit_return_payload_low_tag",
                actual: u64::from(P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG),
            },
        );
    }
    if p9_x86_64_value_layout_uses_js_call_native_exit_payload_tag(value_layout) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "js_call_native_exit_payload_low_tag",
                actual: u64::from(P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG),
            },
        );
    }
    if p10_x86_64_value_layout_uses_property_native_exit_payload_tag(value_layout) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "property_native_exit_payload_low_tag",
                actual: u64::from(P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG),
            },
        );
    }
    if p14_x86_64_value_layout_uses_loop_backedge_return_payload_tag(value_layout) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "loop_backedge_return_payload_low_tag",
                actual: u64::from(P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG),
            },
        );
    }
    Ok(())
}

fn p6_x86_64_value_layout_uses_side_exit_return_payload_tag(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> bool {
    let payload_tag = u64::from(P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG);
    [
        value_layout.immediate_undefined_tag,
        value_layout.immediate_null_tag,
        value_layout.immediate_false_tag,
        value_layout.immediate_true_tag,
        value_layout.immediate_int32_tag,
        value_layout.immediate_double_tag,
        value_layout.cell_tag,
        value_layout.double_tag,
    ]
    .into_iter()
    .any(|tag| tag & value_layout.tag_mask == payload_tag)
}

fn p9_x86_64_value_layout_uses_js_call_native_exit_payload_tag(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> bool {
    let payload_tag = u64::from(P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG);
    [
        value_layout.immediate_undefined_tag,
        value_layout.immediate_null_tag,
        value_layout.immediate_false_tag,
        value_layout.immediate_true_tag,
        value_layout.immediate_int32_tag,
        value_layout.immediate_double_tag,
        value_layout.cell_tag,
        value_layout.double_tag,
    ]
    .into_iter()
    .any(|tag| tag & value_layout.tag_mask == payload_tag)
}

fn p10_x86_64_value_layout_uses_property_native_exit_payload_tag(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> bool {
    let payload_tag = u64::from(P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG);
    [
        value_layout.immediate_undefined_tag,
        value_layout.immediate_null_tag,
        value_layout.immediate_false_tag,
        value_layout.immediate_true_tag,
        value_layout.immediate_int32_tag,
        value_layout.immediate_double_tag,
        value_layout.cell_tag,
        value_layout.double_tag,
    ]
    .into_iter()
    .any(|tag| tag & value_layout.tag_mask == payload_tag)
}

fn p14_x86_64_value_layout_uses_loop_backedge_return_payload_tag(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> bool {
    let payload_tag = u64::from(P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG);
    [
        value_layout.immediate_undefined_tag,
        value_layout.immediate_null_tag,
        value_layout.immediate_false_tag,
        value_layout.immediate_true_tag,
        value_layout.immediate_int32_tag,
        value_layout.immediate_double_tag,
        value_layout.cell_tag,
        value_layout.double_tag,
    ]
    .into_iter()
    .any(|tag| tag & value_layout.tag_mask == payload_tag)
}

fn p6_x86_64_checked_byte_len(
    byte_len: usize,
) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
    if byte_len <= u32::MAX as usize {
        Ok(byte_len as u32)
    } else {
        Err(P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 { actual: byte_len })
    }
}

fn p6_x86_64_checked_side_exit_index(
    side_exit_index: usize,
) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
    if side_exit_index <= u32::MAX as usize {
        Ok(side_exit_index as u32)
    } else {
        Err(
            P6X86_64BaselineSemanticByteEmissionError::SideExitIndexExceedsPayloadCapacity {
                side_exit_index,
            },
        )
    }
}

fn p9_js_call_argument_registers_from_lowered(
    bytecode_index: crate::bytecode::BytecodeIndex,
    argument_register_count: u16,
    argument_registers: [Option<VirtualRegister>;
        P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS],
) -> Result<Vec<VirtualRegister>, P6X86_64BaselineSemanticByteEmissionError> {
    let count = usize::from(argument_register_count);
    if count > P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::MalformedJsCallNativeExit { bytecode_index },
        );
    }
    let mut registers = Vec::with_capacity(count);
    for register in argument_registers.iter().take(count) {
        registers.push(register.ok_or(
            P6X86_64BaselineSemanticByteEmissionError::MalformedJsCallNativeExit { bytecode_index },
        )?);
    }
    Ok(registers)
}

fn p6_x86_64_callable_payload_return_stub_bytes(return_value_bits: u64) -> Vec<u8> {
    let mut bytes = if return_value_bits == 0 {
        vec![0x31, 0xc0]
    } else if return_value_bits <= u64::from(u32::MAX) {
        let mut bytes = vec![0xb8];
        bytes.extend_from_slice(&(return_value_bits as u32).to_le_bytes());
        bytes
    } else {
        let mut bytes = vec![0x48, 0xb8];
        bytes.extend_from_slice(&return_value_bits.to_le_bytes());
        bytes
    };
    bytes.extend_from_slice(P6_X86_64_CALLABLE_EPILOGUE_BYTES);
    bytes
}

fn p6_x86_64_callable_side_exit_return_stub_bytes(return_value_bits: u64) -> Vec<u8> {
    p6_x86_64_callable_payload_return_stub_bytes(return_value_bits)
}

fn p9_x86_64_callable_js_call_native_exit_return_stub_bytes(return_value_bits: u64) -> Vec<u8> {
    p6_x86_64_callable_payload_return_stub_bytes(return_value_bits)
}

fn p10_x86_64_callable_property_native_exit_return_stub_bytes(return_value_bits: u64) -> Vec<u8> {
    p6_x86_64_callable_payload_return_stub_bytes(return_value_bits)
}

fn p14_x86_64_callable_loop_backedge_return_stub_bytes(return_value_bits: u64) -> Vec<u8> {
    p6_x86_64_callable_payload_return_stub_bytes(return_value_bits)
}

fn p6_x86_64_rel32_displacement(
    branch_offset: u32,
    branch_end_offset: u32,
    target_offset: u32,
) -> Result<i32, P6X86_64BaselineSemanticByteEmissionError> {
    let displacement = i64::from(target_offset) - i64::from(branch_end_offset);
    if displacement < i64::from(i32::MIN) || displacement > i64::from(i32::MAX) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::BranchDisplacementOutOfRange {
                branch_offset,
                branch_end_offset,
                target_offset,
            },
        );
    }
    Ok(displacement as i32)
}

fn validate_p6_x86_64_semantic_byte_images(
    source_buffer: &AssemblerBufferDescriptor,
    source_image: &AssemblerByteImage,
    linked_image: &LinkedAssemblerByteImage,
    expected_byte_len: u32,
    entry_offset: u32,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    if source_buffer.data_kind != AssemblerDataKind::Code {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceBufferInvariant { field: "data_kind" },
        );
    }
    if source_buffer.lifecycle != AssemblerBufferLifecycle::FrozenForLink {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceBufferInvariant { field: "lifecycle" },
        );
    }
    if source_buffer.architecture != Some(AssemblerArchitecture::X86_64) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceBufferInvariant {
                field: "architecture",
            },
        );
    }
    if source_buffer.byte_len != expected_byte_len {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceBufferInvariant { field: "byte_len" },
        );
    }
    if !source_buffer.relocations.is_empty() {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceBufferInvariant {
                field: "relocations",
            },
        );
    }

    source_image
        .validate_against_source(source_buffer)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::SourceImageInvalid { error })?;
    let source_descriptor = source_image.descriptor();
    if source_descriptor.data_kind != AssemblerDataKind::Code {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceImageInvariant { field: "data_kind" },
        );
    }
    if source_descriptor.source_lifecycle != AssemblerBufferLifecycle::FrozenForLink {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceImageInvariant {
                field: "source_lifecycle",
            },
        );
    }
    if source_descriptor.architecture != Some(AssemblerArchitecture::X86_64) {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceImageInvariant {
                field: "architecture",
            },
        );
    }
    if source_image.byte_len() != expected_byte_len
        || source_image.bytes().len() != expected_byte_len as usize
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceImageInvariant { field: "byte_len" },
        );
    }
    if source_descriptor.relocation_count != 0 {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::SourceImageInvariant {
                field: "relocation_count",
            },
        );
    }

    linked_image
        .validate()
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvalid { error })?;
    if linked_image.profile != LinkBufferProfile::Baseline {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvariant { field: "profile" },
        );
    }
    if linked_image.state != LinkBufferState::Linked {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvariant { field: "state" },
        );
    }
    if linked_image.relocation_count != 0 {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvariant {
                field: "relocation_count",
            },
        );
    }
    if linked_image.source_image_id != source_image.id() {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvariant {
                field: "source_image_id",
            },
        );
    }
    if linked_image.source_image_digest != source_image.digest() {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvariant {
                field: "source_image_digest",
            },
        );
    }
    if linked_image.output_size_bytes != expected_byte_len
        || linked_image.bytes() != source_image.bytes()
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::LinkedImageInvariant {
                field: "identity_bytes",
            },
        );
    }
    if entry_offset >= linked_image.output_size_bytes {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::EntryOffsetOutOfRange {
                entry_offset,
                image_size_bytes: linked_image.output_size_bytes,
            },
        );
    }

    Ok(())
}

fn p6_x86_64_semantic_source_buffer_id(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> AssemblerBufferId {
    let raw_owner = u64::from((contract.owner.0).0);
    let raw_id = 0x7036_0000_0000_0001_u64
        ^ raw_owner.rotate_left(17)
        ^ u64::from(contract.bytecode.start.as_bits())
        ^ (u64::from(contract.bytecode.instruction_count) << 32);
    AssemblerBufferId(if raw_id == 0 { 1 } else { raw_id })
}

fn p6_x86_64_semantic_source_image_id(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> AssemblerByteImageId {
    let raw_id = p6_x86_64_semantic_source_buffer_id(contract).0 ^ 0x1f00_0000_0000_006a;
    AssemblerByteImageId(if raw_id == 0 { 1 } else { raw_id })
}

fn p6_x86_64_callable_semantic_source_buffer_id(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> AssemblerBufferId {
    let raw_id = p6_x86_64_semantic_source_buffer_id(contract).0 ^ 0x0000_0000_cab1_e006;
    AssemblerBufferId(if raw_id == 0 { 1 } else { raw_id })
}

fn p6_x86_64_callable_semantic_source_image_id(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> AssemblerByteImageId {
    let raw_id = p6_x86_64_callable_semantic_source_buffer_id(contract).0 ^ 0x1f00_0000_cab1_e06a;
    AssemblerByteImageId(if raw_id == 0 { 1 } else { raw_id })
}

fn validate_p6_x86_64_lowering_subset_and_effect_contract(
    emitter: BaselineMachineCodeEmitterKind,
    subset: BaselineSupportedOpcodeSubset,
    effect_contract: BaselineGeneratedEffectContract,
) -> Result<(), P6X86_64BaselineLoweringError> {
    let expected_subset = emitter.supported_opcode_subset();
    if subset != expected_subset {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcodeSubset {
            emitter,
            expected: expected_subset,
            actual: subset,
        });
    }

    let expected_effect_contract = emitter.supported_effect_contract();
    if effect_contract != expected_effect_contract
        || effect_contract.opcode_subset != subset
        || !effect_contract.permits_no_heap_allocation_no_runtime_call()
        || effect_contract.summary.may_throw
        || effect_contract.summary.writes_heap
        || effect_contract.touches_gc_roots
    {
        return Err(P6X86_64BaselineLoweringError::UnsupportedEffectContract {
            emitter,
            expected: expected_effect_contract,
            actual: effect_contract,
        });
    }

    Ok(())
}

fn p6_x86_64_emitter_kind_for_subset(
    subset: BaselineSupportedOpcodeSubset,
) -> BaselineMachineCodeEmitterKind {
    match subset {
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBranchTruthinessSubset
        }
        _ => BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
    }
}

fn validate_p6_x86_64_backend_contract_subset_and_effect_contract(
    emitter: BaselineMachineCodeEmitterKind,
    subset: BaselineSupportedOpcodeSubset,
    effect_contract: BaselineGeneratedEffectContract,
) -> Result<(), P6X86_64BaselineBackendContractError> {
    let expected_subset = emitter.supported_opcode_subset();
    if subset != expected_subset {
        return Err(
            P6X86_64BaselineBackendContractError::UnsupportedOpcodeSubset {
                emitter,
                expected: expected_subset,
                actual: subset,
            },
        );
    }

    let expected_effect_contract = emitter.supported_effect_contract();
    if effect_contract != expected_effect_contract
        || effect_contract.opcode_subset != subset
        || !effect_contract.permits_no_heap_allocation_no_runtime_call()
        || effect_contract.summary.may_throw
        || effect_contract.summary.writes_heap
        || effect_contract.touches_gc_roots
    {
        return Err(
            P6X86_64BaselineBackendContractError::UnsupportedEffectContract {
                emitter,
                expected: expected_effect_contract,
                actual: effect_contract,
            },
        );
    }

    Ok(())
}

fn p6_x86_64_baseline_value_layout_contract(
) -> Result<P6X86_64BaselineValueLayoutContract, P6X86_64BaselineBackendContractError> {
    let layout = static_value_representation_layout();
    layout
        .validate()
        .map_err(|error| P6X86_64BaselineBackendContractError::ValueLayoutInvalid { error })?;

    let slot_width_bytes = layout.storage_bits / 8;
    if layout.storage_bits != 64
        || !layout.storage_bits.is_multiple_of(8)
        || slot_width_bytes != 8
        || layout.tag_mask != 0xff
        || layout.payload_shift != 8
    {
        return Err(
            P6X86_64BaselineBackendContractError::UnsupportedValueLayout {
                layout_name: layout.name,
                storage_bits: layout.storage_bits,
                slot_width_bytes,
                tag_mask: layout.tag_mask,
                payload_shift: layout.payload_shift,
            },
        );
    }

    let immediate_undefined_tag = p6_named_immediate_tag(
        layout.immediate_tags(),
        "undefined",
        ImmediateKind::Undefined,
    )?;
    let immediate_null_tag =
        p6_named_immediate_tag(layout.immediate_tags(), "null", ImmediateKind::Null)?;
    let immediate_false_tag =
        p6_named_immediate_tag(layout.immediate_tags(), "false", ImmediateKind::Boolean)?;
    let immediate_true_tag =
        p6_named_immediate_tag(layout.immediate_tags(), "true", ImmediateKind::Boolean)?;
    let immediate_int32_tag =
        p6_named_immediate_tag(layout.immediate_tags(), "int32", ImmediateKind::Int32)?;
    let immediate_double_tag =
        p6_named_immediate_tag(layout.immediate_tags(), "double", ImmediateKind::Double)?;

    if immediate_double_tag != layout.double_tag {
        return Err(
            P6X86_64BaselineBackendContractError::DoubleImmediateTagMismatch {
                immediate_double_tag,
                double_tag: layout.double_tag,
            },
        );
    }

    Ok(P6X86_64BaselineValueLayoutContract {
        layout_name: layout.name,
        storage_bits: layout.storage_bits,
        slot_width_bytes,
        tag_mask: layout.tag_mask,
        payload_shift: layout.payload_shift,
        immediate_undefined_tag,
        immediate_null_tag,
        immediate_false_tag,
        immediate_true_tag,
        immediate_int32_tag,
        immediate_double_tag,
        cell_tag: layout.cell_tag,
        double_tag: layout.double_tag,
    })
}

fn p6_named_immediate_tag(
    tags: &[crate::value::ImmediateTagDescriptor],
    canonical_name: &'static str,
    expected: ImmediateKind,
) -> Result<u64, P6X86_64BaselineBackendContractError> {
    let descriptor = tags
        .iter()
        .find(|descriptor| descriptor.canonical_name == canonical_name)
        .ok_or(P6X86_64BaselineBackendContractError::MissingImmediateTag { canonical_name })?;
    if descriptor.kind != expected {
        return Err(
            P6X86_64BaselineBackendContractError::UnexpectedImmediateTagKind {
                canonical_name,
                expected,
                actual: descriptor.kind,
            },
        );
    }
    Ok(descriptor.tag)
}

fn p6_x86_64_baseline_frame_layout_contract(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> P6X86_64BaselineFrameLayoutContract {
    P6X86_64BaselineFrameLayoutContract {
        value_slot_width_bytes: value_layout.slot_width_bytes,
        local_zero_slot_index: 0,
        local_slot_stride_bytes: value_layout.slot_width_bytes,
        this_argument_offset: ThisArgumentOffset(5),
        header_registers_below_this_are_value_addressable: false,
        constants: P6X86_64BaselineConstantsLocationContract::ReadOnlyOutOfFrame,
    }
}

fn p6_x86_64_baseline_symbolic_abi_contract(
) -> Result<P6X86_64BaselineSymbolicAbiContract, P6X86_64BaselineBackendContractError> {
    let descriptor = BASELINE_ABI_DESCRIPTOR;
    descriptor
        .validate()
        .map_err(|error| P6X86_64BaselineBackendContractError::AbiDescriptorInvalid { error })?;

    let pinned_vm =
        p6_required_pinned_register(descriptor.pinned_registers, RegisterRole::PinnedVm)?;
    let pinned_call_frame =
        p6_required_pinned_register(descriptor.pinned_registers, RegisterRole::PinnedCallFrame)?;
    let return_throw = descriptor
        .return_throw
        .ok_or(P6X86_64BaselineBackendContractError::MissingReturnConvention)?;
    if return_throw.normal_return.value != AbiValue::JsValue {
        return Err(
            P6X86_64BaselineBackendContractError::UnexpectedReturnValue {
                expected: AbiValue::JsValue,
                actual: return_throw.normal_return.value,
            },
        );
    }
    let BaselineReturnCarrier::Register(return_role) = return_throw.normal_return.carrier else {
        return Err(P6X86_64BaselineBackendContractError::UnsupportedReturnCarrier);
    };

    Ok(P6X86_64BaselineSymbolicAbiContract {
        descriptor_name: descriptor.name,
        entry_kind: descriptor.entry_kind,
        entry_abi: descriptor.entry_abi,
        frame_abi: descriptor.frame_abi,
        pinned_vm,
        pinned_call_frame,
        js_value_return: P6X86_64BaselineReturnRegisterContract {
            role: return_role,
            value: AbiValue::JsValue,
        },
        stack_alignment_bytes: descriptor.stack_alignment.minimum_bytes,
        stack_alignment_applies_at_entry: descriptor.stack_alignment.applies_at_entry,
        stack_alignment_applies_at_runtime_calls: descriptor
            .stack_alignment
            .applies_at_runtime_calls,
        runtime_clobbered_roles: descriptor.runtime_call_clobbers.clobbered_roles.to_vec(),
        runtime_preserved_roles: descriptor.runtime_call_clobbers.preserved_roles.to_vec(),
        runtime_clobbers_condition_flags: descriptor.runtime_call_clobbers.clobbers_condition_flags,
        runtime_clobbers_stack_argument_area: descriptor
            .runtime_call_clobbers
            .clobbers_stack_argument_area,
        runtime_call_may_allocate: descriptor.runtime_call_clobbers.may_allocate,
        runtime_call_may_throw: descriptor.runtime_call_clobbers.may_throw,
    })
}

fn p6_required_pinned_register(
    registers: &[RegisterBinding],
    role: RegisterRole,
) -> Result<RegisterBinding, P6X86_64BaselineBackendContractError> {
    registers
        .iter()
        .copied()
        .find(|binding| binding.role == role && binding.value == AbiValue::Pointer)
        .ok_or(P6X86_64BaselineBackendContractError::MissingPinnedRegister { role })
}

fn p6_x86_64_baseline_backend_instruction_contract(
    lowered: P6X86_64BaselineLoweredInstruction,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
    return_carrier: P6X86_64BaselineReturnRegisterContract,
) -> Result<P6X86_64BaselineBackendInstructionContract, P6X86_64BaselineBackendContractError> {
    let bytecode_index = lowered.bytecode_index;
    let mut operand_locations = Vec::new();
    let mut arithmetic_exit_policy = None;
    let mut branch_target = None;

    match lowered.operation {
        P6X86_64BaselineLoweredOperation::LoadUndefined { destination } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::Undefined,
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::LoadNull { destination } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::Null,
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::LoadBool {
            destination, value, ..
        } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::Boolean(value),
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::LoadInt32 { destination, value } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::Int32(value),
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::Move {
            destination,
            source,
        } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
            )?);
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Source,
                source,
                false,
                frame_layout,
            )?);
        }
        P6X86_64BaselineLoweredOperation::Return { source } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::ReturnValue,
                source,
                false,
                frame_layout,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Destination,
                location: P6X86_64BaselineOperandLocation::ReturnCarrier {
                    role: return_carrier.role,
                    value: return_carrier.value,
                },
            });
        }
        P6X86_64BaselineLoweredOperation::AddInt32 {
            destination,
            left,
            right,
        } => {
            p6_push_int32_binary_operand_locations(
                &mut operand_locations,
                bytecode_index,
                destination,
                left,
                right,
                frame_layout,
            )?;
            arithmetic_exit_policy = Some(p6_int32_arithmetic_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32ArithmeticOperation::Add,
                P6X86_64BaselineCheckedInt32Arithmetic::CheckedAdd,
            ));
        }
        P6X86_64BaselineLoweredOperation::SubInt32 {
            destination,
            left,
            right,
        } => {
            p6_push_int32_binary_operand_locations(
                &mut operand_locations,
                bytecode_index,
                destination,
                left,
                right,
                frame_layout,
            )?;
            arithmetic_exit_policy = Some(p6_int32_arithmetic_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32ArithmeticOperation::Sub,
                P6X86_64BaselineCheckedInt32Arithmetic::CheckedSub,
            ));
        }
        P6X86_64BaselineLoweredOperation::MulInt32 {
            destination,
            left,
            right,
        } => {
            p6_push_int32_binary_operand_locations(
                &mut operand_locations,
                bytecode_index,
                destination,
                left,
                right,
                frame_layout,
            )?;
            arithmetic_exit_policy = Some(p6_int32_arithmetic_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32ArithmeticOperation::Mul,
                P6X86_64BaselineCheckedInt32Arithmetic::CheckedMul,
            ));
        }
        P6X86_64BaselineLoweredOperation::Jump { target } => {
            branch_target = Some(P6X86_64BaselineControlFlowBranchContract {
                kind: P6X86_64BaselineBytecodeBranchKind::UnconditionalJump,
                source_bytecode_index: bytecode_index,
                target_bytecode_index: target,
            });
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::BranchTarget,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::BytecodeIndex(target),
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::JumpIfNotNullish { source, target } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Source,
                source,
                false,
                frame_layout,
            )?);
            branch_target = Some(P6X86_64BaselineControlFlowBranchContract {
                kind: P6X86_64BaselineBytecodeBranchKind::JumpIfNotNullishTaken,
                source_bytecode_index: bytecode_index,
                target_bytecode_index: target,
            });
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::BranchTarget,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::BytecodeIndex(target),
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::JumpIfFalse { source, target } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Source,
                source,
                false,
                frame_layout,
            )?);
            branch_target = Some(P6X86_64BaselineControlFlowBranchContract {
                kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                source_bytecode_index: bytecode_index,
                target_bytecode_index: target,
            });
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::BranchTarget,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::BytecodeIndex(target),
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit { .. }
        | P6X86_64BaselineLoweredOperation::JsCallNativeExit { .. }
        | P6X86_64BaselineLoweredOperation::PropertyNativeExit { .. } => {}
    }

    Ok(P6X86_64BaselineBackendInstructionContract {
        lowered,
        operand_locations,
        arithmetic_exit_policy,
        branch_target,
    })
}

fn p6_push_int32_binary_operand_locations(
    operand_locations: &mut Vec<P6X86_64BaselineOperandLocationRecord>,
    bytecode_index: crate::bytecode::BytecodeIndex,
    destination: VirtualRegister,
    left: VirtualRegister,
    right: VirtualRegister,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
) -> Result<(), P6X86_64BaselineBackendContractError> {
    operand_locations.push(p6_operand_location_record(
        bytecode_index,
        P6X86_64BaselineOperandRole::Destination,
        destination,
        true,
        frame_layout,
    )?);
    operand_locations.push(p6_operand_location_record(
        bytecode_index,
        P6X86_64BaselineOperandRole::Left,
        left,
        false,
        frame_layout,
    )?);
    operand_locations.push(p6_operand_location_record(
        bytecode_index,
        P6X86_64BaselineOperandRole::Right,
        right,
        false,
        frame_layout,
    )?);
    Ok(())
}

fn p6_operand_location_record(
    bytecode_index: crate::bytecode::BytecodeIndex,
    role: P6X86_64BaselineOperandRole,
    register: VirtualRegister,
    is_destination: bool,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
) -> Result<P6X86_64BaselineOperandLocationRecord, P6X86_64BaselineBackendContractError> {
    let location =
        p6_operand_location(bytecode_index, role, register, is_destination, frame_layout)?;
    Ok(P6X86_64BaselineOperandLocationRecord { role, location })
}

fn p6_operand_location(
    bytecode_index: crate::bytecode::BytecodeIndex,
    role: P6X86_64BaselineOperandRole,
    register: VirtualRegister,
    is_destination: bool,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
) -> Result<P6X86_64BaselineOperandLocation, P6X86_64BaselineBackendContractError> {
    let location = match register.classify(frame_layout.this_argument_offset) {
        RegisterClass::Invalid => {
            return Err(p6_operand_location_error(
                bytecode_index,
                role,
                register,
                P6X86_64BaselineOperandLocationError::InvalidRegister,
            ));
        }
        RegisterClass::Local(index) => P6X86_64BaselineOperandLocation::FrameLocal {
            local_index: index,
            slot_index: index,
            byte_offset: u64::from(index) * u64::from(frame_layout.local_slot_stride_bytes),
        },
        RegisterClass::CallFrameHeader(raw_slot) => {
            return Err(p6_operand_location_error(
                bytecode_index,
                role,
                register,
                P6X86_64BaselineOperandLocationError::HeaderAsValueOperandUnsupported { raw_slot },
            ));
        }
        RegisterClass::ArgumentIncludingThis(index) => {
            P6X86_64BaselineOperandLocation::FrameArgument {
                argument_index_including_this: index,
                raw_virtual_register: register.raw(),
                byte_offset_from_argument_base: u64::from(index)
                    * u64::from(frame_layout.value_slot_width_bytes),
            }
        }
        RegisterClass::Constant(index) => {
            if is_destination {
                return Err(p6_operand_location_error(
                    bytecode_index,
                    role,
                    register,
                    P6X86_64BaselineOperandLocationError::ConstantDestination {
                        constant_index: index,
                    },
                ));
            }
            P6X86_64BaselineOperandLocation::Constant {
                constant_index: index,
                read_only: true,
            }
        }
    };

    Ok(location)
}

fn p6_operand_location_error(
    bytecode_index: crate::bytecode::BytecodeIndex,
    role: P6X86_64BaselineOperandRole,
    register: VirtualRegister,
    error: P6X86_64BaselineOperandLocationError,
) -> P6X86_64BaselineBackendContractError {
    P6X86_64BaselineBackendContractError::OperandLocation {
        bytecode_index,
        role,
        register,
        error,
    }
}

fn p6_int32_arithmetic_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operation: P6X86_64BaselineInt32ArithmeticOperation,
    checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
) -> P6X86_64BaselineInt32ArithmeticExitPolicy {
    P6X86_64BaselineInt32ArithmeticExitPolicy {
        operation,
        operand_guard: P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag,
        checked_arithmetic,
        non_int32_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        ),
        overflow_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::Overflow,
        ),
        negative_zero_policy: match operation {
            P6X86_64BaselineInt32ArithmeticOperation::Mul => {
                P6X86_64BaselineMulNegativeZeroPolicy::NegativeZeroSideExitRequired
            }
            P6X86_64BaselineInt32ArithmeticOperation::Add
            | P6X86_64BaselineInt32ArithmeticOperation::Sub => {
                P6X86_64BaselineMulNegativeZeroPolicy::NotApplicable
            }
        },
    }
}

fn p6_arithmetic_side_exit_contract(
    bytecode_index: crate::bytecode::BytecodeIndex,
    reason: P6X86_64BaselineArithmeticSideExitReason,
) -> P6X86_64BaselineArithmeticSideExitContract {
    P6X86_64BaselineArithmeticSideExitContract {
        reason,
        destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
        retained_bytecode_index: bytecode_index,
        may_throw: false,
        runtime_call: false,
        heap_allocation: false,
        touches_gc_roots: false,
    }
}

fn validate_p6_x86_64_instruction_selection_contract(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    let expected_emitter = p6_x86_64_emitter_kind_for_subset(contract.opcode_subset);
    if contract.emitter_kind != expected_emitter {
        return Err(P6X86_64BaselineInstructionSelectionError::Contract {
            error: P6X86_64BaselineBackendContractError::UnexpectedEmitterKind {
                expected: expected_emitter,
                actual: contract.emitter_kind,
            },
        });
    }
    if contract.architecture != AssemblerArchitecture::X86_64 {
        return Err(P6X86_64BaselineInstructionSelectionError::Contract {
            error: P6X86_64BaselineBackendContractError::UnexpectedArchitecture {
                expected: AssemblerArchitecture::X86_64,
                actual: contract.architecture,
            },
        });
    }
    if contract.byte_emission != P6X86_64BaselineLoweringByteEmission::NotGenerated {
        return Err(P6X86_64BaselineInstructionSelectionError::Contract {
            error: P6X86_64BaselineBackendContractError::UnexpectedByteEmission {
                actual: contract.byte_emission,
            },
        });
    }
    if contract.callable_authority != P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority
    {
        return Err(P6X86_64BaselineInstructionSelectionError::Contract {
            error: P6X86_64BaselineBackendContractError::UnexpectedCallableAuthority {
                actual: contract.callable_authority,
            },
        });
    }
    validate_p6_x86_64_backend_contract_subset_and_effect_contract(
        contract.emitter_kind,
        contract.opcode_subset,
        contract.effect_contract,
    )
    .map_err(|error| P6X86_64BaselineInstructionSelectionError::Contract { error })?;
    if contract.artifact_contract != P6X86_64BaselineBackendArtifactContract::absent() {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArtifactContract {
                actual: contract.artifact_contract,
            },
        );
    }

    let canonical_value_layout = p6_x86_64_baseline_value_layout_contract()
        .map_err(|error| P6X86_64BaselineInstructionSelectionError::Contract { error })?;
    if contract.value_layout != canonical_value_layout {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                field: "value_layout",
            },
        );
    }

    let canonical_frame_layout = p6_x86_64_baseline_frame_layout_contract(canonical_value_layout);
    if contract.frame_layout != canonical_frame_layout {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                field: "frame_layout",
            },
        );
    }

    let canonical_abi = p6_x86_64_baseline_symbolic_abi_contract()
        .map_err(|error| P6X86_64BaselineInstructionSelectionError::Contract { error })?;
    if contract.abi != canonical_abi {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BackendContractMismatch { field: "abi" },
        );
    }

    if contract.instructions.len() != contract.bytecode.instruction_count as usize {
        return Err(
            P6X86_64BaselineInstructionSelectionError::InstructionCountMismatch {
                expected: contract.bytecode.instruction_count as usize,
                actual: contract.instructions.len(),
            },
        );
    }
    validate_p6_x86_64_instruction_selection_bytecode_range(contract)?;
    for instruction in &contract.instructions {
        validate_p6_x86_64_instruction_selection_instruction_contract(
            instruction,
            canonical_frame_layout,
            canonical_abi.js_value_return,
        )?;
    }
    Ok(())
}

fn validate_p6_x86_64_instruction_selection_bytecode_range(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    let Some(first) = contract.instructions.first() else {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                field: "bytecode",
            },
        );
    };
    let Some(last) = contract.instructions.last() else {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                field: "bytecode",
            },
        );
    };
    if first.lowered.bytecode_index != contract.bytecode.start {
        return Err(
            P6X86_64BaselineInstructionSelectionError::InstructionBytecodeRangeMismatch {
                field: "start",
                expected: contract.bytecode.start,
                actual: first.lowered.bytecode_index,
            },
        );
    }
    if last.lowered.bytecode_index != contract.bytecode.end {
        return Err(
            P6X86_64BaselineInstructionSelectionError::InstructionBytecodeRangeMismatch {
                field: "end",
                expected: contract.bytecode.end,
                actual: last.lowered.bytecode_index,
            },
        );
    }

    let mut previous = first.lowered.bytecode_index;
    if !previous.is_valid() || previous.checkpoint() != Checkpoint::NONE {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                field: "bytecode_index",
            },
        );
    }
    for instruction in contract.instructions.iter().skip(1) {
        let actual = instruction.lowered.bytecode_index;
        if !actual.is_valid() || actual.checkpoint() != Checkpoint::NONE {
            return Err(
                P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                    field: "bytecode_index",
                },
            );
        }
        if actual <= previous {
            return Err(
                P6X86_64BaselineInstructionSelectionError::InstructionBytecodeOrderMismatch {
                    previous,
                    actual,
                },
            );
        }
        previous = actual;
    }

    Ok(())
}

fn validate_p6_x86_64_instruction_selection_instruction_contract(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
    return_carrier: P6X86_64BaselineReturnRegisterContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    let expected = p6_x86_64_baseline_backend_instruction_contract(
        instruction.lowered,
        frame_layout,
        return_carrier,
    )
    .map_err(|error| P6X86_64BaselineInstructionSelectionError::Contract { error })?;

    p6_validate_operand_locations_match_lowered(instruction, &expected)?;
    p6_validate_arithmetic_policy_match_lowered(instruction, &expected)?;
    p6_validate_branch_target_contract_match_lowered(instruction, &expected)?;
    Ok(())
}

fn p6_validate_operand_locations_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    let expected_roles = expected
        .operand_locations
        .iter()
        .map(|record| record.role)
        .collect::<Vec<_>>();
    let actual_roles = instruction
        .operand_locations
        .iter()
        .map(|record| record.role)
        .collect::<Vec<_>>();
    if actual_roles != expected_roles {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedOperandRoles {
                bytecode_index: instruction.lowered.bytecode_index,
                expected: expected_roles,
                actual: actual_roles,
            },
        );
    }

    for (actual, expected) in instruction
        .operand_locations
        .iter()
        .zip(expected.operand_locations.iter())
    {
        if actual.location != expected.location {
            return Err(
                P6X86_64BaselineInstructionSelectionError::OperandLocationMismatch {
                    bytecode_index: instruction.lowered.bytecode_index,
                    role: expected.role,
                    expected: expected.location,
                    actual: actual.location,
                },
            );
        }
    }

    Ok(())
}

fn p6_validate_arithmetic_policy_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    match (
        instruction.arithmetic_exit_policy,
        expected.arithmetic_exit_policy,
    ) {
        (None, None) => Ok(()),
        (Some(actual), None) => Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        ),
        (None, Some(_)) => Err(
            P6X86_64BaselineInstructionSelectionError::MissingArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
            },
        ),
        (Some(actual), Some(expected)) => {
            p6_validate_arithmetic_policy(
                instruction.lowered.bytecode_index,
                expected.operation,
                expected.checked_arithmetic,
                actual,
            )?;
            p6_side_exit_label_from_contract(
                instruction.lowered.bytecode_index,
                P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                actual.non_int32_exit,
            )?;
            p6_side_exit_label_from_contract(
                instruction.lowered.bytecode_index,
                P6X86_64BaselineArithmeticSideExitReason::Overflow,
                actual.overflow_exit,
            )?;
            Ok(())
        }
    }
}

fn p6_validate_branch_target_contract_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    match (instruction.branch_target, expected.branch_target) {
        (None, None) => Ok(()),
        (Some(actual), None) => Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBranchTargetContract {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        ),
        (None, Some(_)) => Err(
            P6X86_64BaselineInstructionSelectionError::MissingBranchTargetContract {
                bytecode_index: instruction.lowered.bytecode_index,
            },
        ),
        (Some(actual), Some(expected)) if actual == expected => Ok(()),
        (Some(actual), Some(expected)) => Err(
            P6X86_64BaselineInstructionSelectionError::BranchTargetContractMismatch {
                bytecode_index: instruction.lowered.bytecode_index,
                expected,
                actual,
            },
        ),
    }
}

fn p6_select_x86_64_instruction(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<P6X86_64BaselineSelectedInstruction, P6X86_64BaselineInstructionSelectionError> {
    let machine_instructions = match instruction.lowered.operation {
        P6X86_64BaselineLoweredOperation::LoadUndefined { .. } => p6_select_immediate_load(
            contract,
            instruction,
            P6X86_64BaselineImmediateOperand::Undefined,
        )?,
        P6X86_64BaselineLoweredOperation::LoadNull { .. } => p6_select_immediate_load(
            contract,
            instruction,
            P6X86_64BaselineImmediateOperand::Null,
        )?,
        P6X86_64BaselineLoweredOperation::LoadBool { value, .. } => p6_select_immediate_load(
            contract,
            instruction,
            P6X86_64BaselineImmediateOperand::Boolean(value),
        )?,
        P6X86_64BaselineLoweredOperation::LoadInt32 { value, .. } => p6_select_immediate_load(
            contract,
            instruction,
            P6X86_64BaselineImmediateOperand::Int32(value),
        )?,
        P6X86_64BaselineLoweredOperation::Move { .. } => p6_select_move(instruction)?,
        P6X86_64BaselineLoweredOperation::Return { .. } => p6_select_return(contract, instruction)?,
        P6X86_64BaselineLoweredOperation::AddInt32 { .. } => p6_select_int32_arithmetic(
            contract,
            instruction,
            P6X86_64BaselineInt32ArithmeticOperation::Add,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedAdd,
        )?,
        P6X86_64BaselineLoweredOperation::SubInt32 { .. } => p6_select_int32_arithmetic(
            contract,
            instruction,
            P6X86_64BaselineInt32ArithmeticOperation::Sub,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedSub,
        )?,
        P6X86_64BaselineLoweredOperation::MulInt32 { .. } => p6_select_int32_arithmetic(
            contract,
            instruction,
            P6X86_64BaselineInt32ArithmeticOperation::Mul,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedMul,
        )?,
        P6X86_64BaselineLoweredOperation::Jump { .. } => p6_select_jump(instruction)?,
        P6X86_64BaselineLoweredOperation::JumpIfNotNullish { .. } => {
            p6_select_jump_if_not_nullish(contract, instruction)?
        }
        P6X86_64BaselineLoweredOperation::JumpIfFalse { .. } => {
            p6_select_jump_if_false(contract, instruction)?
        }
        P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit {
            encoded_payload, ..
        } => p6_select_runtime_helper_native_exit(instruction, encoded_payload)?,
        P6X86_64BaselineLoweredOperation::JsCallNativeExit {
            encoded_payload, ..
        } => p9_select_js_call_native_exit(instruction, encoded_payload)?,
        P6X86_64BaselineLoweredOperation::PropertyNativeExit {
            encoded_payload, ..
        } => p10_select_property_native_exit(instruction, encoded_payload)?,
    };

    Ok(P6X86_64BaselineSelectedInstruction {
        bytecode_index: instruction.lowered.bytecode_index,
        lowered: instruction.lowered,
        operand_locations: instruction.operand_locations.clone(),
        effects: P6X86_64BaselineSelectedInstructionEffects::no_runtime_allocation_or_roots(),
        machine_instructions,
    })
}

fn p6_select_runtime_helper_native_exit(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    encoded_payload: P6X86_64BaselineSideExitReturnPayload,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    p6_exact_operand_locations(instruction, &[])?;
    Ok(vec![
        P6X86_64BaselineMachineInstruction::ReturnRuntimeHelperNativeExitPayload {
            encoded_payload,
        },
    ])
}

fn p9_select_js_call_native_exit(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    p6_exact_operand_locations(instruction, &[])?;
    Ok(vec![
        P6X86_64BaselineMachineInstruction::ReturnJsCallNativeExitPayload { encoded_payload },
    ])
}

fn p10_select_property_native_exit(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    encoded_payload: P10X86_64BaselinePropertyNativeExitReturnPayload,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    p6_exact_operand_locations(instruction, &[])?;
    Ok(vec![
        P6X86_64BaselineMachineInstruction::ReturnPropertyNativeExitPayload { encoded_payload },
    ])
}

fn p6_select_jump(
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    let target = instruction.branch_target.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingBranchTargetContract {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_exact_operand_locations(instruction, &[P6X86_64BaselineOperandRole::BranchTarget])?;
    Ok(vec![P6X86_64BaselineMachineInstruction::Jump { target }])
}

fn p6_select_jump_if_not_nullish(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    let target = instruction.branch_target.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingBranchTargetContract {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    let locations = p6_exact_operand_locations(
        instruction,
        &[
            P6X86_64BaselineOperandRole::Source,
            P6X86_64BaselineOperandRole::BranchTarget,
        ],
    )?;
    let source = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[0].role,
        locations[0].location,
        P6X86_64BaselineSelectionOperandAccess::Source,
    )?;

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source,
        },
        P6X86_64BaselineMachineInstruction::BranchIfNotNullish {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag: contract.value_layout.immediate_undefined_tag,
            null_tag: contract.value_layout.immediate_null_tag,
            target,
        },
    ])
}

fn p6_select_jump_if_false(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    let target = instruction.branch_target.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingBranchTargetContract {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    let locations = p6_exact_operand_locations(
        instruction,
        &[
            P6X86_64BaselineOperandRole::Source,
            P6X86_64BaselineOperandRole::BranchTarget,
        ],
    )?;
    let source = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[0].role,
        locations[0].location,
        P6X86_64BaselineSelectionOperandAccess::Source,
    )?;

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source,
        },
        P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag: contract.value_layout.immediate_undefined_tag,
            null_tag: contract.value_layout.immediate_null_tag,
            false_tag: contract.value_layout.immediate_false_tag,
            true_tag: contract.value_layout.immediate_true_tag,
            int32_tag: contract.value_layout.immediate_int32_tag,
            unsupported_exit: p6_unsupported_truthiness_side_exit_label(
                instruction.lowered.bytecode_index,
            ),
            target,
        },
    ])
}

fn p6_select_immediate_load(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected_immediate: P6X86_64BaselineImmediateOperand,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

    let locations = p6_exact_operand_locations(
        instruction,
        &[
            P6X86_64BaselineOperandRole::Destination,
            P6X86_64BaselineOperandRole::Source,
        ],
    )?;
    let destination = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[0].role,
        locations[0].location,
        P6X86_64BaselineSelectionOperandAccess::Destination,
    )?;
    let encoded = p6_encoded_immediate_from_source_location(
        instruction.lowered.bytecode_index,
        expected_immediate,
        locations[1].location,
        contract.value_layout,
    )?;

    Ok(vec![
        P6X86_64BaselineMachineInstruction::MoveQ {
            destination: P6X86_64BaselineMachineOperand::Register(
                P6X86_64BaselineSymbolicRegister::Scratch0,
            ),
            source: P6X86_64BaselineMachineOperand::Immediate64(encoded),
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_move(
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

    let locations = p6_exact_operand_locations(
        instruction,
        &[
            P6X86_64BaselineOperandRole::Destination,
            P6X86_64BaselineOperandRole::Source,
        ],
    )?;
    let destination = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[0].role,
        locations[0].location,
        P6X86_64BaselineSelectionOperandAccess::Destination,
    )?;
    let source = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[1].role,
        locations[1].location,
        P6X86_64BaselineSelectionOperandAccess::Source,
    )?;

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_return(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

    let locations = p6_exact_operand_locations(
        instruction,
        &[
            P6X86_64BaselineOperandRole::ReturnValue,
            P6X86_64BaselineOperandRole::Destination,
        ],
    )?;
    let source = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[0].role,
        locations[0].location,
        P6X86_64BaselineSelectionOperandAccess::Source,
    )?;
    let expected_carrier = contract.abi.js_value_return;
    let actual_carrier = locations[1].location;
    if actual_carrier
        != (P6X86_64BaselineOperandLocation::ReturnCarrier {
            role: expected_carrier.role,
            value: expected_carrier.value,
        })
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::ReturnCarrierMismatch {
                bytecode_index: instruction.lowered.bytecode_index,
                expected: expected_carrier,
                actual: actual_carrier,
            },
        );
    }

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::ReturnGpr,
            source,
        },
        P6X86_64BaselineMachineInstruction::SetReturnCarrier {
            carrier: expected_carrier,
            source: P6X86_64BaselineSymbolicRegister::ReturnGpr,
        },
    ])
}

fn p6_select_int32_arithmetic(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operation: P6X86_64BaselineInt32ArithmeticOperation,
    checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    let locations = p6_exact_operand_locations(
        instruction,
        &[
            P6X86_64BaselineOperandRole::Destination,
            P6X86_64BaselineOperandRole::Left,
            P6X86_64BaselineOperandRole::Right,
        ],
    )?;
    let destination = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[0].role,
        locations[0].location,
        P6X86_64BaselineSelectionOperandAccess::Destination,
    )?;
    let left = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[1].role,
        locations[1].location,
        P6X86_64BaselineSelectionOperandAccess::Source,
    )?;
    let right = p6_machine_operand_for_location(
        instruction.lowered.bytecode_index,
        locations[2].role,
        locations[2].location,
        P6X86_64BaselineSelectionOperandAccess::Source,
    )?;
    let policy = instruction.arithmetic_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingArithmeticPolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_validate_arithmetic_policy(
        instruction.lowered.bytecode_index,
        operation,
        checked_arithmetic,
        policy,
    )?;

    let non_int32_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        policy.non_int32_exit,
    )?;
    let overflow_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::Overflow,
        policy.overflow_exit,
    )?;

    let mut selected = vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source: left,
        },
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch1,
            source: right,
        },
        P6X86_64BaselineMachineInstruction::CheckTagEquals {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            tag_mask: contract.value_layout.tag_mask,
            expected_tag: contract.value_layout.immediate_int32_tag,
            on_not_equal: non_int32_exit,
        },
        P6X86_64BaselineMachineInstruction::CheckTagEquals {
            value: P6X86_64BaselineSymbolicRegister::Scratch1,
            tag_mask: contract.value_layout.tag_mask,
            expected_tag: contract.value_layout.immediate_int32_tag,
            on_not_equal: non_int32_exit,
        },
        P6X86_64BaselineMachineInstruction::ExtractInt32Payload {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
            payload_shift: contract.value_layout.payload_shift,
        },
        P6X86_64BaselineMachineInstruction::ExtractInt32Payload {
            destination: P6X86_64BaselineSymbolicRegister::Scratch1,
            source: P6X86_64BaselineSymbolicRegister::Scratch1,
            payload_shift: contract.value_layout.payload_shift,
        },
        P6X86_64BaselineMachineInstruction::CheckedInt32Arithmetic {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch2,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            on_overflow: overflow_exit,
        },
    ];

    if operation == P6X86_64BaselineInt32ArithmeticOperation::Mul {
        selected.push(P6X86_64BaselineMachineInstruction::CheckMulNegativeZero {
            result: P6X86_64BaselineSymbolicRegister::Scratch2,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            on_negative_zero: p6_negative_zero_side_exit_label(
                instruction.lowered.bytecode_index,
                policy,
            )?,
        });
    }

    selected.push(P6X86_64BaselineMachineInstruction::RetagInt32 {
        destination: P6X86_64BaselineSymbolicRegister::Scratch0,
        payload: P6X86_64BaselineSymbolicRegister::Scratch2,
        payload_shift: contract.value_layout.payload_shift,
        tag: contract.value_layout.immediate_int32_tag,
    });
    selected.push(P6X86_64BaselineMachineInstruction::StoreQ {
        destination,
        source: P6X86_64BaselineSymbolicRegister::Scratch0,
    });

    Ok(selected)
}

fn p6_validate_arithmetic_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_operation: P6X86_64BaselineInt32ArithmeticOperation,
    expected_checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
    actual: P6X86_64BaselineInt32ArithmeticExitPolicy,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    if actual.operation != expected_operation
        || actual.operand_guard != P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
        || actual.checked_arithmetic != expected_checked_arithmetic
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::ArithmeticPolicyMismatch {
                bytecode_index,
                expected_operation,
                expected_checked_arithmetic,
                actual,
            },
        );
    }

    let expected_negative_zero_policy = match expected_operation {
        P6X86_64BaselineInt32ArithmeticOperation::Mul => {
            P6X86_64BaselineMulNegativeZeroPolicy::NegativeZeroSideExitRequired
        }
        P6X86_64BaselineInt32ArithmeticOperation::Add
        | P6X86_64BaselineInt32ArithmeticOperation::Sub => {
            P6X86_64BaselineMulNegativeZeroPolicy::NotApplicable
        }
    };
    if actual.negative_zero_policy != expected_negative_zero_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::NegativeZeroPolicyMismatch {
                bytecode_index,
                expected: expected_negative_zero_policy,
                actual: actual.negative_zero_policy,
            },
        );
    }

    Ok(())
}

fn p6_side_exit_label_from_contract(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_reason: P6X86_64BaselineArithmeticSideExitReason,
    actual: P6X86_64BaselineArithmeticSideExitContract,
) -> Result<P6X86_64BaselineSideExitLabel, P6X86_64BaselineInstructionSelectionError> {
    if actual.reason != expected_reason
        || actual.destination != P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged
        || actual.retained_bytecode_index != bytecode_index
        || actual.may_throw
        || actual.runtime_call
        || actual.heap_allocation
        || actual.touches_gc_roots
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::SideExitContractMismatch {
                bytecode_index,
                expected_reason,
                actual,
            },
        );
    }

    Ok(P6X86_64BaselineSideExitLabel {
        reason: match expected_reason {
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand => {
                P6X86_64BaselineSelectedSideExitReason::NonInt32Operand
            }
            P6X86_64BaselineArithmeticSideExitReason::Overflow => {
                P6X86_64BaselineSelectedSideExitReason::Overflow
            }
        },
        retained_bytecode_index: actual.retained_bytecode_index,
        destination: actual.destination,
        may_throw: actual.may_throw,
        runtime_call: actual.runtime_call,
        heap_allocation: actual.heap_allocation,
        touches_gc_roots: actual.touches_gc_roots,
    })
}

fn p6_negative_zero_side_exit_label(
    bytecode_index: crate::bytecode::BytecodeIndex,
    policy: P6X86_64BaselineInt32ArithmeticExitPolicy,
) -> Result<P6X86_64BaselineSideExitLabel, P6X86_64BaselineInstructionSelectionError> {
    if policy.negative_zero_policy
        != P6X86_64BaselineMulNegativeZeroPolicy::NegativeZeroSideExitRequired
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::NegativeZeroPolicyMismatch {
                bytecode_index,
                expected: P6X86_64BaselineMulNegativeZeroPolicy::NegativeZeroSideExitRequired,
                actual: policy.negative_zero_policy,
            },
        );
    }
    Ok(P6X86_64BaselineSideExitLabel {
        reason: P6X86_64BaselineSelectedSideExitReason::NegativeZero,
        retained_bytecode_index: bytecode_index,
        destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
        may_throw: false,
        runtime_call: false,
        heap_allocation: false,
        touches_gc_roots: false,
    })
}

fn p6_unsupported_truthiness_side_exit_label(
    bytecode_index: crate::bytecode::BytecodeIndex,
) -> P6X86_64BaselineSideExitLabel {
    P6X86_64BaselineSideExitLabel {
        reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
        retained_bytecode_index: bytecode_index,
        destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
        may_throw: false,
        runtime_call: false,
        heap_allocation: false,
        touches_gc_roots: false,
    }
}

fn p6_exact_operand_locations<'a>(
    instruction: &'a P6X86_64BaselineBackendInstructionContract,
    expected: &[P6X86_64BaselineOperandRole],
) -> Result<&'a [P6X86_64BaselineOperandLocationRecord], P6X86_64BaselineInstructionSelectionError>
{
    let actual = instruction
        .operand_locations
        .iter()
        .map(|record| record.role)
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedOperandRoles {
                bytecode_index: instruction.lowered.bytecode_index,
                expected: expected.to_vec(),
                actual,
            },
        );
    }
    Ok(&instruction.operand_locations)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum P6X86_64BaselineSelectionOperandAccess {
    Source,
    Destination,
}

fn p6_machine_operand_for_location(
    bytecode_index: crate::bytecode::BytecodeIndex,
    role: P6X86_64BaselineOperandRole,
    location: P6X86_64BaselineOperandLocation,
    access: P6X86_64BaselineSelectionOperandAccess,
) -> Result<P6X86_64BaselineMachineOperand, P6X86_64BaselineInstructionSelectionError> {
    match (access, location) {
        (
            P6X86_64BaselineSelectionOperandAccess::Destination,
            P6X86_64BaselineOperandLocation::FrameLocal { .. }
            | P6X86_64BaselineOperandLocation::FrameArgument { .. },
        ) => Ok(P6X86_64BaselineMachineOperand::Memory(
            P6X86_64BaselineMachineMemoryOperand {
                base: P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                location,
            },
        )),
        (
            P6X86_64BaselineSelectionOperandAccess::Source,
            P6X86_64BaselineOperandLocation::FrameLocal { .. }
            | P6X86_64BaselineOperandLocation::FrameArgument { .. },
        ) => Ok(P6X86_64BaselineMachineOperand::Memory(
            P6X86_64BaselineMachineMemoryOperand {
                base: P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                location,
            },
        )),
        (
            P6X86_64BaselineSelectionOperandAccess::Source,
            P6X86_64BaselineOperandLocation::Constant {
                read_only: true, ..
            },
        ) => Ok(P6X86_64BaselineMachineOperand::Memory(
            P6X86_64BaselineMachineMemoryOperand {
                base: P6X86_64BaselineSymbolicRegister::PinnedVm,
                location,
            },
        )),
        (
            P6X86_64BaselineSelectionOperandAccess::Source,
            P6X86_64BaselineOperandLocation::Constant {
                read_only: false, ..
            },
        ) => Err(
            P6X86_64BaselineInstructionSelectionError::InvalidOperandLocation {
                bytecode_index,
                role,
                location,
                reason:
                    P6X86_64BaselineInstructionSelectionOperandLocationError::ConstantSourceMustBeReadOnly,
            },
        ),
        (P6X86_64BaselineSelectionOperandAccess::Destination, _) => Err(
            P6X86_64BaselineInstructionSelectionError::InvalidOperandLocation {
                bytecode_index,
                role,
                location,
                reason:
                    P6X86_64BaselineInstructionSelectionOperandLocationError::ExpectedWritableValueSlot,
            },
        ),
        (P6X86_64BaselineSelectionOperandAccess::Source, _) => Err(
            P6X86_64BaselineInstructionSelectionError::InvalidOperandLocation {
                bytecode_index,
                role,
                location,
                reason:
                    P6X86_64BaselineInstructionSelectionOperandLocationError::ExpectedReadableValueSource,
            },
        ),
    }
}

fn p6_encoded_immediate_from_source_location(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected: P6X86_64BaselineImmediateOperand,
    actual: P6X86_64BaselineOperandLocation,
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> Result<u64, P6X86_64BaselineInstructionSelectionError> {
    if actual != P6X86_64BaselineOperandLocation::Immediate(expected) {
        return Err(
            P6X86_64BaselineInstructionSelectionError::ImmediateSourceMismatch {
                bytecode_index,
                expected,
                actual,
            },
        );
    }

    Ok(match expected {
        P6X86_64BaselineImmediateOperand::Undefined => value_layout.immediate_undefined_tag,
        P6X86_64BaselineImmediateOperand::Null => value_layout.immediate_null_tag,
        P6X86_64BaselineImmediateOperand::Boolean(false) => value_layout.immediate_false_tag,
        P6X86_64BaselineImmediateOperand::Boolean(true) => value_layout.immediate_true_tag,
        P6X86_64BaselineImmediateOperand::Int32(value) => {
            ((value as u32 as u64) << value_layout.payload_shift) | value_layout.immediate_int32_tag
        }
        P6X86_64BaselineImmediateOperand::BytecodeIndex(_) => 0,
    })
}

fn p6_selection_fingerprint<T: Debug>(value: &T) -> u128 {
    const OFFSET: u128 = 0x6c62_36a9_3b4d_9f15_8422_2325_cb7a_1d7b;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET;
    for byte in format!("{value:?}").bytes() {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn p6_selection_plan_fingerprint(plan: &P6X86_64BaselineInstructionSelectionPlan) -> u128 {
    p6_selection_fingerprint(&(
        plan.owner,
        plan.bytecode,
        plan.opcode_subset,
        plan.effect_contract,
        plan.emitter_kind,
        plan.architecture,
        plan.byte_emission,
        plan.callable_authority,
        plan.artifact_contract,
        plan.readiness,
        &plan.instructions,
    ))
}

fn validate_emitter_subset_and_effect_contract(
    emitter: BaselineMachineCodeEmitterKind,
    subset: BaselineSupportedOpcodeSubset,
    effect_contract: BaselineGeneratedEffectContract,
) -> Result<(), BaselineMachineCodeByteGenerationError> {
    let expected_subset = emitter.supported_opcode_subset();
    if subset != expected_subset {
        return Err(BaselineMachineCodeByteGenerationError::Emission {
            reason: BaselineMachineCodeEmissionValidationError::UnsupportedOpcodeSubset {
                emitter,
                expected: expected_subset,
                actual: subset,
            },
        });
    }

    let expected_effect_contract = emitter.supported_effect_contract();
    if effect_contract != expected_effect_contract
        || effect_contract.opcode_subset != subset
        || !effect_contract.permits_no_heap_allocation_no_runtime_call()
        || effect_contract.summary.may_throw
        || effect_contract.summary.writes_heap
        || effect_contract.touches_gc_roots
    {
        return Err(BaselineMachineCodeByteGenerationError::Emission {
            reason: BaselineMachineCodeEmissionValidationError::UnsupportedEffectContract {
                emitter,
                expected: expected_effect_contract,
                actual: effect_contract,
            },
        });
    }

    Ok(())
}

fn validate_p6_x86_64_lowering_shape(
    emitter: BaselineMachineCodeEmitterKind,
    code_block: &CodeBlock,
    eligibility_proof: &BaselineBytecodeEligibilityProof,
) -> Result<(), P6X86_64BaselineLoweringError> {
    let shape = P6X86_64BaselineLoweringValidationShape::from_code_block_and_proof(
        code_block,
        eligibility_proof,
    );
    if shape.proof_root_map_count == 0
        && shape.proof_safepoint_count == 0
        && shape.proof_complete_safepoint_root_map_count == 0
        && shape.proof_exception_metadata
            == (BaselineExceptionMetadataPresence::Present { handler_count: 0 })
        && shape.code_block_linked_root_map_count == 0
        && shape.code_block_unlinked_root_map_count == 0
        && shape.code_block_linked_handler_count == 0
        && shape.code_block_unlinked_handler_count == 0
    {
        return Ok(());
    }

    Err(P6X86_64BaselineLoweringError::UnsupportedValidationShape {
        emitter,
        requirement:
            P6X86_64BaselineLoweringRequirement::MatchingCodeBlockSnapshotNoRootsNoExceptionHandlers,
        actual: shape,
    })
}

fn validate_p6_x86_64_lowering_shape_with_runtime_helper_native_exits(
    emitter: BaselineMachineCodeEmitterKind,
    code_block: &CodeBlock,
    eligibility_proof: &BaselineBytecodeEligibilityProof,
    runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    _js_call_native_exit_plan: Option<&BaselineGeneratedJsCallNativeExitPlanMetadata>,
    _property_native_exit_plan: Option<&BaselineGeneratedPropertyHandoffPlanMetadata>,
) -> Result<(), P6X86_64BaselineLoweringError> {
    let shape = P6X86_64BaselineLoweringValidationShape::from_code_block_and_proof(
        code_block,
        eligibility_proof,
    );
    let runtime_helper_proof_count =
        runtime_helper_plan.map_or(0, BaselineGeneratedRuntimeHelperPlanMetadata::proof_count);
    if shape.proof_safepoint_count == runtime_helper_proof_count
        && shape.proof_complete_safepoint_root_map_count == runtime_helper_proof_count
        && shape.proof_root_map_count == runtime_helper_proof_count
        && shape.proof_exception_metadata
            == (BaselineExceptionMetadataPresence::Present { handler_count: 0 })
        && shape.code_block_linked_root_map_count == runtime_helper_proof_count
        && shape.code_block_unlinked_root_map_count == 0
        && shape.code_block_unlinked_handler_count == 0
        && shape.code_block_linked_handler_count == 0
    {
        return Ok(());
    }

    Err(P6X86_64BaselineLoweringError::UnsupportedValidationShape {
        emitter,
        requirement:
            P6X86_64BaselineLoweringRequirement::MatchingCodeBlockSnapshotWithRuntimeHelperNativeExitRootMapsNoExceptionHandlers,
        actual: shape,
    })
}

fn validate_p6_x86_64_non_callable_return_stub_proof_shape(
    eligibility_proof: &BaselineBytecodeEligibilityProof,
) -> Result<(), BaselineMachineCodeByteGenerationError> {
    let shape = BaselineMachineCodeByteGenerationProofShape::from_proof(eligibility_proof);
    if shape.bytecode.instruction_count == 1
        && shape.bytecode.start.is_valid()
        && shape.bytecode.end.is_valid()
        && shape.bytecode.start == shape.bytecode.end
        && shape.bytecode.start.checkpoint() == Checkpoint::NONE
        && shape.bytecode.end.checkpoint() == Checkpoint::NONE
        && shape.root_map_count == 0
        && shape.safepoint_count == 0
        && shape.complete_safepoint_root_map_count == 0
        && shape.exception_metadata
            == (BaselineExceptionMetadataPresence::Present { handler_count: 0 })
    {
        return Ok(());
    }

    Err(BaselineMachineCodeByteGenerationError::UnsupportedProofShape {
        emitter: BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
        requirement: BaselineMachineCodeByteGenerationProofRequirement::SingleInstructionSameIndexNoCheckpointNoRootsNoExceptionHandlers,
        actual: shape,
    })
}

fn collect_p6_x86_64_lowering_operations(
    code_block: &CodeBlock,
    opcode_subset: BaselineSupportedOpcodeSubset,
) -> Result<
    (
        BaselineBytecodeRange,
        Vec<P6X86_64BaselineLoweredInstruction>,
    ),
    P6X86_64BaselineLoweringError,
> {
    let mut operations = Vec::new();
    for instruction in code_block.unlinked().instructions().decoded_instructions() {
        operations.push(lower_p6_x86_64_decoded_instruction_for_subset(
            instruction
                .map_err(|error| P6X86_64BaselineLoweringError::InstructionDecode { error })?,
            opcode_subset,
        )?);
    }

    let bytecode = p6_x86_64_lowering_bytecode_range(&operations)
        .ok_or(P6X86_64BaselineLoweringError::EmptyCodeBlock)?;
    Ok((bytecode, operations))
}

fn collect_p6_x86_64_lowering_operations_with_native_exits(
    code_block: &CodeBlock,
    opcode_subset: BaselineSupportedOpcodeSubset,
    runtime_helper_plan: Option<&BaselineGeneratedRuntimeHelperPlanMetadata>,
    js_call_native_exit_plan: Option<&BaselineGeneratedJsCallNativeExitPlanMetadata>,
    property_native_exit_plan: Option<&BaselineGeneratedPropertyHandoffPlanMetadata>,
) -> Result<
    (
        BaselineBytecodeRange,
        Vec<P6X86_64BaselineLoweredInstruction>,
    ),
    P6X86_64BaselineLoweringError,
> {
    let mut operations = Vec::new();
    let mut runtime_helper_site_index = 0u32;
    let mut js_call_site_index = 0u32;
    let mut property_site_index = 0u32;
    for instruction in code_block.unlinked().instructions().decoded_instructions() {
        let instruction = instruction
            .map_err(|error| P6X86_64BaselineLoweringError::InstructionDecode { error })?;
        let lowered = match CoreOpcode::from_opcode(instruction.opcode) {
            Some(opcode) if opcode_subset.supports(opcode) => {
                lower_p6_x86_64_decoded_instruction_for_subset(instruction, opcode_subset)?
            }
            Some(opcode) if p9_x86_64_js_call_native_exit_opcode(opcode) => {
                let site = js_call_native_exit_plan
                    .and_then(|plan| plan.site_for_bytecode_index(instruction.bytecode_index))
                    .ok_or(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                        bytecode_index: instruction.bytecode_index,
                        opcode: instruction.opcode,
                        core_opcode: Some(opcode),
                    })?;
                let lowered = lower_p9_x86_64_js_call_native_exit(
                    instruction,
                    opcode,
                    site,
                    js_call_site_index,
                )?;
                js_call_site_index = js_call_site_index.saturating_add(1);
                lowered
            }
            Some(opcode) if p10_x86_64_property_native_exit_opcode(opcode) => {
                let site = property_native_exit_plan
                    .and_then(|plan| plan.site_for_bytecode_index(instruction.bytecode_index))
                    .ok_or(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                        bytecode_index: instruction.bytecode_index,
                        opcode: instruction.opcode,
                        core_opcode: Some(opcode),
                    })?;
                let lowered = lower_p10_x86_64_property_native_exit(
                    instruction,
                    opcode,
                    site,
                    property_site_index,
                )?;
                property_site_index = property_site_index.saturating_add(1);
                lowered
            }
            Some(opcode) if p6_x86_64_runtime_helper_native_exit_opcode(opcode) => {
                let proof = runtime_helper_plan
                    .and_then(|plan| plan.proof_for_bytecode_index(instruction.bytecode_index))
                    .ok_or(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                        bytecode_index: instruction.bytecode_index,
                        opcode: instruction.opcode,
                        core_opcode: Some(opcode),
                    })?;
                let lowered = lower_p6_x86_64_runtime_helper_native_exit(
                    instruction,
                    opcode,
                    proof,
                    runtime_helper_site_index,
                )?;
                runtime_helper_site_index = runtime_helper_site_index.saturating_add(1);
                lowered
            }
            Some(opcode) => {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            None => {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: None,
                });
            }
        };
        operations.push(lowered);
    }

    let bytecode = p6_x86_64_lowering_bytecode_range(&operations)
        .ok_or(P6X86_64BaselineLoweringError::EmptyCodeBlock)?;
    Ok((bytecode, operations))
}

fn p6_x86_64_lowering_bytecode_range(
    operations: &[P6X86_64BaselineLoweredInstruction],
) -> Option<BaselineBytecodeRange> {
    let start = operations.first()?.bytecode_index;
    let end = operations.last()?.bytecode_index;
    Some(BaselineBytecodeRange {
        start,
        end,
        instruction_count: operations.len().min(u32::MAX as usize) as u32,
    })
}

#[cfg(test)]
fn lower_p6_x86_64_decoded_instruction(
    instruction: DecodedInstruction<'_>,
) -> Result<P6X86_64BaselineLoweredInstruction, P6X86_64BaselineLoweringError> {
    lower_p6_x86_64_decoded_instruction_for_subset(
        instruction,
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
    )
}

fn lower_p6_x86_64_decoded_instruction_for_subset(
    instruction: DecodedInstruction<'_>,
    opcode_subset: BaselineSupportedOpcodeSubset,
) -> Result<P6X86_64BaselineLoweredInstruction, P6X86_64BaselineLoweringError> {
    let Some(opcode) = CoreOpcode::from_opcode(instruction.opcode) else {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: None,
        });
    };

    if !opcode_subset.supports(opcode) {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }

    let operation = match opcode {
        CoreOpcode::LoadUndefined => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            P6X86_64BaselineLoweredOperation::LoadUndefined {
                destination: p6_register_operand(instruction, opcode, 0)?,
            }
        }
        CoreOpcode::LoadNull => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            P6X86_64BaselineLoweredOperation::LoadNull {
                destination: p6_register_operand(instruction, opcode, 0)?,
            }
        }
        CoreOpcode::LoadBool => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let raw_immediate = p6_unsigned_immediate_operand(instruction, opcode, 1)?;
            P6X86_64BaselineLoweredOperation::LoadBool {
                destination: p6_register_operand(instruction, opcode, 0)?,
                raw_immediate,
                value: raw_immediate != 0,
            }
        }
        CoreOpcode::LoadInt32 => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            P6X86_64BaselineLoweredOperation::LoadInt32 {
                destination: p6_register_operand(instruction, opcode, 0)?,
                value: p6_signed_immediate_operand(instruction, opcode, 1)?,
            }
        }
        CoreOpcode::Move => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            P6X86_64BaselineLoweredOperation::Move {
                destination: p6_register_operand(instruction, opcode, 0)?,
                source: p6_register_operand(instruction, opcode, 1)?,
            }
        }
        CoreOpcode::Return => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            P6X86_64BaselineLoweredOperation::Return {
                source: p6_register_operand(instruction, opcode, 0)?,
            }
        }
        CoreOpcode::AddInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::AddInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::SubInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::SubInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::MulInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::MulInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::Jump => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            P6X86_64BaselineLoweredOperation::Jump {
                target: p6_bytecode_index_operand(instruction, opcode, 0)?,
            }
        }
        CoreOpcode::JumpIfNotNullish => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            P6X86_64BaselineLoweredOperation::JumpIfNotNullish {
                source: p6_register_operand(instruction, opcode, 0)?,
                target: p6_bytecode_index_operand(instruction, opcode, 1)?,
            }
        }
        CoreOpcode::JumpIfFalse => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            P6X86_64BaselineLoweredOperation::JumpIfFalse {
                source: p6_register_operand(instruction, opcode, 0)?,
                target: p6_bytecode_index_operand(instruction, opcode, 1)?,
            }
        }
        _ => {
            return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                bytecode_index: instruction.bytecode_index,
                opcode: instruction.opcode,
                core_opcode: Some(opcode),
            });
        }
    };

    Ok(P6X86_64BaselineLoweredInstruction {
        bytecode_index: instruction.bytecode_index,
        width: instruction.width,
        operation,
    })
}

fn lower_p6_x86_64_runtime_helper_native_exit(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    proof: &BaselineGeneratedRuntimeBoundaryProof,
    site_index: u32,
) -> Result<P6X86_64BaselineLoweredInstruction, P6X86_64BaselineLoweringError> {
    validate_p6_x86_64_runtime_helper_native_exit_operands(instruction, opcode)?;
    let Some(root_map) = proof.root_map else {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    };
    if proof.contract.opcode != opcode
        || !proof.contract.effects.calls_runtime_helper
        || !proof.contract.effects.touches_gc_roots
        || !proof.contract.requirements.complete_safepoint_root_map
        || !proof.contract.requirements.no_gc_exit_reentry
        || !proof.no_gc_exit_reentry
        || proof.may_throw != proof.contract.effects.may_throw
    {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    let Some(payload_index) =
        P6_X86_64_BASELINE_RUNTIME_HELPER_NATIVE_EXIT_PAYLOAD_INDEX_BASE.checked_add(site_index)
    else {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    };

    Ok(P6X86_64BaselineLoweredInstruction {
        bytecode_index: instruction.bytecode_index,
        width: instruction.width,
        operation: P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit {
            opcode,
            safepoint: proof.safepoint,
            root_map,
            root_count: proof.root_count,
            requires_no_gc_exit_reentry: proof.no_gc_exit_reentry,
            may_throw: proof.may_throw,
            encoded_payload: P6X86_64BaselineSideExitReturnPayload::encode(payload_index),
        },
    })
}

fn lower_p9_x86_64_js_call_native_exit(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    site: &crate::jit::plan::BaselineGeneratedJsCallNativeExitSite,
    site_index: u32,
) -> Result<P6X86_64BaselineLoweredInstruction, P6X86_64BaselineLoweringError> {
    if site.bytecode_index != instruction.bytecode_index
        || site.opcode != opcode
        || !site.requires_no_gc_exit_reentry
        || !site.may_throw
    {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    validate_p9_x86_64_js_call_native_exit_operands(instruction, opcode, site)?;
    if site.argument_registers.len() > P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS
        || site.argument_registers.len() > u16::MAX as usize
    {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    let mut argument_registers =
        [None; P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS];
    for (index, register) in site.argument_registers.iter().copied().enumerate() {
        argument_registers[index] = Some(register);
    }

    Ok(P6X86_64BaselineLoweredInstruction {
        bytecode_index: instruction.bytecode_index,
        width: instruction.width,
        operation: P6X86_64BaselineLoweredOperation::JsCallNativeExit {
            opcode,
            destination: site.destination,
            callee: site.callee,
            this_register: site.this_register,
            provided_argument_count: site.provided_argument_count,
            argument_register_count: site.argument_registers.len() as u16,
            argument_registers,
            resume_bytecode_index: site.resume_bytecode_index,
            requires_no_gc_exit_reentry: site.requires_no_gc_exit_reentry,
            may_throw: site.may_throw,
            encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload::encode(site_index),
        },
    })
}

fn lower_p10_x86_64_property_native_exit(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    site: &BaselineGeneratedPropertyHandoffSite,
    site_index: u32,
) -> Result<P6X86_64BaselineLoweredInstruction, P6X86_64BaselineLoweringError> {
    if site.bytecode_index != instruction.bytecode_index
        || site.opcode != opcode
        || !p10_x86_64_property_native_exit_opcode(site.opcode)
        || !site.requires_no_gc_exit_reentry
        || !site.may_throw
    {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    validate_baseline_generated_property_handoff_site_metadata(site).map_err(|_| {
        P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        }
    })?;
    let operands = validate_p10_x86_64_property_native_exit_operands(instruction, opcode, site)?;

    Ok(P6X86_64BaselineLoweredInstruction {
        bytecode_index: instruction.bytecode_index,
        width: instruction.width,
        operation: P6X86_64BaselineLoweredOperation::PropertyNativeExit {
            site: *site,
            operands,
            encoded_payload: P10X86_64BaselinePropertyNativeExitReturnPayload::encode(site_index),
        },
    })
}

const fn p6_x86_64_runtime_helper_native_exit_opcode(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::NewObject
            | CoreOpcode::NewArray
            | CoreOpcode::LoadString
            | CoreOpcode::LoadBigInt
            | CoreOpcode::TypeOf
    )
}

const fn p9_x86_64_js_call_native_exit_opcode(opcode: CoreOpcode) -> bool {
    matches!(opcode, CoreOpcode::Call | CoreOpcode::CallWithThis)
}

const fn p10_x86_64_property_native_exit_opcode(opcode: CoreOpcode) -> bool {
    matches!(opcode, CoreOpcode::GetByName | CoreOpcode::PutByName)
}

fn validate_p10_x86_64_property_native_exit_operands(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    site: &BaselineGeneratedPropertyHandoffSite,
) -> Result<P10X86_64BaselinePropertyNativeExitOperands, P6X86_64BaselineLoweringError> {
    if !p10_x86_64_property_native_exit_opcode(opcode) {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    validate_p6_operand_count(instruction, opcode, 3)?;
    let (identifier_operand_index, operands) = match opcode {
        CoreOpcode::GetByName => {
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let base = p6_register_operand(instruction, opcode, 1)?;
            (
                2,
                P10X86_64BaselinePropertyNativeExitOperands::GetByName { destination, base },
            )
        }
        CoreOpcode::PutByName => {
            let base = p6_register_operand(instruction, opcode, 0)?;
            let value = p6_register_operand(instruction, opcode, 2)?;
            (
                1,
                P10X86_64BaselinePropertyNativeExitOperands::PutByName { base, value },
            )
        }
        _ => unreachable!(),
    };
    let identifier_index =
        p6_identifier_index_operand(instruction, opcode, identifier_operand_index)?;
    let property_key = PropertyKey::from_identifier(Identifier::from_atom(
        AtomId::from_table_slot(identifier_index),
    ));
    if site.property_key != property_key {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    Ok(operands)
}

fn validate_p9_x86_64_js_call_native_exit_operands(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    site: &crate::jit::plan::BaselineGeneratedJsCallNativeExitSite,
) -> Result<(), P6X86_64BaselineLoweringError> {
    let destination = p6_register_operand(instruction, opcode, 0)?;
    let callee = p6_register_operand(instruction, opcode, 1)?;
    if destination != site.destination || callee != site.callee {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    let (this_register, argument_count_operand, first_argument_operand) = match opcode {
        CoreOpcode::Call => (None, 2usize, 3usize),
        CoreOpcode::CallWithThis => (
            Some(p6_register_operand(instruction, opcode, 2)?),
            3usize,
            4usize,
        ),
        _ => {
            return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                bytecode_index: instruction.bytecode_index,
                opcode: instruction.opcode,
                core_opcode: Some(opcode),
            });
        }
    };
    let provided_argument_count =
        p6_unsigned_immediate_operand(instruction, opcode, argument_count_operand)?;
    let expected_operand_count = first_argument_operand
        .checked_add(usize::try_from(provided_argument_count).unwrap_or(usize::MAX))
        .ok_or(P6X86_64BaselineLoweringError::UnexpectedOperandCount {
            bytecode_index: instruction.bytecode_index,
            opcode,
            expected: usize::MAX,
            actual: instruction.operands.len(),
        })?;
    validate_p6_operand_count(instruction, opcode, expected_operand_count)?;
    if this_register != site.this_register
        || provided_argument_count != site.provided_argument_count
        || site.argument_registers.len()
            != usize::try_from(provided_argument_count).unwrap_or(usize::MAX)
    {
        return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
            bytecode_index: instruction.bytecode_index,
            opcode: instruction.opcode,
            core_opcode: Some(opcode),
        });
    }
    for (argument_index, expected) in site.argument_registers.iter().copied().enumerate() {
        let actual = p6_register_operand(
            instruction,
            opcode,
            first_argument_operand.saturating_add(argument_index),
        )?;
        if actual != expected {
            return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                bytecode_index: instruction.bytecode_index,
                opcode: instruction.opcode,
                core_opcode: Some(opcode),
            });
        }
    }
    Ok(())
}

fn validate_p6_x86_64_runtime_helper_native_exit_operands(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
) -> Result<(), P6X86_64BaselineLoweringError> {
    match opcode {
        CoreOpcode::NewObject | CoreOpcode::NewArray => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            let _destination = p6_register_operand(instruction, opcode, 0)?;
        }
        CoreOpcode::LoadString | CoreOpcode::LoadBigInt => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let _destination = p6_register_operand(instruction, opcode, 0)?;
            let _literal_key = p6_identifier_index_operand(instruction, opcode, 1)?;
        }
        CoreOpcode::TypeOf => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let _destination = p6_register_operand(instruction, opcode, 0)?;
            let _source = p6_register_operand(instruction, opcode, 1)?;
        }
        _ => {
            return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                bytecode_index: instruction.bytecode_index,
                opcode: instruction.opcode,
                core_opcode: Some(opcode),
            });
        }
    }
    Ok(())
}

fn lower_p6_int32_binary_operands(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
) -> Result<(VirtualRegister, VirtualRegister, VirtualRegister), P6X86_64BaselineLoweringError> {
    validate_p6_operand_count(instruction, opcode, 3)?;
    Ok((
        p6_register_operand(instruction, opcode, 0)?,
        p6_register_operand(instruction, opcode, 1)?,
        p6_register_operand(instruction, opcode, 2)?,
    ))
}

fn validate_p6_operand_count(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    expected: usize,
) -> Result<(), P6X86_64BaselineLoweringError> {
    let actual = instruction.operands.len();
    if actual != expected {
        return Err(P6X86_64BaselineLoweringError::UnexpectedOperandCount {
            bytecode_index: instruction.bytecode_index,
            opcode,
            expected,
            actual,
        });
    }
    Ok(())
}

fn p6_register_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    index: usize,
) -> Result<VirtualRegister, P6X86_64BaselineLoweringError> {
    instruction.register_operand(index).map_err(|error| {
        P6X86_64BaselineLoweringError::UnsupportedOperandShape {
            bytecode_index: instruction.bytecode_index,
            opcode,
            error,
        }
    })
}

fn p6_signed_immediate_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    index: usize,
) -> Result<i32, P6X86_64BaselineLoweringError> {
    instruction
        .signed_immediate_operand(index)
        .map_err(
            |error| P6X86_64BaselineLoweringError::UnsupportedOperandShape {
                bytecode_index: instruction.bytecode_index,
                opcode,
                error,
            },
        )
}

fn p6_unsigned_immediate_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    index: usize,
) -> Result<u32, P6X86_64BaselineLoweringError> {
    instruction
        .unsigned_immediate_operand(index)
        .map_err(
            |error| P6X86_64BaselineLoweringError::UnsupportedOperandShape {
                bytecode_index: instruction.bytecode_index,
                opcode,
                error,
            },
        )
}

fn p6_bytecode_index_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    index: usize,
) -> Result<crate::bytecode::BytecodeIndex, P6X86_64BaselineLoweringError> {
    match instruction.operand(index) {
        Ok(Operand::BytecodeIndex(target)) => Ok(target),
        Ok(operand) => Err(P6X86_64BaselineLoweringError::UnsupportedOperandShape {
            bytecode_index: instruction.bytecode_index,
            opcode,
            error: OperandAccessError::UnexpectedOperandKind {
                opcode: instruction.opcode,
                index: index as u32,
                expected: crate::bytecode::OperandKind::BytecodeIndex,
                actual: operand.kind(),
            },
        }),
        Err(error) => Err(P6X86_64BaselineLoweringError::UnsupportedOperandShape {
            bytecode_index: instruction.bytecode_index,
            opcode,
            error,
        }),
    }
}

fn p6_identifier_index_operand(
    instruction: DecodedInstruction<'_>,
    opcode: CoreOpcode,
    index: usize,
) -> Result<u32, P6X86_64BaselineLoweringError> {
    match instruction.operand(index) {
        Ok(Operand::IdentifierIndex(identifier_index)) => Ok(identifier_index),
        Ok(operand) => Err(P6X86_64BaselineLoweringError::UnsupportedOperandShape {
            bytecode_index: instruction.bytecode_index,
            opcode,
            error: OperandAccessError::UnexpectedOperandKind {
                opcode: instruction.opcode,
                index: index as u32,
                expected: crate::bytecode::OperandKind::IdentifierIndex,
                actual: operand.kind(),
            },
        }),
        Err(error) => Err(P6X86_64BaselineLoweringError::UnsupportedOperandShape {
            bytecode_index: instruction.bytecode_index,
            opcode,
            error,
        }),
    }
}

fn validate_p6_x86_64_branch_targets(
    operations: &[P6X86_64BaselineLoweredInstruction],
    owner: CodeBlockId,
    proven_bytecode: BaselineBytecodeRange,
    bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    backedge_safepoints: &[P14X86_64BaselineBackedgeSafepointAuthority],
) -> Result<(), P6X86_64BaselineLoweringError> {
    for operation in operations {
        let (opcode, kind, target) = match operation.operation {
            P6X86_64BaselineLoweredOperation::Jump { target } => (
                CoreOpcode::Jump,
                P6X86_64BaselineBytecodeBranchKind::UnconditionalJump,
                target,
            ),
            P6X86_64BaselineLoweredOperation::JumpIfNotNullish { target, .. } => (
                CoreOpcode::JumpIfNotNullish,
                P6X86_64BaselineBytecodeBranchKind::JumpIfNotNullishTaken,
                target,
            ),
            P6X86_64BaselineLoweredOperation::JumpIfFalse { target, .. } => (
                CoreOpcode::JumpIfFalse,
                P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                target,
            ),
            _ => continue,
        };

        let bytecode_index = operation.bytecode_index;
        if !target.is_valid() {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::InvalidBytecodeIndex,
            });
        }
        if target.checkpoint() != Checkpoint::NONE {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::CheckpointedTarget,
            });
        }
        if !proven_bytecode.contains(target) {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::OutOfRange,
            });
        }
        if target == bytecode_index {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::SelfBranch,
            });
        }

        let Some(target_instruction) = operations
            .iter()
            .find(|candidate| candidate.bytecode_index == target)
        else {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::SparseInstructionStart,
            });
        };
        if matches!(
            target_instruction.operation,
            P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit { .. }
                | P6X86_64BaselineLoweredOperation::JsCallNativeExit { .. }
                | P6X86_64BaselineLoweredOperation::PropertyNativeExit { .. }
        ) {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::RuntimeHelperNativeExit,
            });
        }
        if target < bytecode_index
            && !backedge_safepoints.iter().any(|authority| {
                authority.owner == owner
                    && authority.bytecode_snapshot == bytecode_snapshot
                    && authority.source_bytecode_index == bytecode_index
                    && authority.target_bytecode_index == target
                    && authority.opcode == opcode
                    && authority.kind == kind
            })
        {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason:
                    P6X86_64BaselineBranchTargetRejectionReason::BackwardWithoutSafepointAuthority,
            });
        }
    }

    Ok(())
}

fn p6_x86_64_non_callable_return_stub_bytes() -> &'static [u8] {
    P6_X86_64_NON_CALLABLE_RETURN_STUB_BYTES
}

fn p6_x86_64_source_buffer_id(entry_artifact: &BaselineEntryArtifact) -> AssemblerBufferId {
    AssemblerBufferId(entry_artifact.id.0)
}

fn p6_x86_64_source_image_id(entry_artifact: &BaselineEntryArtifact) -> AssemblerByteImageId {
    AssemblerByteImageId(entry_artifact.id.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::AssemblerArchitecture;
    use crate::bytecode::{
        BytecodeIndex, BytecodeRange, BytecodeRootMap, BytecodeRootMapId,
        BytecodeRootSlotDescriptor, BytecodeRootSlotKind, CodeBlock, CodeKind, CoreOpcode,
        HandlerKind, LinkContext, LinkedSideTables, Operand, OperandKind, OperandWidth,
        PackedInstructionStream, TypedInstruction, UnlinkedCodeBlock, UnlinkedCodeBlockPhase,
        UnlinkedHandlerInfo, UnlinkedSideTables, VirtualRegister,
    };
    use crate::gc::CellId;
    use crate::jit::{
        BaselineBytecodeEligibilityRecord, BaselineBytecodeInstruction,
        BaselineRootMapRequirements, CodeFinalizationAuthority, CodeLiveness, CodeOrigin,
        CodeOriginKind, CodeOwnership, EntryAbi, Entrypoint, EntrypointKind,
        ExecutableAllocationId, ExecutableAllocationLifecycle, ExecutableMemoryProtection,
        ExecutableMutationAuthority, JitCodeArtifact, JitCodeId, JitPlanValidationError, JitType,
        MachineCodeHandle, MachineCodeOwnership, MachineCodeRange, TierCounters, TieringSnapshot,
        TieringTrigger,
    };
    use crate::runtime::{CodeBlockId, NativeCodeId};

    fn owner() -> CodeBlockId {
        CodeBlockId(CellId(1))
    }

    fn native_code() -> NativeCodeId {
        NativeCodeId(11)
    }

    fn code_id() -> JitCodeId {
        JitCodeId(7)
    }

    fn local(index: u32) -> VirtualRegister {
        VirtualRegister::local(index)
    }

    fn argument_including_this(index: u32) -> VirtualRegister {
        VirtualRegister::argument_including_this(index, ThisArgumentOffset(5))
    }

    fn header(raw_slot: u32) -> VirtualRegister {
        VirtualRegister::argument_or_header(raw_slot)
    }

    fn constant(index: u32) -> VirtualRegister {
        VirtualRegister::constant(index)
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

    fn code_block_from_typed_instructions(instructions: Vec<TypedInstruction>) -> CodeBlock {
        code_block_from_typed_instructions_with_side_tables(
            instructions,
            UnlinkedSideTables::default(),
        )
    }

    fn code_block_from_typed_instructions_with_side_tables(
        instructions: Vec<TypedInstruction>,
        side_tables: UnlinkedSideTables,
    ) -> CodeBlock {
        let unlinked = UnlinkedCodeBlock::new(
            CodeKind::Program,
            PackedInstructionStream::from_typed_placeholder(instructions),
        )
        .with_side_tables(side_tables)
        .with_phase(UnlinkedCodeBlockPhase::Finalized);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    fn lowering_snapshot() -> TieringSnapshot {
        TieringSnapshot {
            owner: owner(),
            from_tier: JitType::None,
            to_tier: JitType::Baseline,
            trigger: TieringTrigger::EntryCounter,
            counters: TierCounters::default(),
            osr_entry_bytecode_index: None,
            epoch: 1,
        }
    }

    fn lowering_proof_for_code_block(
        code_block: &CodeBlock,
        subset: BaselineSupportedOpcodeSubset,
    ) -> BaselineBytecodeEligibilityProof {
        BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            code_block,
            owner(),
            lowering_snapshot(),
            subset,
            Vec::new(),
        )
        .unwrap()
    }

    fn p6_lowering_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadUndefined,
                vec![Operand::Register(local(0))],
            ),
            typed_instruction(1, CoreOpcode::LoadNull, vec![Operand::Register(local(1))]),
            typed_instruction(
                2,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(2)), Operand::UnsignedImmediate(1)],
            ),
            typed_instruction(
                3,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                4,
                CoreOpcode::Move,
                vec![Operand::Register(local(4)), Operand::Register(local(3))],
            ),
            typed_instruction(
                5,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(5)), Operand::SignedImmediate(6)],
            ),
            typed_instruction(
                6,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(6)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                7,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(7)),
                    Operand::Register(local(5)),
                    Operand::Register(local(6)),
                ],
            ),
            typed_instruction(
                8,
                CoreOpcode::SubInt32,
                vec![
                    Operand::Register(local(8)),
                    Operand::Register(local(7)),
                    Operand::Register(local(6)),
                ],
            ),
            typed_instruction(
                9,
                CoreOpcode::MulInt32,
                vec![
                    Operand::Register(local(9)),
                    Operand::Register(local(8)),
                    Operand::Register(local(6)),
                ],
            ),
            typed_instruction(10, CoreOpcode::Return, vec![Operand::Register(local(9))]),
        ])
    }

    fn p8a_forward_jump_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(BytecodeIndex::from_offset(4))],
            ),
            typed_instruction(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p8a_jump_if_not_nullish_code_block(source: CoreOpcode) -> CodeBlock {
        let source_instruction = match source {
            CoreOpcode::LoadUndefined => typed_instruction(
                0,
                CoreOpcode::LoadUndefined,
                vec![Operand::Register(local(0))],
            ),
            CoreOpcode::LoadNull => {
                typed_instruction(0, CoreOpcode::LoadNull, vec![Operand::Register(local(0))])
            }
            CoreOpcode::LoadBool => typed_instruction(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            CoreOpcode::LoadInt32 => typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            _ => unreachable!("unsupported P8a nullish test source"),
        };
        code_block_from_typed_instructions(vec![
            source_instruction,
            typed_instruction(
                1,
                CoreOpcode::JumpIfNotNullish,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(4)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum P8bTruthinessSource {
        Undefined,
        Null,
        Bool(bool),
        Int32(i32),
    }

    fn p8b_jump_if_false_code_block(source: P8bTruthinessSource) -> CodeBlock {
        let source_instruction = match source {
            P8bTruthinessSource::Undefined => typed_instruction(
                0,
                CoreOpcode::LoadUndefined,
                vec![Operand::Register(local(0))],
            ),
            P8bTruthinessSource::Null => {
                typed_instruction(0, CoreOpcode::LoadNull, vec![Operand::Register(local(0))])
            }
            P8bTruthinessSource::Bool(value) => typed_instruction(
                0,
                CoreOpcode::LoadBool,
                vec![
                    Operand::Register(local(0)),
                    Operand::UnsignedImmediate(value as u32),
                ],
            ),
            P8bTruthinessSource::Int32(value) => typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(value)],
            ),
        };
        code_block_from_typed_instructions(vec![
            source_instruction,
            typed_instruction(
                1,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(4)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p8b_jump_if_false_local_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(3)),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(11)],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            typed_instruction(
                3,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(4, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p9_call_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::Call,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(argument_including_this(1)),
                    Operand::UnsignedImmediate(1),
                    Operand::Register(argument_including_this(3)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::CallWithThis,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(argument_including_this(1)),
                    Operand::Register(argument_including_this(2)),
                    Operand::UnsignedImmediate(2),
                    Operand::Register(argument_including_this(3)),
                    Operand::Register(argument_including_this(4)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ])
    }

    fn p10_get_by_name_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(argument_including_this(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p10_put_by_name_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(43)],
            ),
            typed_instruction(
                1,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(argument_including_this(1)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(0)),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    fn p10_get_by_name_then_put_by_name_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(argument_including_this(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(43)],
            ),
            typed_instruction(
                2,
                CoreOpcode::PutByName,
                vec![
                    Operand::Register(argument_including_this(1)),
                    Operand::IdentifierIndex(11),
                    Operand::Register(local(0)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p6_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
        )
    }

    fn p8a_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish,
        )
    }

    fn p8b_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse,
        )
    }

    fn p14_backedge_authority(
        proof: &BaselineBytecodeEligibilityProof,
        source: BytecodeIndex,
        target: BytecodeIndex,
        opcode: CoreOpcode,
    ) -> P14X86_64BaselineBackedgeSafepointAuthority {
        P14X86_64BaselineBackedgeSafepointAuthority {
            owner: owner(),
            bytecode_snapshot: proof.bytecode_snapshot_fingerprint(),
            source_bytecode_index: source,
            target_bytecode_index: target,
            opcode,
            kind: match opcode {
                CoreOpcode::Jump => P6X86_64BaselineBytecodeBranchKind::UnconditionalJump,
                CoreOpcode::JumpIfNotNullish => {
                    P6X86_64BaselineBytecodeBranchKind::JumpIfNotNullishTaken
                }
                CoreOpcode::JumpIfFalse => P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                _ => unreachable!("unsupported P14 backedge opcode"),
            },
        }
    }

    fn p9_mixed_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_for_mixed_vm_install(
            code_block,
            owner(),
            lowering_snapshot(),
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse,
            Vec::new(),
            None,
        )
        .unwrap()
    }

    fn root_map_for_lowering_code_block() -> BytecodeRootMap {
        BytecodeRootMap {
            id: BytecodeRootMapId(3),
            owner: None,
            bytecode_range_start: BytecodeIndex::from_offset(0),
            bytecode_range_end: BytecodeIndex::from_offset(1),
            slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                BytecodeIndex::from_offset(0),
                local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
            complete: true,
        }
    }

    fn p6_lowering_code_block_with_root_map() -> CodeBlock {
        code_block_from_typed_instructions_with_side_tables(
            vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
                ),
                typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            ],
            UnlinkedSideTables {
                root_maps: vec![root_map_for_lowering_code_block()],
                ..UnlinkedSideTables::default()
            },
        )
        .with_root_map_owner(owner())
    }

    fn p6_lowering_code_block_with_handler() -> CodeBlock {
        let start = BytecodeIndex::from_offset(0);
        let end = BytecodeIndex::from_offset(1);
        code_block_from_typed_instructions_with_side_tables(
            vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
                ),
                typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            ],
            UnlinkedSideTables {
                handlers: vec![UnlinkedHandlerInfo {
                    range: BytecodeRange { start, end },
                    target: end,
                    kind: HandlerKind::Catch,
                }],
                ..UnlinkedSideTables::default()
            },
        )
    }

    fn eligibility_proof_with_instructions(
        subset: BaselineSupportedOpcodeSubset,
        instructions: Vec<BaselineBytecodeInstruction>,
    ) -> BaselineBytecodeEligibilityProof {
        let start = instructions
            .first()
            .map(|instruction| instruction.bytecode_index)
            .unwrap_or_else(|| BytecodeIndex::from_offset(0));
        let end = instructions
            .last()
            .map(|instruction| instruction.bytecode_index)
            .unwrap_or(start);
        BaselineBytecodeEligibilityRecord {
            owner: Some(owner()),
            snapshot: TieringSnapshot {
                owner: owner(),
                from_tier: JitType::None,
                to_tier: JitType::Baseline,
                trigger: TieringTrigger::EntryCounter,
                counters: TierCounters::default(),
                osr_entry_bytecode_index: None,
                epoch: 1,
            },
            bytecode: BaselineBytecodeRange {
                start,
                end,
                instruction_count: instructions.len() as u32,
            },
            opcode_subset: subset,
            instructions,
            root_map_requirements: BaselineRootMapRequirements::default(),
            exception_metadata: BaselineExceptionMetadataPresence::Present { handler_count: 0 },
        }
        .validate()
        .unwrap()
    }

    fn single_return_proof() -> BaselineBytecodeEligibilityProof {
        single_return_proof_for_subset(
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
        )
    }

    fn single_return_proof_for_subset(
        subset: BaselineSupportedOpcodeSubset,
    ) -> BaselineBytecodeEligibilityProof {
        eligibility_proof_with_instructions(
            subset,
            vec![BaselineBytecodeInstruction {
                bytecode_index: BytecodeIndex::from_offset(0),
                opcode: CoreOpcode::Return,
            }],
        )
    }

    fn multi_instruction_proof() -> BaselineBytecodeEligibilityProof {
        eligibility_proof_with_instructions(
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
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

    fn machine_code() -> MachineCodeHandle {
        let allocation = ExecutableAllocationId(13);
        MachineCodeHandle {
            allocation,
            owner: MachineCodeOwnership::CodeBlock(owner()),
            range: MachineCodeRange {
                allocation,
                start_offset: 0,
                size_bytes: p6_x86_64_non_callable_return_stub_bytes().len() as u32,
            },
            symbol: Some(native_code()),
            protection: ExecutableMemoryProtection::Executable,
            lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
            mutation_authority: ExecutableMutationAuthority::LinkBuffer,
        }
    }

    fn entrypoint() -> Entrypoint {
        Entrypoint {
            kind: EntrypointKind::GeneratedCode,
            abi: EntryAbi::GeneratedCode,
            code: Some(code_id()),
            boundary: None,
        }
    }

    fn native_artifact() -> JitCodeArtifact {
        JitCodeArtifact {
            id: code_id(),
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(owner()),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(native_code()),
            machine_code: Some(machine_code()),
            entrypoint: entrypoint(),
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Live,
            finalization_authority: CodeFinalizationAuthority::CompilerThread,
        }
    }

    fn entry_artifact() -> BaselineEntryArtifact {
        native_artifact()
            .validate_baseline_entry_artifact(owner())
            .unwrap()
    }

    fn lowered_instruction(
        offset: u32,
        operation: P6X86_64BaselineLoweredOperation,
    ) -> P6X86_64BaselineLoweredInstruction {
        P6X86_64BaselineLoweredInstruction {
            bytecode_index: BytecodeIndex::from_offset(offset),
            width: OperandWidth::Narrow,
            operation,
        }
    }

    fn p6_backend_contract() -> P6X86_64BaselineBackendContractRecord {
        let code_block = p6_lowering_code_block();
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        record_p6_x86_64_baseline_backend_contract(&lowering).unwrap()
    }

    fn p6_backend_contract_and_selection() -> (
        P6X86_64BaselineBackendContractRecord,
        P6X86_64BaselineInstructionSelectionPlan,
    ) {
        let contract = p6_backend_contract();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        (contract, selection)
    }

    fn p6_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p6_lowering_proof(code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_semantic_bytes(contract, selection)
    }

    fn p8a_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p8a_lowering_proof(code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_semantic_bytes(contract, selection)
    }

    fn p8a_callable_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p8a_lowering_proof(code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection)
    }

    fn p8b_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p8b_lowering_proof(code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_semantic_bytes(contract, selection)
    }

    fn p8b_callable_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p8b_lowering_proof(code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection)
    }

    fn p9_callable_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p9_mixed_lowering_proof(code_block);
        let js_call_plan =
            crate::jit::plan::derive_baseline_generated_js_call_native_exit_plan_from_code_block(
                code_block,
                owner(),
            )
            .unwrap()
            .metadata
            .expect("P9 call native-exit metadata");
        let lowering = plan_p6_x86_64_baseline_lowering_with_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), code_block, proof),
            None,
            Some(&js_call_plan),
            None,
        )
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection)
    }

    fn p10_callable_semantic_emission_for_code_block(
        code_block: &CodeBlock,
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let proof = p9_mixed_lowering_proof(code_block);
        let property_plan =
            crate::jit::plan::derive_baseline_generated_property_handoff_plan_from_code_block(
                code_block,
                owner(),
            )
            .unwrap()
            .metadata
            .expect("P10 property native-exit metadata");
        let lowering = plan_p6_x86_64_baseline_lowering_with_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), code_block, proof),
            None,
            None,
            Some(&property_plan),
        )
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection)
    }

    fn p6_semantic_emission(
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let (contract, selection) = p6_backend_contract_and_selection();
        emit_p6_x86_64_baseline_semantic_bytes(contract, selection)
    }

    fn p6_callable_semantic_emission(
    ) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError>
    {
        let (contract, selection) = p6_backend_contract_and_selection();
        emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection)
    }

    fn assert_absent_artifacts(contract: &P6X86_64BaselineBackendArtifactContract) {
        assert_eq!(
            contract.assembler_byte_images,
            P6X86_64BaselineBackendArtifactPresence::Absent
        );
        assert_eq!(
            contract.machine_handles,
            P6X86_64BaselineBackendArtifactPresence::Absent
        );
        assert_eq!(
            contract.native_ids,
            P6X86_64BaselineBackendArtifactPresence::Absent
        );
        assert_eq!(
            contract.jit_artifacts,
            P6X86_64BaselineBackendArtifactPresence::Absent
        );
        assert_eq!(
            contract.vm_materialization,
            P6X86_64BaselineBackendArtifactPresence::Absent
        );
        assert_eq!(
            contract.vm_readiness,
            P6X86_64BaselineBackendArtifactPresence::Absent
        );
    }

    fn frame_local_location(index: u32) -> P6X86_64BaselineOperandLocation {
        P6X86_64BaselineOperandLocation::FrameLocal {
            local_index: index,
            slot_index: index,
            byte_offset: u64::from(index) * 8,
        }
    }

    fn frame_argument_location(index: u32) -> P6X86_64BaselineOperandLocation {
        P6X86_64BaselineOperandLocation::FrameArgument {
            argument_index_including_this: index,
            raw_virtual_register: argument_including_this(index).raw(),
            byte_offset_from_argument_base: u64::from(index) * 8,
        }
    }

    fn constant_location(index: u32, read_only: bool) -> P6X86_64BaselineOperandLocation {
        P6X86_64BaselineOperandLocation::Constant {
            constant_index: index,
            read_only,
        }
    }

    fn frame_memory_operand(index: u32) -> P6X86_64BaselineMachineOperand {
        memory_operand(
            P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
            frame_local_location(index),
        )
    }

    fn memory_operand(
        base: P6X86_64BaselineSymbolicRegister,
        location: P6X86_64BaselineOperandLocation,
    ) -> P6X86_64BaselineMachineOperand {
        P6X86_64BaselineMachineOperand::Memory(P6X86_64BaselineMachineMemoryOperand {
            base,
            location,
        })
    }

    fn side_exit_label(
        reason: P6X86_64BaselineSelectedSideExitReason,
        bytecode_offset: u32,
    ) -> P6X86_64BaselineSideExitLabel {
        P6X86_64BaselineSideExitLabel {
            reason,
            retained_bytecode_index: BytecodeIndex::from_offset(bytecode_offset),
            destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
            may_throw: false,
            runtime_call: false,
            heap_allocation: false,
            touches_gc_roots: false,
        }
    }

    fn rel32_branch_target(bytes: &[u8], branch_end_offset: u32) -> i64 {
        let rel32_start = (branch_end_offset - 4) as usize;
        let displacement = i32::from_le_bytes([
            bytes[rel32_start],
            bytes[rel32_start + 1],
            bytes[rel32_start + 2],
            bytes[rel32_start + 3],
        ]);
        i64::from(branch_end_offset) + i64::from(displacement)
    }

    fn expected_immediate_load_selection(
        destination: u32,
        encoded: u64,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        vec![
            P6X86_64BaselineMachineInstruction::MoveQ {
                destination: P6X86_64BaselineMachineOperand::Register(
                    P6X86_64BaselineSymbolicRegister::Scratch0,
                ),
                source: P6X86_64BaselineMachineOperand::Immediate64(encoded),
            },
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: frame_memory_operand(destination),
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
        ]
    }

    fn expected_move_selection(
        destination: P6X86_64BaselineMachineOperand,
        source: P6X86_64BaselineMachineOperand,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        vec![
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                source,
            },
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination,
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
        ]
    }

    fn expected_return_selection(
        source: P6X86_64BaselineMachineOperand,
        carrier: P6X86_64BaselineReturnRegisterContract,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        vec![
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::ReturnGpr,
                source,
            },
            P6X86_64BaselineMachineInstruction::SetReturnCarrier {
                carrier,
                source: P6X86_64BaselineSymbolicRegister::ReturnGpr,
            },
        ]
    }

    fn expected_int32_arithmetic_selection(
        contract: &P6X86_64BaselineBackendContractRecord,
        bytecode_offset: u32,
        operation: P6X86_64BaselineInt32ArithmeticOperation,
        destination: u32,
        left: u32,
        right: u32,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        let non_int32_exit = side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            bytecode_offset,
        );
        let overflow_exit = side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::Overflow,
            bytecode_offset,
        );
        let mut selected = vec![
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                source: frame_memory_operand(left),
            },
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch1,
                source: frame_memory_operand(right),
            },
            P6X86_64BaselineMachineInstruction::CheckTagEquals {
                value: P6X86_64BaselineSymbolicRegister::Scratch0,
                tag_mask: contract.value_layout.tag_mask,
                expected_tag: contract.value_layout.immediate_int32_tag,
                on_not_equal: non_int32_exit,
            },
            P6X86_64BaselineMachineInstruction::CheckTagEquals {
                value: P6X86_64BaselineSymbolicRegister::Scratch1,
                tag_mask: contract.value_layout.tag_mask,
                expected_tag: contract.value_layout.immediate_int32_tag,
                on_not_equal: non_int32_exit,
            },
            P6X86_64BaselineMachineInstruction::ExtractInt32Payload {
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
                payload_shift: contract.value_layout.payload_shift,
            },
            P6X86_64BaselineMachineInstruction::ExtractInt32Payload {
                destination: P6X86_64BaselineSymbolicRegister::Scratch1,
                source: P6X86_64BaselineSymbolicRegister::Scratch1,
                payload_shift: contract.value_layout.payload_shift,
            },
            P6X86_64BaselineMachineInstruction::CheckedInt32Arithmetic {
                operation,
                destination: P6X86_64BaselineSymbolicRegister::Scratch2,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                on_overflow: overflow_exit,
            },
        ];

        if operation == P6X86_64BaselineInt32ArithmeticOperation::Mul {
            selected.push(P6X86_64BaselineMachineInstruction::CheckMulNegativeZero {
                result: P6X86_64BaselineSymbolicRegister::Scratch2,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                on_negative_zero: side_exit_label(
                    P6X86_64BaselineSelectedSideExitReason::NegativeZero,
                    bytecode_offset,
                ),
            });
        }

        selected.push(P6X86_64BaselineMachineInstruction::RetagInt32 {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            payload: P6X86_64BaselineSymbolicRegister::Scratch2,
            payload_shift: contract.value_layout.payload_shift,
            tag: contract.value_layout.immediate_int32_tag,
        });
        selected.push(P6X86_64BaselineMachineInstruction::StoreQ {
            destination: frame_memory_operand(destination),
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        });
        selected
    }

    #[test]
    fn p6_lowering_accepts_operand_aware_baseline_plan() {
        let code_block = p6_lowering_code_block();
        let proof = p6_lowering_proof(&code_block);

        let result = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            result.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset
        );
        assert_eq!(
            result.byte_emission,
            P6X86_64BaselineLoweringByteEmission::NotGenerated
        );
        assert_eq!(
            result.callable_authority,
            P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority
        );
        assert_eq!(result.plan.owner, owner());
        assert_eq!(
            result.plan.bytecode,
            BaselineBytecodeRange {
                start: BytecodeIndex::from_offset(0),
                end: BytecodeIndex::from_offset(10),
                instruction_count: 11,
            }
        );
        assert_eq!(
            result.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
        );
        assert_eq!(
            result.plan.effect_contract,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic
                .generated_effect_contract()
        );
        assert_eq!(
            result.plan.bytecode_snapshot,
            proof.bytecode_snapshot_fingerprint()
        );
        assert_eq!(
            result.plan.operations,
            vec![
                lowered_instruction(
                    0,
                    P6X86_64BaselineLoweredOperation::LoadUndefined {
                        destination: local(0),
                    },
                ),
                lowered_instruction(
                    1,
                    P6X86_64BaselineLoweredOperation::LoadNull {
                        destination: local(1),
                    },
                ),
                lowered_instruction(
                    2,
                    P6X86_64BaselineLoweredOperation::LoadBool {
                        destination: local(2),
                        raw_immediate: 1,
                        value: true,
                    },
                ),
                lowered_instruction(
                    3,
                    P6X86_64BaselineLoweredOperation::LoadInt32 {
                        destination: local(3),
                        value: 7,
                    },
                ),
                lowered_instruction(
                    4,
                    P6X86_64BaselineLoweredOperation::Move {
                        destination: local(4),
                        source: local(3),
                    },
                ),
                lowered_instruction(
                    5,
                    P6X86_64BaselineLoweredOperation::LoadInt32 {
                        destination: local(5),
                        value: 6,
                    },
                ),
                lowered_instruction(
                    6,
                    P6X86_64BaselineLoweredOperation::LoadInt32 {
                        destination: local(6),
                        value: 7,
                    },
                ),
                lowered_instruction(
                    7,
                    P6X86_64BaselineLoweredOperation::AddInt32 {
                        destination: local(7),
                        left: local(5),
                        right: local(6),
                    },
                ),
                lowered_instruction(
                    8,
                    P6X86_64BaselineLoweredOperation::SubInt32 {
                        destination: local(8),
                        left: local(7),
                        right: local(6),
                    },
                ),
                lowered_instruction(
                    9,
                    P6X86_64BaselineLoweredOperation::MulInt32 {
                        destination: local(9),
                        left: local(8),
                        right: local(6),
                    },
                ),
                lowered_instruction(
                    10,
                    P6X86_64BaselineLoweredOperation::Return { source: local(9) },
                ),
            ]
        );
    }

    #[test]
    fn p8a_lowering_accepts_forward_jump_and_nullish_branch_only() {
        let jump_block = p8a_forward_jump_code_block();
        let jump_proof = p8a_lowering_proof(&jump_block);
        let jump = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &jump_block,
            jump_proof,
        ))
        .unwrap();
        assert_eq!(
            jump.emitter_kind,
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBranchSubset
        );
        assert_eq!(
            jump.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish
        );
        assert_eq!(
            jump.plan.operations[1].operation,
            P6X86_64BaselineLoweredOperation::Jump {
                target: BytecodeIndex::from_offset(4),
            }
        );

        let nullish_block = p8a_jump_if_not_nullish_code_block(CoreOpcode::LoadInt32);
        let nullish_proof = p8a_lowering_proof(&nullish_block);
        let nullish = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &nullish_block,
            nullish_proof,
        ))
        .unwrap();
        assert_eq!(
            nullish.plan.operations[1].operation,
            P6X86_64BaselineLoweredOperation::JumpIfNotNullish {
                source: local(0),
                target: BytecodeIndex::from_offset(4),
            }
        );

        let jump_if_false = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_instruction(
                1,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(3)),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let error = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &jump_if_false,
            owner(),
            lowering_snapshot(),
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish,
            Vec::new(),
        )
        .expect_err("P8a must not advertise JumpIfFalse");
        assert!(matches!(
            error,
            JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                opcode: CoreOpcode::JumpIfFalse,
                ..
            }
        ));
    }

    #[test]
    fn p8b_lowering_accepts_jump_if_false_with_exact_subset_and_p8a_rejects_it() {
        let code_block = p8b_jump_if_false_code_block(P8bTruthinessSource::Bool(false));
        let proof = p8b_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBranchTruthinessSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBranchNullishFalse
        );
        assert_eq!(
            lowering.plan.operations[1].operation,
            P6X86_64BaselineLoweredOperation::JumpIfFalse {
                source: local(0),
                target: BytecodeIndex::from_offset(4),
            }
        );

        let error = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner(),
            lowering_snapshot(),
            BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish,
            Vec::new(),
        )
        .expect_err("P8a must not advertise JumpIfFalse");
        assert!(matches!(
            error,
            JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                opcode: CoreOpcode::JumpIfFalse,
                ..
            }
        ));

        for source in [P8bTruthinessSource::Undefined, P8bTruthinessSource::Null] {
            let code_block = p8b_jump_if_false_code_block(source);
            let proof = p8b_lowering_proof(&code_block);
            assert!(
                plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                    owner(),
                    &code_block,
                    proof,
                ))
                .is_ok(),
                "{source:?}"
            );
        }
    }

    #[test]
    fn p8b_lowering_rejects_jump_if_false_invalid_branch_target_before_emission() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_instruction(
                1,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(1)),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p8b_lowering_proof(&code_block);

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index: BytecodeIndex::from_offset(1),
                opcode: CoreOpcode::JumpIfFalse,
                target: BytecodeIndex::from_offset(1),
                reason: P6X86_64BaselineBranchTargetRejectionReason::SelfBranch,
            })
        );
    }

    #[test]
    fn p8a_lowering_rejects_invalid_branch_targets_before_emission() {
        struct BranchTargetCase {
            name: &'static str,
            target: Operand,
            expected: Result<(), P6X86_64BaselineLoweringError>,
        }

        let cases = vec![
            BranchTargetCase {
                name: "malformed operand",
                target: Operand::SignedImmediate(4),
                expected: Err(P6X86_64BaselineLoweringError::UnsupportedOperandShape {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Jump,
                    error: OperandAccessError::UnexpectedOperandKind {
                        opcode: CoreOpcode::Jump.opcode(),
                        index: 0,
                        expected: OperandKind::BytecodeIndex,
                        actual: OperandKind::SignedImmediate,
                    },
                }),
            },
            BranchTargetCase {
                name: "invalid target",
                target: Operand::BytecodeIndex(BytecodeIndex::INVALID),
                expected: Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Jump,
                    target: BytecodeIndex::INVALID,
                    reason: P6X86_64BaselineBranchTargetRejectionReason::InvalidBytecodeIndex,
                }),
            },
            BranchTargetCase {
                name: "checkpoint target",
                target: Operand::BytecodeIndex(
                    BytecodeIndex::from_offset(4).with_checkpoint(Checkpoint(1)),
                ),
                expected: Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Jump,
                    target: BytecodeIndex::from_offset(4).with_checkpoint(Checkpoint(1)),
                    reason: P6X86_64BaselineBranchTargetRejectionReason::CheckpointedTarget,
                }),
            },
            BranchTargetCase {
                name: "out of range target",
                target: Operand::BytecodeIndex(BytecodeIndex::from_offset(99)),
                expected: Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Jump,
                    target: BytecodeIndex::from_offset(99),
                    reason: P6X86_64BaselineBranchTargetRejectionReason::OutOfRange,
                }),
            },
            BranchTargetCase {
                name: "self target",
                target: Operand::BytecodeIndex(BytecodeIndex::from_offset(1)),
                expected: Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Jump,
                    target: BytecodeIndex::from_offset(1),
                    reason: P6X86_64BaselineBranchTargetRejectionReason::SelfBranch,
                }),
            },
            BranchTargetCase {
                name: "backward target without authority",
                target: Operand::BytecodeIndex(BytecodeIndex::from_offset(0)),
                expected: Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                    bytecode_index: BytecodeIndex::from_offset(1),
                    opcode: CoreOpcode::Jump,
                    target: BytecodeIndex::from_offset(0),
                    reason:
                        P6X86_64BaselineBranchTargetRejectionReason::BackwardWithoutSafepointAuthority,
                }),
            },
        ];

        for case in cases {
            let code_block = code_block_from_typed_instructions(vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
                ),
                typed_instruction(1, CoreOpcode::Jump, vec![case.target]),
                typed_instruction(
                    2,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(11)],
                ),
                typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(0))]),
                typed_instruction(4, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            ]);
            let proof = p8a_lowering_proof(&code_block);
            let result = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            ))
            .map(|_| ());
            assert_eq!(result, case.expected, "{}", case.name);
        }

        let sparse = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(BytecodeIndex::from_offset(1))],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p8a_lowering_proof(&sparse);
        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &sparse,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index: BytecodeIndex::from_offset(0),
                opcode: CoreOpcode::Jump,
                target: BytecodeIndex::from_offset(1),
                reason: P6X86_64BaselineBranchTargetRejectionReason::SparseInstructionStart,
            })
        );
    }

    #[test]
    fn p14_lowering_accepts_backward_branch_only_with_exact_safepoint_authority() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(BytecodeIndex::from_offset(0))],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p8a_lowering_proof(&code_block);
        let exact = P14X86_64BaselineBackedgeSafepointAuthority {
            owner: owner(),
            bytecode_snapshot: proof.bytecode_snapshot_fingerprint(),
            source_bytecode_index: BytecodeIndex::from_offset(1),
            target_bytecode_index: BytecodeIndex::from_offset(0),
            opcode: CoreOpcode::Jump,
            kind: P6X86_64BaselineBytecodeBranchKind::UnconditionalJump,
        };

        let without_authority = plan_p6_x86_64_baseline_lowering(
            P6X86_64BaselineLoweringRequest::new(owner(), &code_block, proof),
        )
        .expect_err("P14 backedge requires explicit authority");
        assert_eq!(
            without_authority,
            P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index: BytecodeIndex::from_offset(1),
                opcode: CoreOpcode::Jump,
                target: BytecodeIndex::from_offset(0),
                reason:
                    P6X86_64BaselineBranchTargetRejectionReason::BackwardWithoutSafepointAuthority,
            }
        );

        let wrong_snapshot =
            p8a_lowering_proof(&p8a_forward_jump_code_block()).bytecode_snapshot_fingerprint();
        for (name, authority) in [
            (
                "owner",
                P14X86_64BaselineBackedgeSafepointAuthority {
                    owner: CodeBlockId(CellId(2)),
                    ..exact
                },
            ),
            (
                "snapshot",
                P14X86_64BaselineBackedgeSafepointAuthority {
                    bytecode_snapshot: wrong_snapshot,
                    ..exact
                },
            ),
            (
                "source",
                P14X86_64BaselineBackedgeSafepointAuthority {
                    source_bytecode_index: BytecodeIndex::from_offset(2),
                    ..exact
                },
            ),
            (
                "target",
                P14X86_64BaselineBackedgeSafepointAuthority {
                    target_bytecode_index: BytecodeIndex::from_offset(1),
                    ..exact
                },
            ),
            (
                "opcode",
                P14X86_64BaselineBackedgeSafepointAuthority {
                    opcode: CoreOpcode::JumpIfFalse,
                    ..exact
                },
            ),
            (
                "kind",
                P14X86_64BaselineBackedgeSafepointAuthority {
                    kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                    ..exact
                },
            ),
        ] {
            let result = plan_p6_x86_64_baseline_lowering(
                P6X86_64BaselineLoweringRequest::new_with_backedge_safepoints(
                    owner(),
                    &code_block,
                    proof,
                    &[authority],
                ),
            );
            assert!(
                matches!(
                    result,
                    Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                        reason:
                            P6X86_64BaselineBranchTargetRejectionReason::BackwardWithoutSafepointAuthority,
                        ..
                    })
                ),
                "{name}: {result:?}"
            );
        }

        let lowering = plan_p6_x86_64_baseline_lowering(
            P6X86_64BaselineLoweringRequest::new_with_backedge_safepoints(
                owner(),
                &code_block,
                proof,
                &[exact],
            ),
        )
        .expect("exact P14 backedge authority accepts");
        assert_eq!(
            lowering.plan.operations[1].bytecode_index,
            BytecodeIndex::from_offset(1)
        );
    }

    #[test]
    fn p8a_lowering_rejects_branch_target_to_runtime_helper_native_exit() {
        let helper_index = BytecodeIndex::from_offset(1);
        let root_map = BytecodeRootMap {
            id: BytecodeRootMapId(77),
            owner: Some(owner()),
            bytecode_range_start: helper_index,
            bytecode_range_end: helper_index,
            slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                helper_index,
                local(0),
                BytecodeRootSlotKind::VirtualRegister,
            )],
            complete: true,
        };
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(helper_index)],
            ),
            typed_instruction(1, CoreOpcode::NewObject, vec![Operand::Register(local(0))]),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
        .with_side_tables(LinkedSideTables {
            root_maps: vec![root_map],
            ..LinkedSideTables::default()
        })
        .with_root_map_owner(owner());
        let derivation =
            crate::jit::plan::derive_baseline_generated_runtime_helper_plan_from_code_block(
                &code_block,
                owner(),
            )
            .unwrap();
        let runtime_helper_plan = derivation.metadata.expect("runtime helper metadata");
        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner(),
                lowering_snapshot(),
                BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBranchNullish,
                derivation.safepoints,
                &runtime_helper_plan,
            )
            .unwrap();

        assert_eq!(
            plan_p6_x86_64_baseline_lowering_with_runtime_helper_native_exits(
                P6X86_64BaselineLoweringRequest::new(owner(), &code_block, proof),
                &runtime_helper_plan,
            ),
            Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index: BytecodeIndex::from_offset(0),
                opcode: CoreOpcode::Jump,
                target: helper_index,
                reason: P6X86_64BaselineBranchTargetRejectionReason::RuntimeHelperNativeExit,
            })
        );
    }

    #[test]
    fn p6_backend_contract_records_value_abi_frame_and_operand_locations() {
        let code_block = p6_lowering_code_block();
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let layout = static_value_representation_layout();

        assert_eq!(contract.owner, owner());
        assert_eq!(contract.bytecode, lowering.plan.bytecode);
        assert_eq!(contract.opcode_subset, lowering.plan.opcode_subset);
        assert_eq!(contract.effect_contract, lowering.plan.effect_contract);
        assert_eq!(contract.bytecode_snapshot, lowering.plan.bytecode_snapshot);
        assert_eq!(
            contract.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset
        );
        assert_eq!(contract.architecture, AssemblerArchitecture::X86_64);
        assert_eq!(
            contract.byte_emission,
            P6X86_64BaselineLoweringByteEmission::NotGenerated
        );
        assert_eq!(
            contract.callable_authority,
            P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority
        );
        assert_absent_artifacts(&contract.artifact_contract);

        assert_eq!(contract.value_layout.layout_name, layout.name);
        assert_eq!(contract.value_layout.storage_bits, 64);
        assert_eq!(contract.value_layout.slot_width_bytes, 8);
        assert_eq!(contract.value_layout.tag_mask, 0xff);
        assert_eq!(contract.value_layout.payload_shift, 8);
        assert_eq!(
            contract.value_layout.immediate_undefined_tag,
            layout.tag_for_immediate_name("undefined").unwrap()
        );
        assert_eq!(
            contract.value_layout.immediate_null_tag,
            layout.tag_for_immediate_name("null").unwrap()
        );
        assert_eq!(
            contract.value_layout.immediate_false_tag,
            layout.tag_for_immediate_name("false").unwrap()
        );
        assert_eq!(
            contract.value_layout.immediate_true_tag,
            layout.tag_for_immediate_name("true").unwrap()
        );
        assert_eq!(
            contract.value_layout.immediate_int32_tag,
            layout.tag_for_immediate_name("int32").unwrap()
        );
        assert_eq!(
            contract.value_layout.immediate_double_tag,
            layout.tag_for_immediate_name("double").unwrap()
        );
        assert_eq!(contract.value_layout.cell_tag, layout.cell_tag);
        assert_eq!(contract.value_layout.double_tag, layout.double_tag);

        assert_eq!(contract.frame_layout.value_slot_width_bytes, 8);
        assert_eq!(contract.frame_layout.local_zero_slot_index, 0);
        assert_eq!(contract.frame_layout.local_slot_stride_bytes, 8);
        assert_eq!(
            contract.frame_layout.this_argument_offset,
            ThisArgumentOffset(5)
        );
        assert!(
            !contract
                .frame_layout
                .header_registers_below_this_are_value_addressable
        );
        assert_eq!(
            contract.frame_layout.constants,
            P6X86_64BaselineConstantsLocationContract::ReadOnlyOutOfFrame
        );

        assert_eq!(contract.abi.pinned_vm.role, RegisterRole::PinnedVm);
        assert_eq!(contract.abi.pinned_vm.value, AbiValue::Pointer);
        assert_eq!(
            contract.abi.pinned_call_frame.role,
            RegisterRole::PinnedCallFrame
        );
        assert_eq!(contract.abi.pinned_call_frame.value, AbiValue::Pointer);
        assert_eq!(
            contract.abi.js_value_return,
            P6X86_64BaselineReturnRegisterContract {
                role: RegisterRole::Return,
                value: AbiValue::JsValue,
            }
        );
        assert_eq!(contract.abi.stack_alignment_bytes, 16);
        assert!(contract.abi.stack_alignment_applies_at_entry);
        assert!(contract.abi.stack_alignment_applies_at_runtime_calls);
        assert!(contract
            .abi
            .runtime_clobbered_roles
            .contains(&RegisterRole::Argument));
        assert!(contract
            .abi
            .runtime_clobbered_roles
            .contains(&RegisterRole::Return));
        assert!(contract
            .abi
            .runtime_preserved_roles
            .contains(&RegisterRole::PinnedVm));
        assert!(contract
            .abi
            .runtime_preserved_roles
            .contains(&RegisterRole::PinnedCallFrame));

        assert_eq!(contract.instructions.len(), lowering.plan.operations.len());
        assert_eq!(
            contract.instructions[0].operand_locations,
            vec![
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Destination,
                    location: P6X86_64BaselineOperandLocation::FrameLocal {
                        local_index: 0,
                        slot_index: 0,
                        byte_offset: 0,
                    },
                },
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Source,
                    location: P6X86_64BaselineOperandLocation::Immediate(
                        P6X86_64BaselineImmediateOperand::Undefined,
                    ),
                },
            ]
        );
        assert_eq!(
            contract.instructions[7].operand_locations,
            vec![
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Destination,
                    location: P6X86_64BaselineOperandLocation::FrameLocal {
                        local_index: 7,
                        slot_index: 7,
                        byte_offset: 56,
                    },
                },
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Left,
                    location: P6X86_64BaselineOperandLocation::FrameLocal {
                        local_index: 5,
                        slot_index: 5,
                        byte_offset: 40,
                    },
                },
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Right,
                    location: P6X86_64BaselineOperandLocation::FrameLocal {
                        local_index: 6,
                        slot_index: 6,
                        byte_offset: 48,
                    },
                },
            ]
        );
        assert_eq!(
            contract.instructions[10].operand_locations,
            vec![
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::ReturnValue,
                    location: P6X86_64BaselineOperandLocation::FrameLocal {
                        local_index: 9,
                        slot_index: 9,
                        byte_offset: 72,
                    },
                },
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Destination,
                    location: P6X86_64BaselineOperandLocation::ReturnCarrier {
                        role: RegisterRole::Return,
                        value: AbiValue::JsValue,
                    },
                },
            ]
        );
    }

    #[test]
    fn p6_backend_contract_records_argument_and_constant_sources() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::Move,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(argument_including_this(1)),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Move,
                vec![Operand::Register(local(1)), Operand::Register(constant(2))],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();

        assert_eq!(
            contract.instructions[0].operand_locations[1],
            P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::FrameArgument {
                    argument_index_including_this: 1,
                    raw_virtual_register: 6,
                    byte_offset_from_argument_base: 8,
                },
            }
        );
        assert_eq!(
            contract.instructions[1].operand_locations[1],
            P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::Constant {
                    constant_index: 2,
                    read_only: true,
                },
            }
        );
    }

    #[test]
    fn p6_backend_contract_rejects_invalid_constant_and_header_destinations() {
        let accepted = {
            let code_block = p6_lowering_code_block();
            let proof = p6_lowering_proof(&code_block);
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            ))
            .unwrap()
        };

        let cases = [
            (
                VirtualRegister::INVALID,
                P6X86_64BaselineOperandLocationError::InvalidRegister,
            ),
            (
                constant(0),
                P6X86_64BaselineOperandLocationError::ConstantDestination { constant_index: 0 },
            ),
            (
                header(4),
                P6X86_64BaselineOperandLocationError::HeaderAsValueOperandUnsupported {
                    raw_slot: 4,
                },
            ),
        ];

        for (destination, error) in cases {
            let mut lowering = accepted.clone();
            lowering.plan.operations[0].operation =
                P6X86_64BaselineLoweredOperation::LoadUndefined { destination };

            assert_eq!(
                record_p6_x86_64_baseline_backend_contract(&lowering),
                Err(P6X86_64BaselineBackendContractError::OperandLocation {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    role: P6X86_64BaselineOperandRole::Destination,
                    register: destination,
                    error,
                })
            );
        }
    }

    #[test]
    fn p6_backend_contract_rejects_header_sources_as_values() {
        let code_block = code_block_from_typed_instructions(vec![typed_instruction(
            0,
            CoreOpcode::Move,
            vec![Operand::Register(local(0)), Operand::Register(header(4))],
        )]);
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            record_p6_x86_64_baseline_backend_contract(&lowering),
            Err(P6X86_64BaselineBackendContractError::OperandLocation {
                bytecode_index: BytecodeIndex::from_offset(0),
                role: P6X86_64BaselineOperandRole::Source,
                register: header(4),
                error: P6X86_64BaselineOperandLocationError::HeaderAsValueOperandUnsupported {
                    raw_slot: 4,
                },
            })
        );
    }

    fn assert_exit_contract(
        actual: P6X86_64BaselineArithmeticSideExitContract,
        reason: P6X86_64BaselineArithmeticSideExitReason,
        bytecode_index: BytecodeIndex,
    ) {
        assert_eq!(actual.reason, reason);
        assert_eq!(
            actual.destination,
            P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged
        );
        assert_eq!(actual.retained_bytecode_index, bytecode_index);
        assert!(!actual.may_throw);
        assert!(!actual.runtime_call);
        assert!(!actual.heap_allocation);
        assert!(!actual.touches_gc_roots);
    }

    #[test]
    fn p6_backend_contract_records_int32_arithmetic_exit_policy() {
        let contract = p6_backend_contract();

        let add = contract.instructions[7].arithmetic_exit_policy.unwrap();
        assert_eq!(add.operation, P6X86_64BaselineInt32ArithmeticOperation::Add);
        assert_eq!(
            add.operand_guard,
            P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
        );
        assert_eq!(
            add.checked_arithmetic,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedAdd
        );
        assert_eq!(
            add.negative_zero_policy,
            P6X86_64BaselineMulNegativeZeroPolicy::NotApplicable
        );
        assert_exit_contract(
            add.non_int32_exit,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
            BytecodeIndex::from_offset(7),
        );
        assert_exit_contract(
            add.overflow_exit,
            P6X86_64BaselineArithmeticSideExitReason::Overflow,
            BytecodeIndex::from_offset(7),
        );

        let sub = contract.instructions[8].arithmetic_exit_policy.unwrap();
        assert_eq!(sub.operation, P6X86_64BaselineInt32ArithmeticOperation::Sub);
        assert_eq!(
            sub.checked_arithmetic,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedSub
        );
        assert_eq!(
            sub.negative_zero_policy,
            P6X86_64BaselineMulNegativeZeroPolicy::NotApplicable
        );
        assert_exit_contract(
            sub.non_int32_exit,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
            BytecodeIndex::from_offset(8),
        );
        assert_exit_contract(
            sub.overflow_exit,
            P6X86_64BaselineArithmeticSideExitReason::Overflow,
            BytecodeIndex::from_offset(8),
        );

        let mul = contract.instructions[9].arithmetic_exit_policy.unwrap();
        assert_eq!(mul.operation, P6X86_64BaselineInt32ArithmeticOperation::Mul);
        assert_eq!(
            mul.checked_arithmetic,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedMul
        );
        assert_eq!(
            mul.negative_zero_policy,
            P6X86_64BaselineMulNegativeZeroPolicy::NegativeZeroSideExitRequired
        );
        assert_exit_contract(
            mul.non_int32_exit,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
            BytecodeIndex::from_offset(9),
        );
        assert_exit_contract(
            mul.overflow_exit,
            P6X86_64BaselineArithmeticSideExitReason::Overflow,
            BytecodeIndex::from_offset(9),
        );
    }

    #[test]
    fn p6_backend_contract_has_no_byte_artifact_or_code_block_side_effects() {
        let code_block = p6_lowering_code_block();
        let entrypoints_before = *code_block.entrypoints();
        let lifecycle_before = code_block.lifecycle();
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();

        assert_eq!(
            contract.byte_emission,
            P6X86_64BaselineLoweringByteEmission::NotGenerated
        );
        assert_eq!(
            contract.callable_authority,
            P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority
        );
        assert_absent_artifacts(&contract.artifact_contract);
        assert_eq!(*code_block.entrypoints(), entrypoints_before);
        assert_eq!(code_block.lifecycle(), lifecycle_before);
        assert!(code_block.entrypoints().baseline_jit.is_none());
    }

    #[test]
    fn p6_instruction_selection_selects_symbolic_golden_machine_records() {
        let contract = p6_backend_contract();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();

        assert_eq!(selection.validate_against(&contract), Ok(()));
        assert_eq!(selection.instructions.len(), 11);
        for (selected, contract_instruction) in selection
            .instructions
            .iter()
            .zip(contract.instructions.iter())
        {
            assert_eq!(selected.lowered, contract_instruction.lowered);
            assert_eq!(
                selected.operand_locations,
                contract_instruction.operand_locations
            );
            assert_eq!(
                selected.effects,
                P6X86_64BaselineSelectedInstructionEffects::no_runtime_allocation_or_roots()
            );
        }

        assert_eq!(
            selection.instructions[0].machine_instructions,
            expected_immediate_load_selection(0, contract.value_layout.immediate_undefined_tag)
        );
        assert_eq!(
            selection.instructions[1].machine_instructions,
            expected_immediate_load_selection(1, contract.value_layout.immediate_null_tag)
        );
        assert_eq!(
            selection.instructions[2].machine_instructions,
            expected_immediate_load_selection(2, contract.value_layout.immediate_true_tag)
        );
        assert_eq!(
            selection.instructions[3].machine_instructions,
            expected_immediate_load_selection(
                3,
                (7_u64 << contract.value_layout.payload_shift)
                    | contract.value_layout.immediate_int32_tag,
            )
        );
        assert_eq!(
            selection.instructions[4].machine_instructions,
            expected_move_selection(frame_memory_operand(4), frame_memory_operand(3))
        );
        assert_eq!(
            selection.instructions[7].machine_instructions,
            expected_int32_arithmetic_selection(
                &contract,
                7,
                P6X86_64BaselineInt32ArithmeticOperation::Add,
                7,
                5,
                6,
            )
        );
        assert_eq!(
            selection.instructions[8].machine_instructions,
            expected_int32_arithmetic_selection(
                &contract,
                8,
                P6X86_64BaselineInt32ArithmeticOperation::Sub,
                8,
                7,
                6,
            )
        );
        assert_eq!(
            selection.instructions[9].machine_instructions,
            expected_int32_arithmetic_selection(
                &contract,
                9,
                P6X86_64BaselineInt32ArithmeticOperation::Mul,
                9,
                8,
                6,
            )
        );
        assert_eq!(
            selection.instructions[10].machine_instructions,
            expected_return_selection(frame_memory_operand(9), contract.abi.js_value_return)
        );
    }

    #[test]
    fn p6_instruction_selection_preserves_no_byte_no_callable_artifact_or_readiness_authority() {
        let code_block = p6_lowering_code_block();
        let entrypoints_before = *code_block.entrypoints();
        let lifecycle_before = code_block.lifecycle();
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();

        assert_eq!(
            selection.byte_emission,
            P6X86_64BaselineInstructionSelectionByteEmission::NotGenerated
        );
        assert_eq!(
            selection.callable_authority,
            P6X86_64BaselineInstructionSelectionCallableAuthority::NoCallableAuthority
        );
        assert_absent_artifacts(&selection.artifact_contract);
        assert_eq!(
            selection.readiness,
            P6X86_64BaselineInstructionSelectionReadiness::NoArtifactsNoVmPlatformExecutableReadiness
        );
        assert!(selection.instructions.iter().all(|instruction| {
            instruction.effects
                == P6X86_64BaselineSelectedInstructionEffects::no_runtime_allocation_or_roots()
        }));
        assert_eq!(*code_block.entrypoints(), entrypoints_before);
        assert_eq!(code_block.lifecycle(), lifecycle_before);
        assert!(code_block.entrypoints().baseline_jit.is_none());
    }

    #[test]
    fn p6_instruction_selection_selects_argument_and_constant_sources() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::Move,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(argument_including_this(1)),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Move,
                vec![Operand::Register(local(1)), Operand::Register(constant(2))],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();

        assert_eq!(
            selection.instructions[0].machine_instructions,
            expected_move_selection(
                frame_memory_operand(0),
                memory_operand(
                    P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                    frame_argument_location(1),
                ),
            )
        );
        assert_eq!(
            selection.instructions[1].machine_instructions,
            expected_move_selection(
                frame_memory_operand(1),
                memory_operand(
                    P6X86_64BaselineSymbolicRegister::PinnedVm,
                    constant_location(2, true),
                ),
            )
        );
    }

    #[test]
    fn p6_instruction_selection_rejects_invalid_operand_locations() {
        let mut constant_destination = p6_backend_contract();
        let invalid_destination = constant_location(0, true);
        constant_destination.instructions[0].operand_locations[0].location = invalid_destination;

        assert_eq!(
            select_p6_x86_64_baseline_instructions(&constant_destination),
            Err(
                P6X86_64BaselineInstructionSelectionError::OperandLocationMismatch {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    role: P6X86_64BaselineOperandRole::Destination,
                    expected: frame_local_location(0),
                    actual: invalid_destination,
                },
            )
        );

        let mut writable_constant_source = p6_backend_contract();
        let invalid_source = constant_location(1, false);
        writable_constant_source.instructions[4].operand_locations[1].location = invalid_source;

        assert_eq!(
            select_p6_x86_64_baseline_instructions(&writable_constant_source),
            Err(
                P6X86_64BaselineInstructionSelectionError::OperandLocationMismatch {
                    bytecode_index: BytecodeIndex::from_offset(4),
                    role: P6X86_64BaselineOperandRole::Source,
                    expected: frame_local_location(3),
                    actual: invalid_source,
                },
            )
        );

        let mut immediate_move_source = p6_backend_contract();
        let invalid_source =
            P6X86_64BaselineOperandLocation::Immediate(P6X86_64BaselineImmediateOperand::Int32(1));
        immediate_move_source.instructions[4].operand_locations[1].location = invalid_source;

        assert_eq!(
            select_p6_x86_64_baseline_instructions(&immediate_move_source),
            Err(
                P6X86_64BaselineInstructionSelectionError::OperandLocationMismatch {
                    bytecode_index: BytecodeIndex::from_offset(4),
                    role: P6X86_64BaselineOperandRole::Source,
                    expected: frame_local_location(3),
                    actual: invalid_source,
                },
            )
        );
    }

    #[test]
    fn p6_instruction_selection_rejects_tampered_canonical_backend_contract_fields() {
        let mut tampered_value_layout = p6_backend_contract();
        tampered_value_layout.value_layout.immediate_int32_tag ^= 1;
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_value_layout),
            Err(
                P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                    field: "value_layout",
                },
            )
        );

        let mut tampered_frame_layout = p6_backend_contract();
        tampered_frame_layout.frame_layout.local_slot_stride_bytes = 16;
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_frame_layout),
            Err(
                P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                    field: "frame_layout",
                },
            )
        );

        let mut tampered_abi = p6_backend_contract();
        tampered_abi.abi.js_value_return.value = AbiValue::Pointer;
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_abi),
            Err(
                P6X86_64BaselineInstructionSelectionError::BackendContractMismatch { field: "abi" },
            )
        );

        let mut tampered_return_carrier = p6_backend_contract();
        let actual = P6X86_64BaselineOperandLocation::ReturnCarrier {
            role: RegisterRole::Argument,
            value: AbiValue::JsValue,
        };
        tampered_return_carrier.instructions[10].operand_locations[1].location = actual;
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_return_carrier),
            Err(
                P6X86_64BaselineInstructionSelectionError::OperandLocationMismatch {
                    bytecode_index: BytecodeIndex::from_offset(10),
                    role: P6X86_64BaselineOperandRole::Destination,
                    expected: P6X86_64BaselineOperandLocation::ReturnCarrier {
                        role: RegisterRole::Return,
                        value: AbiValue::JsValue,
                    },
                    actual,
                },
            )
        );
    }

    #[test]
    fn p6_instruction_selection_rejects_tampered_bytecode_range_count_and_order() {
        let contract = p6_backend_contract();

        let mut tampered_count = contract.clone();
        tampered_count.instructions.pop();
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_count),
            Err(
                P6X86_64BaselineInstructionSelectionError::InstructionCountMismatch {
                    expected: contract.bytecode.instruction_count as usize,
                    actual: contract.instructions.len() - 1,
                },
            )
        );

        let mut tampered_start = contract.clone();
        tampered_start.bytecode.start = BytecodeIndex::from_offset(1);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_start),
            Err(
                P6X86_64BaselineInstructionSelectionError::InstructionBytecodeRangeMismatch {
                    field: "start",
                    expected: BytecodeIndex::from_offset(1),
                    actual: BytecodeIndex::from_offset(0),
                },
            )
        );

        let mut tampered_order = contract.clone();
        tampered_order.instructions.swap(7, 8);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&tampered_order),
            Err(
                P6X86_64BaselineInstructionSelectionError::InstructionBytecodeOrderMismatch {
                    previous: BytecodeIndex::from_offset(8),
                    actual: BytecodeIndex::from_offset(7),
                },
            )
        );
    }

    #[test]
    fn p6_instruction_selection_rejects_tampered_plan_and_proof() {
        let contract = p6_backend_contract();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();

        let mut tampered_proof = selection.clone();
        tampered_proof.proof.selection_fingerprint ^= 1;
        assert_eq!(
            tampered_proof.validate_against(&contract),
            Err(P6X86_64BaselineInstructionSelectionError::SelectionProofMismatch)
        );

        let mut tampered_plan = selection.clone();
        tampered_plan.owner = CodeBlockId(CellId(2));
        tampered_plan.proof = tampered_plan.expected_proof(&contract);
        assert_eq!(
            tampered_plan.validate_against(&contract),
            Err(P6X86_64BaselineInstructionSelectionError::PlanContractMismatch { field: "owner" },)
        );

        let mut tampered_instruction = selection.clone();
        tampered_instruction.instructions[0]
            .machine_instructions
            .clear();
        tampered_instruction.proof = tampered_instruction.expected_proof(&contract);
        assert_eq!(
            tampered_instruction.validate_against(&contract),
            Err(
                P6X86_64BaselineInstructionSelectionError::SelectedInstructionMismatch {
                    bytecode_index: BytecodeIndex::from_offset(0),
                },
            )
        );

        let mut wrong_architecture = contract.clone();
        wrong_architecture.architecture = AssemblerArchitecture::Arm64;
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&wrong_architecture),
            Err(P6X86_64BaselineInstructionSelectionError::Contract {
                error: P6X86_64BaselineBackendContractError::UnexpectedArchitecture {
                    expected: AssemblerArchitecture::X86_64,
                    actual: AssemblerArchitecture::Arm64,
                },
            })
        );
    }

    #[test]
    fn p6_instruction_selection_rejects_missing_unexpected_and_wrong_arithmetic_policies() {
        let mut missing_policy = p6_backend_contract();
        missing_policy.instructions[7].arithmetic_exit_policy = None;
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&missing_policy),
            Err(
                P6X86_64BaselineInstructionSelectionError::MissingArithmeticPolicy {
                    bytecode_index: BytecodeIndex::from_offset(7),
                },
            )
        );

        let mut unexpected_policy = p6_backend_contract();
        let actual = p6_int32_arithmetic_exit_policy(
            BytecodeIndex::from_offset(0),
            P6X86_64BaselineInt32ArithmeticOperation::Add,
            P6X86_64BaselineCheckedInt32Arithmetic::CheckedAdd,
        );
        unexpected_policy.instructions[0].arithmetic_exit_policy = Some(actual);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&unexpected_policy),
            Err(
                P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    actual,
                },
            )
        );

        let mut wrong_policy = p6_backend_contract();
        let mut actual = wrong_policy.instructions[7].arithmetic_exit_policy.unwrap();
        actual.operation = P6X86_64BaselineInt32ArithmeticOperation::Sub;
        wrong_policy.instructions[7].arithmetic_exit_policy = Some(actual);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&wrong_policy),
            Err(
                P6X86_64BaselineInstructionSelectionError::ArithmeticPolicyMismatch {
                    bytecode_index: BytecodeIndex::from_offset(7),
                    expected_operation: P6X86_64BaselineInt32ArithmeticOperation::Add,
                    expected_checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic::CheckedAdd,
                    actual,
                },
            )
        );
    }

    #[test]
    fn p6_instruction_selection_records_and_validates_arithmetic_side_exits() {
        let contract = p6_backend_contract();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();

        assert_eq!(
            selection.instructions[9].machine_instructions[7],
            P6X86_64BaselineMachineInstruction::CheckMulNegativeZero {
                result: P6X86_64BaselineSymbolicRegister::Scratch2,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                on_negative_zero: side_exit_label(
                    P6X86_64BaselineSelectedSideExitReason::NegativeZero,
                    9,
                ),
            }
        );

        let mut throwing_non_int32_exit = contract.clone();
        let mut policy = throwing_non_int32_exit.instructions[7]
            .arithmetic_exit_policy
            .unwrap();
        policy.non_int32_exit.may_throw = true;
        throwing_non_int32_exit.instructions[7].arithmetic_exit_policy = Some(policy);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&throwing_non_int32_exit),
            Err(
                P6X86_64BaselineInstructionSelectionError::SideExitContractMismatch {
                    bytecode_index: BytecodeIndex::from_offset(7),
                    expected_reason: P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                    actual: policy.non_int32_exit,
                },
            )
        );

        let mut wrong_non_int32_reason = contract.clone();
        let mut policy = wrong_non_int32_reason.instructions[7]
            .arithmetic_exit_policy
            .unwrap();
        policy.non_int32_exit.reason = P6X86_64BaselineArithmeticSideExitReason::Overflow;
        wrong_non_int32_reason.instructions[7].arithmetic_exit_policy = Some(policy);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&wrong_non_int32_reason),
            Err(
                P6X86_64BaselineInstructionSelectionError::SideExitContractMismatch {
                    bytecode_index: BytecodeIndex::from_offset(7),
                    expected_reason: P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                    actual: policy.non_int32_exit,
                },
            )
        );

        let mut wrong_overflow_index = contract.clone();
        let mut policy = wrong_overflow_index.instructions[8]
            .arithmetic_exit_policy
            .unwrap();
        policy.overflow_exit.retained_bytecode_index = BytecodeIndex::from_offset(99);
        wrong_overflow_index.instructions[8].arithmetic_exit_policy = Some(policy);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&wrong_overflow_index),
            Err(
                P6X86_64BaselineInstructionSelectionError::SideExitContractMismatch {
                    bytecode_index: BytecodeIndex::from_offset(8),
                    expected_reason: P6X86_64BaselineArithmeticSideExitReason::Overflow,
                    actual: policy.overflow_exit,
                },
            )
        );

        let mut missing_negative_zero_exit = contract.clone();
        let mut policy = missing_negative_zero_exit.instructions[9]
            .arithmetic_exit_policy
            .unwrap();
        policy.negative_zero_policy = P6X86_64BaselineMulNegativeZeroPolicy::NotApplicable;
        missing_negative_zero_exit.instructions[9].arithmetic_exit_policy = Some(policy);
        assert_eq!(
            select_p6_x86_64_baseline_instructions(&missing_negative_zero_exit),
            Err(
                P6X86_64BaselineInstructionSelectionError::NegativeZeroPolicyMismatch {
                    bytecode_index: BytecodeIndex::from_offset(9),
                    expected: P6X86_64BaselineMulNegativeZeroPolicy::NegativeZeroSideExitRequired,
                    actual: P6X86_64BaselineMulNegativeZeroPolicy::NotApplicable,
                },
            )
        );
    }

    #[test]
    fn p8b_instruction_selection_records_jump_if_false_branch_and_truthiness_side_exit() {
        let code_block = p8b_jump_if_false_code_block(P8bTruthinessSource::Int32(0));
        let proof = p8b_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();

        assert_eq!(
            contract.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBranchTruthinessSubset
        );
        assert_eq!(
            contract.instructions[1].branch_target,
            Some(P6X86_64BaselineControlFlowBranchContract {
                kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                source_bytecode_index: BytecodeIndex::from_offset(1),
                target_bytecode_index: BytecodeIndex::from_offset(4),
            })
        );

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();

        assert_eq!(
            selection.instructions[1].machine_instructions,
            vec![
                P6X86_64BaselineMachineInstruction::LoadQ {
                    destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                    source: frame_memory_operand(0),
                },
                P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive {
                    value: P6X86_64BaselineSymbolicRegister::Scratch0,
                    undefined_tag: contract.value_layout.immediate_undefined_tag,
                    null_tag: contract.value_layout.immediate_null_tag,
                    false_tag: contract.value_layout.immediate_false_tag,
                    true_tag: contract.value_layout.immediate_true_tag,
                    int32_tag: contract.value_layout.immediate_int32_tag,
                    unsupported_exit: side_exit_label(
                        P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
                        1,
                    ),
                    target: P6X86_64BaselineControlFlowBranchContract {
                        kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                        source_bytecode_index: BytecodeIndex::from_offset(1),
                        target_bytecode_index: BytecodeIndex::from_offset(4),
                    },
                },
            ]
        );
    }

    #[test]
    fn p6_semantic_emitter_produces_actual_non_executable_x86_64_bytes() {
        let result = p6_semantic_emission().unwrap();

        assert_eq!(
            result.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset
        );
        assert_eq!(
            result.byte_shape,
            P6X86_64BaselineSemanticByteEmissionShape::P2aSemanticX86_64FromAcceptedP6Selection
        );
        assert_eq!(
            result.authority,
            P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableNonCallableSemanticBytesOnly
        );
        assert_eq!(result.entry_offset, 0);
        assert_eq!(
            result.physical_registers,
            p6_x86_64_semantic_physical_register_map()
        );
        assert_eq!(result.instruction_bytes.len(), 11);
        assert!(!result.source_image.bytes().is_empty());
        assert_eq!(&result.source_image.bytes()[0..2], &[0x49, 0xba]);
        assert_eq!(
            result.source_image.bytes()[result.terminal_policy.ret_offset as usize],
            0xc3
        );

        assert_eq!(result.source_buffer.data_kind, AssemblerDataKind::Code);
        assert_eq!(
            result.source_buffer.lifecycle,
            AssemblerBufferLifecycle::FrozenForLink
        );
        assert_eq!(
            result.source_buffer.architecture,
            Some(AssemblerArchitecture::X86_64)
        );
        assert_eq!(
            result.source_buffer.byte_len as usize,
            result.source_image.bytes().len()
        );
        assert_eq!(result.source_buffer.relocations, Vec::new());

        assert_eq!(
            result.source_image.descriptor().data_kind,
            AssemblerDataKind::Code
        );
        assert_eq!(
            result.source_image.descriptor().source_lifecycle,
            AssemblerBufferLifecycle::FrozenForLink
        );
        assert_eq!(
            result.source_image.descriptor().architecture,
            Some(AssemblerArchitecture::X86_64)
        );
        assert_eq!(result.source_image.descriptor().relocation_count, 0);

        assert_eq!(result.linked_image.profile, LinkBufferProfile::Baseline);
        assert_eq!(result.linked_image.state, LinkBufferState::Linked);
        assert_eq!(result.linked_image.relocation_count, 0);
        assert_eq!(result.linked_image.bytes(), result.source_image.bytes());
        assert_eq!(
            validate_p6_x86_64_semantic_byte_images(
                &result.source_buffer,
                &result.source_image,
                &result.linked_image,
                result.source_buffer.byte_len,
                result.entry_offset,
            ),
            Ok(())
        );
    }

    #[test]
    fn p6_semantic_result_carries_no_native_readiness_or_finalization_authority() {
        let result = p6_semantic_emission().unwrap();

        assert_eq!(
            result.authority,
            P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableNonCallableSemanticBytesOnly
        );
        assert_eq!(
            result.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::SingleFinalNormalReturnRetThenInlineUd2SideExits
        );
        assert_eq!(
            result.terminal_policy.return_bytecode_index,
            BytecodeIndex::from_offset(10)
        );
        assert_eq!(result.source_buffer.relocations.len(), 0);
        assert_eq!(result.source_image.descriptor().relocation_count, 0);
        assert_eq!(result.linked_image.relocation_count, 0);
        assert_eq!(result.linked_image.state, LinkBufferState::Linked);
        assert_ne!(
            result.source_buffer.lifecycle,
            AssemblerBufferLifecycle::CopiedToExecutableMemory
        );
    }

    #[test]
    fn p6_semantic_emitter_rejects_tampered_contract_and_selection_before_images() {
        let (contract, selection) = p6_backend_contract_and_selection();

        let mut wrong_architecture = contract.clone();
        wrong_architecture.architecture = AssemblerArchitecture::Arm64;
        assert_eq!(
            emit_p6_x86_64_baseline_semantic_bytes(wrong_architecture, selection.clone()),
            Err(P6X86_64BaselineSemanticByteEmissionError::Selection {
                error: P6X86_64BaselineInstructionSelectionError::Contract {
                    error: P6X86_64BaselineBackendContractError::UnexpectedArchitecture {
                        expected: AssemblerArchitecture::X86_64,
                        actual: AssemblerArchitecture::Arm64,
                    },
                },
            })
        );

        let mut tampered_selection = selection;
        tampered_selection.instructions[0]
            .machine_instructions
            .clear();
        tampered_selection.proof = tampered_selection.expected_proof(&contract);
        assert_eq!(
            emit_p6_x86_64_baseline_semantic_bytes(contract, tampered_selection),
            Err(P6X86_64BaselineSemanticByteEmissionError::Selection {
                error: P6X86_64BaselineInstructionSelectionError::SelectedInstructionMismatch {
                    bytecode_index: BytecodeIndex::from_offset(0),
                },
            })
        );
    }

    #[test]
    fn p6_semantic_emitter_rejects_missing_multiple_and_non_final_returns() {
        let no_return = code_block_from_typed_instructions(vec![typed_instruction(
            0,
            CoreOpcode::LoadInt32,
            vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
        )]);
        assert_eq!(
            p6_semantic_emission_for_code_block(&no_return),
            Err(P6X86_64BaselineSemanticByteEmissionError::MissingReturn)
        );

        let multiple_returns = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        assert_eq!(
            p6_semantic_emission_for_code_block(&multiple_returns),
            Err(P6X86_64BaselineSemanticByteEmissionError::MultipleReturns {
                first: BytecodeIndex::from_offset(1),
                second: BytecodeIndex::from_offset(2),
            })
        );

        let non_final_return = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
            typed_instruction(
                2,
                CoreOpcode::Move,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
        ]);
        assert_eq!(
            p6_semantic_emission_for_code_block(&non_final_return),
            Err(P6X86_64BaselineSemanticByteEmissionError::NonFinalReturn {
                bytecode_index: BytecodeIndex::from_offset(1),
                next_bytecode_index: BytecodeIndex::from_offset(2),
            })
        );
    }

    #[test]
    fn p6_semantic_emitter_rejects_frame_argument_and_constant_memory_sources() {
        let frame_argument_source = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::Move,
                vec![
                    Operand::Register(local(0)),
                    Operand::Register(argument_including_this(1)),
                ],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        assert_eq!(
            p6_semantic_emission_for_code_block(&frame_argument_source),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    location: frame_argument_location(1),
                    reason:
                        P6X86_64BaselineSemanticOperandRejectionReason::FrameArgumentUnsupported,
                }
            )
        );

        let constant_source = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::Move,
                vec![Operand::Register(local(0)), Operand::Register(constant(2))],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        assert_eq!(
            p6_semantic_emission_for_code_block(&constant_source),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    location: constant_location(2, true),
                    reason:
                        P6X86_64BaselineSemanticOperandRejectionReason::ConstantMemoryUnsupported,
                }
            )
        );
    }

    #[test]
    fn p6_semantic_side_exits_branch_to_inline_ud2_placeholders_after_ret() {
        let result = p6_semantic_emission().unwrap();
        let bytes = result.source_image.bytes();

        assert_eq!(result.side_exit_placeholders.len(), 10);
        assert_eq!(
            result.terminal_policy.normal_path_end_offset,
            result.terminal_policy.ret_offset + 1
        );
        for placeholder in &result.side_exit_placeholders {
            assert!(placeholder.target_offset >= result.terminal_policy.normal_path_end_offset);
            assert_eq!(
                &bytes[placeholder.target_offset as usize..placeholder.target_offset as usize + 2],
                &[0x0f, 0x0b]
            );

            assert_eq!(
                rel32_branch_target(bytes, placeholder.branch_end_offset),
                i64::from(placeholder.target_offset)
            );
        }
        for record in &result.instruction_bytes {
            assert_eq!(
                record.bytes,
                bytes[record.start_offset as usize..record.end_offset as usize]
            );
        }
    }

    #[test]
    fn p6_callable_semantic_emitter_produces_c_abi_envelope_without_platform_authority() {
        let result = p6_callable_semantic_emission().unwrap();
        let bytes = result.source_image.bytes();

        assert_eq!(
            result.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset
        );
        assert_eq!(
            result.byte_shape,
            P6X86_64BaselineSemanticByteEmissionShape::P3bCallableCAbiSemanticX86_64FromAcceptedP6Selection
        );
        assert_eq!(
            result.authority,
            P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableCallableSemanticBytesOnlyNoVmOrPlatformAuthority
        );
        assert_eq!(
            result.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs
        );
        assert_eq!(result.entry_offset, 0);
        assert_eq!(result.instruction_bytes.len(), 11);
        assert!(result.side_exit_placeholders.is_empty());
        assert_eq!(result.side_exit_return_stubs.len(), 10);

        let prologue = result.callable_prologue.as_ref().unwrap();
        assert_eq!(prologue.start_offset, 0);
        assert_eq!(prologue.bytes, P6_X86_64_CALLABLE_PROLOGUE_BYTES);
        assert_eq!(
            &bytes[prologue.start_offset as usize..prologue.end_offset as usize],
            P6_X86_64_CALLABLE_PROLOGUE_BYTES
        );
        assert_eq!(
            result.instruction_bytes[0].start_offset,
            prologue.end_offset
        );

        assert_eq!(
            result.source_buffer.lifecycle,
            AssemblerBufferLifecycle::FrozenForLink
        );
        assert_eq!(
            result.source_buffer.architecture,
            Some(AssemblerArchitecture::X86_64)
        );
        assert_eq!(result.source_buffer.relocations, Vec::new());
        assert_eq!(result.source_image.descriptor().relocation_count, 0);
        assert_eq!(result.linked_image.relocation_count, 0);
        assert_eq!(result.linked_image.state, LinkBufferState::Linked);
        assert_ne!(
            result.source_buffer.lifecycle,
            AssemblerBufferLifecycle::CopiedToExecutableMemory
        );
    }

    #[test]
    fn p6_callable_normal_path_return_ends_in_callable_epilogue() {
        let result = p6_callable_semantic_emission().unwrap();
        let bytes = result.source_image.bytes();
        let epilogue = result.callable_normal_epilogue.as_ref().unwrap();
        let return_record = result.instruction_bytes.last().unwrap();

        assert_eq!(epilogue.bytes, P6_X86_64_CALLABLE_EPILOGUE_BYTES);
        assert_eq!(
            &bytes[epilogue.start_offset as usize..epilogue.end_offset as usize],
            P6_X86_64_CALLABLE_EPILOGUE_BYTES
        );
        assert_eq!(
            bytes[result.terminal_policy.ret_offset as usize],
            P6_X86_64_CALLABLE_EPILOGUE_BYTES[P6_X86_64_CALLABLE_EPILOGUE_BYTES.len() - 1]
        );
        assert_eq!(result.terminal_policy.ret_offset, epilogue.ret_offset);
        assert_eq!(
            result.terminal_policy.normal_path_end_offset,
            epilogue.end_offset
        );
        assert_eq!(return_record.end_offset, epilogue.end_offset);
        assert_eq!(
            &return_record.bytes
                [return_record.bytes.len() - P6_X86_64_CALLABLE_EPILOGUE_BYTES.len()..],
            P6_X86_64_CALLABLE_EPILOGUE_BYTES
        );
    }

    #[test]
    fn p6_side_exit_return_payloads_use_reserved_tag_and_decode_indices() {
        let layout = static_value_representation_layout();
        let payload_tag = u64::from(P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG);
        let js_value_low_tags = [
            layout.tag_for_immediate_name("undefined").unwrap(),
            layout.tag_for_immediate_name("null").unwrap(),
            layout.tag_for_immediate_name("false").unwrap(),
            layout.tag_for_immediate_name("true").unwrap(),
            layout.tag_for_immediate_name("int32").unwrap(),
            layout.tag_for_immediate_name("double").unwrap(),
            layout.cell_tag,
            layout.double_tag,
        ];

        for tag in js_value_low_tags {
            assert_ne!(payload_tag, tag & layout.tag_mask);
            assert_eq!(P6X86_64BaselineSideExitReturnPayload::decode(tag), None);
        }
        let mut colliding_layout = p6_backend_contract().value_layout;
        colliding_layout.immediate_true_tag = payload_tag;
        assert_eq!(
            validate_p6_x86_64_semantic_value_layout(colliding_layout),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                    field: "side_exit_return_payload_low_tag",
                    actual: payload_tag,
                }
            )
        );

        let sample_indices = [0, 1, 2, 17, u32::MAX];
        let mut raw_payloads = Vec::new();
        for side_exit_index in sample_indices {
            let payload = P6X86_64BaselineSideExitReturnPayload::encode(side_exit_index);
            assert_eq!(payload.side_exit_index(), side_exit_index);
            assert_eq!(
                payload.low_tag(),
                P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG
            );
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(payload.raw_bits()),
                Some(payload)
            );
            assert!(matches!(
                layout.classify_encoded(crate::value::EncodedJsValue(payload.raw_bits())),
                crate::value::ValueClassification::Unknown(encoded)
                    if encoded.0 == payload.raw_bits()
            ));
            assert!(!raw_payloads.contains(&payload.raw_bits()));
            raw_payloads.push(payload.raw_bits());
        }
    }

    #[test]
    fn p9_js_call_native_exit_payloads_use_distinct_reserved_tag_and_decode_indices() {
        let layout = static_value_representation_layout();
        let payload_tag = u64::from(P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG);
        let p6_payload = P6X86_64BaselineSideExitReturnPayload::encode(7);
        let js_value_low_tags = [
            layout.tag_for_immediate_name("undefined").unwrap(),
            layout.tag_for_immediate_name("null").unwrap(),
            layout.tag_for_immediate_name("false").unwrap(),
            layout.tag_for_immediate_name("true").unwrap(),
            layout.tag_for_immediate_name("int32").unwrap(),
            layout.tag_for_immediate_name("double").unwrap(),
            layout.cell_tag,
            layout.double_tag,
        ];

        assert_ne!(
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
            P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_eq!(
            P9X86_64BaselineJsCallNativeExitReturnPayload::decode(p6_payload.raw_bits()),
            None
        );
        for tag in js_value_low_tags {
            assert_ne!(payload_tag, tag & layout.tag_mask);
            assert_eq!(
                P9X86_64BaselineJsCallNativeExitReturnPayload::decode(tag),
                None
            );
        }

        let mut colliding_layout = p6_backend_contract().value_layout;
        colliding_layout.immediate_false_tag = payload_tag;
        assert_eq!(
            validate_p6_x86_64_semantic_value_layout(colliding_layout),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                    field: "js_call_native_exit_payload_low_tag",
                    actual: payload_tag,
                }
            )
        );

        for call_exit_index in [0, 1, 2, 17, u32::MAX] {
            let payload = P9X86_64BaselineJsCallNativeExitReturnPayload::encode(call_exit_index);
            assert_eq!(payload.call_exit_index(), call_exit_index);
            assert_eq!(
                payload.low_tag(),
                P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
            );
            assert_eq!(
                P9X86_64BaselineJsCallNativeExitReturnPayload::decode(payload.raw_bits()),
                Some(payload)
            );
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(payload.raw_bits()),
                None
            );
        }
        assert_eq!(
            P9X86_64BaselineJsCallNativeExitReturnPayload::decode(
                ((u64::from(u32::MAX) + 1) << 8) | payload_tag
            ),
            None
        );
    }

    #[test]
    fn p10_property_native_exit_payloads_use_distinct_reserved_tag_and_decode_indices() {
        let layout = static_value_representation_layout();
        let payload_tag =
            u64::from(P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG);
        let p6_payload = P6X86_64BaselineSideExitReturnPayload::encode(7);
        let p9_payload = P9X86_64BaselineJsCallNativeExitReturnPayload::encode(7);
        let js_value_low_tags = [
            layout.tag_for_immediate_name("undefined").unwrap(),
            layout.tag_for_immediate_name("null").unwrap(),
            layout.tag_for_immediate_name("false").unwrap(),
            layout.tag_for_immediate_name("true").unwrap(),
            layout.tag_for_immediate_name("int32").unwrap(),
            layout.tag_for_immediate_name("double").unwrap(),
            layout.cell_tag,
            layout.double_tag,
        ];

        assert_ne!(
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
            P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_ne!(
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG,
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_eq!(
            P10X86_64BaselinePropertyNativeExitReturnPayload::decode(p6_payload.raw_bits()),
            None
        );
        assert_eq!(
            P10X86_64BaselinePropertyNativeExitReturnPayload::decode(p9_payload.raw_bits()),
            None
        );
        for tag in js_value_low_tags {
            assert_ne!(payload_tag, tag & layout.tag_mask);
            assert_eq!(
                P10X86_64BaselinePropertyNativeExitReturnPayload::decode(tag),
                None
            );
        }

        let mut colliding_layout = p6_backend_contract().value_layout;
        colliding_layout.immediate_null_tag = payload_tag;
        assert_eq!(
            validate_p6_x86_64_semantic_value_layout(colliding_layout),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                    field: "property_native_exit_payload_low_tag",
                    actual: payload_tag,
                }
            )
        );

        for property_exit_index in [0, 1, 2, 17, u32::MAX] {
            let payload =
                P10X86_64BaselinePropertyNativeExitReturnPayload::encode(property_exit_index);
            assert_eq!(payload.property_exit_index(), property_exit_index);
            assert_eq!(
                payload.low_tag(),
                P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
            );
            assert_eq!(
                P10X86_64BaselinePropertyNativeExitReturnPayload::decode(payload.raw_bits()),
                Some(payload)
            );
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(payload.raw_bits()),
                None
            );
            assert_eq!(
                P9X86_64BaselineJsCallNativeExitReturnPayload::decode(payload.raw_bits()),
                None
            );
            assert!(matches!(
                layout.classify_encoded(crate::value::EncodedJsValue(payload.raw_bits())),
                crate::value::ValueClassification::Unknown(encoded)
                    if encoded.0 == payload.raw_bits()
            ));
        }
        assert_eq!(
            P10X86_64BaselinePropertyNativeExitReturnPayload::decode(
                ((u64::from(u32::MAX) + 1) << 8) | payload_tag
            ),
            None
        );
    }

    #[test]
    fn p14_loop_backedge_payloads_use_distinct_reserved_tag_and_decode_indices() {
        let layout = static_value_representation_layout();
        let payload_tag = u64::from(P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG);
        let p6_payload = P6X86_64BaselineSideExitReturnPayload::encode(7);
        let p9_payload = P9X86_64BaselineJsCallNativeExitReturnPayload::encode(7);
        let p10_payload = P10X86_64BaselinePropertyNativeExitReturnPayload::encode(7);
        let p14_payload = P14X86_64BaselineLoopBackedgeReturnPayload::encode(7);
        let js_value_low_tags = [
            layout.tag_for_immediate_name("undefined").unwrap(),
            layout.tag_for_immediate_name("null").unwrap(),
            layout.tag_for_immediate_name("false").unwrap(),
            layout.tag_for_immediate_name("true").unwrap(),
            layout.tag_for_immediate_name("int32").unwrap(),
            layout.tag_for_immediate_name("double").unwrap(),
            layout.cell_tag,
            layout.double_tag,
        ];

        assert_ne!(
            P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG,
            P6_X86_64_BASELINE_SIDE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_ne!(
            P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG,
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_ne!(
            P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG,
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_eq!(
            P14X86_64BaselineLoopBackedgeReturnPayload::decode(p6_payload.raw_bits()),
            None
        );
        assert_eq!(
            P14X86_64BaselineLoopBackedgeReturnPayload::decode(p9_payload.raw_bits()),
            None
        );
        assert_eq!(
            P14X86_64BaselineLoopBackedgeReturnPayload::decode(p10_payload.raw_bits()),
            None
        );
        assert_eq!(
            P6X86_64BaselineSideExitReturnPayload::decode(p14_payload.raw_bits()),
            None
        );
        assert_eq!(
            P9X86_64BaselineJsCallNativeExitReturnPayload::decode(p14_payload.raw_bits()),
            None
        );
        assert_eq!(
            P10X86_64BaselinePropertyNativeExitReturnPayload::decode(p14_payload.raw_bits()),
            None
        );

        for tag in js_value_low_tags {
            assert_ne!(payload_tag, tag & layout.tag_mask);
            assert_eq!(
                P14X86_64BaselineLoopBackedgeReturnPayload::decode(tag),
                None
            );
        }

        let mut colliding_layout = p6_backend_contract().value_layout;
        colliding_layout.immediate_undefined_tag = payload_tag;
        assert_eq!(
            validate_p6_x86_64_semantic_value_layout(colliding_layout),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                    field: "loop_backedge_return_payload_low_tag",
                    actual: payload_tag,
                }
            )
        );

        for backedge_index in [0, 1, 2, 17, u32::MAX] {
            let payload = P14X86_64BaselineLoopBackedgeReturnPayload::encode(backedge_index);
            assert_eq!(payload.backedge_index(), backedge_index);
            assert_eq!(
                payload.low_tag(),
                P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG
            );
            assert_eq!(
                P14X86_64BaselineLoopBackedgeReturnPayload::decode(payload.raw_bits()),
                Some(payload)
            );
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(payload.raw_bits()),
                None
            );
            assert_eq!(
                P9X86_64BaselineJsCallNativeExitReturnPayload::decode(payload.raw_bits()),
                None
            );
            assert_eq!(
                P10X86_64BaselinePropertyNativeExitReturnPayload::decode(payload.raw_bits()),
                None
            );
            assert!(matches!(
                layout.classify_encoded(crate::value::EncodedJsValue(payload.raw_bits())),
                crate::value::ValueClassification::Unknown(encoded)
                    if encoded.0 == payload.raw_bits()
            ));
        }
        assert_eq!(
            P14X86_64BaselineLoopBackedgeReturnPayload::decode(
                ((u64::from(u32::MAX) + 1) << 8) | payload_tag
            ),
            None
        );
    }

    #[test]
    fn p9_callable_emission_retains_exact_call_and_call_with_this_native_exit_metadata() {
        let code_block = p9_call_native_exit_code_block();
        let result = p9_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert_eq!(result.js_call_native_exit_stubs.len(), 2);

        let call = &result.js_call_native_exit_stubs[0];
        assert_eq!(call.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(call.opcode, CoreOpcode::Call);
        assert_eq!(call.destination, local(1));
        assert_eq!(call.callee, argument_including_this(1));
        assert_eq!(call.this_register, None);
        assert_eq!(call.provided_argument_count, 1);
        assert_eq!(call.argument_registers, vec![argument_including_this(3)]);
        assert_eq!(
            call.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(2))
        );
        assert!(call.requires_no_gc_exit_reentry);
        assert!(call.may_throw);
        assert_eq!(call.encoded_payload.call_exit_index(), 0);
        assert_eq!(
            call.encoded_payload.low_tag(),
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );

        let call_with_this = &result.js_call_native_exit_stubs[1];
        assert_eq!(call_with_this.bytecode_index, BytecodeIndex::from_offset(2));
        assert_eq!(call_with_this.opcode, CoreOpcode::CallWithThis);
        assert_eq!(call_with_this.destination, local(2));
        assert_eq!(call_with_this.callee, argument_including_this(1));
        assert_eq!(
            call_with_this.this_register,
            Some(argument_including_this(2))
        );
        assert_eq!(call_with_this.provided_argument_count, 2);
        assert_eq!(
            call_with_this.argument_registers,
            vec![argument_including_this(3), argument_including_this(4)]
        );
        assert_eq!(
            call_with_this.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(3))
        );
        assert!(call_with_this.requires_no_gc_exit_reentry);
        assert!(call_with_this.may_throw);
        assert_eq!(call_with_this.encoded_payload.call_exit_index(), 1);
        assert_eq!(
            call_with_this.encoded_payload.low_tag(),
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_ne!(
            call.encoded_payload.raw_bits(),
            call_with_this.encoded_payload.raw_bits()
        );
    }

    #[test]
    fn p10_callable_emission_retains_exact_get_by_name_property_native_exit_metadata() {
        let code_block = p10_get_by_name_native_exit_code_block();
        let result = p10_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert!(result.js_call_native_exit_stubs.is_empty());
        assert_eq!(result.property_native_exit_stubs.len(), 1);

        let property = &result.property_native_exit_stubs[0];
        assert_eq!(property.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(property.site.owner, owner());
        assert_eq!(property.site.slot, crate::jit::InlineCacheSlotId(0));
        assert_eq!(property.site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(property.site.opcode, CoreOpcode::GetByName);
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::GetByName {
                destination: local(1),
                base: argument_including_this(1),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(11)))
        );
        assert!(property.site.requires_no_gc_exit_reentry);
        assert!(property.site.may_throw);
        assert_eq!(property.encoded_payload.property_exit_index(), 0);
        assert_eq!(
            property.encoded_payload.low_tag(),
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_eq!(
            P6X86_64BaselineSideExitReturnPayload::decode(property.encoded_payload.raw_bits()),
            None
        );
        assert_eq!(
            P9X86_64BaselineJsCallNativeExitReturnPayload::decode(
                property.encoded_payload.raw_bits()
            ),
            None
        );
    }

    #[test]
    fn p10_callable_emission_retains_exact_put_by_name_property_native_exit_metadata() {
        let code_block = p10_put_by_name_native_exit_code_block();
        let result = p10_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert!(result.js_call_native_exit_stubs.is_empty());
        assert_eq!(result.property_native_exit_stubs.len(), 1);

        let property = &result.property_native_exit_stubs[0];
        assert_eq!(property.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(property.site.owner, owner());
        assert_eq!(property.site.slot, crate::jit::InlineCacheSlotId(0));
        assert_eq!(property.site.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(property.site.opcode, CoreOpcode::PutByName);
        assert_eq!(
            property.site.cache_kind,
            crate::jit::InlineCacheKind::PropertyStore
        );
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::PutByName {
                base: argument_including_this(1),
                value: local(0),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(11)))
        );
        assert!(property.site.requires_no_gc_exit_reentry);
        assert!(property.site.may_throw);
        assert_eq!(property.encoded_payload.property_exit_index(), 0);
        assert_eq!(
            property.encoded_payload.low_tag(),
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_eq!(
            P6X86_64BaselineSideExitReturnPayload::decode(property.encoded_payload.raw_bits()),
            None
        );
        assert_eq!(
            P9X86_64BaselineJsCallNativeExitReturnPayload::decode(
                property.encoded_payload.raw_bits()
            ),
            None
        );
    }

    #[test]
    fn p10_callable_emission_retains_mixed_get_and_put_property_native_exits_in_bytecode_order() {
        let code_block = p10_get_by_name_then_put_by_name_native_exit_code_block();
        let result = p10_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert!(result.js_call_native_exit_stubs.is_empty());
        assert_eq!(result.property_native_exit_stubs.len(), 2);

        let get = &result.property_native_exit_stubs[0];
        assert_eq!(get.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(get.site.opcode, CoreOpcode::GetByName);
        assert_eq!(
            get.operands,
            P10X86_64BaselinePropertyNativeExitOperands::GetByName {
                destination: local(1),
                base: argument_including_this(1),
            }
        );
        assert_eq!(get.encoded_payload.property_exit_index(), 0);
        assert_eq!(
            get.encoded_payload.low_tag(),
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );

        let put = &result.property_native_exit_stubs[1];
        assert_eq!(put.bytecode_index, BytecodeIndex::from_offset(2));
        assert_eq!(put.site.opcode, CoreOpcode::PutByName);
        assert_eq!(
            put.site.cache_kind,
            crate::jit::InlineCacheKind::PropertyStore
        );
        assert_eq!(
            put.operands,
            P10X86_64BaselinePropertyNativeExitOperands::PutByName {
                base: argument_including_this(1),
                value: local(0),
            }
        );
        assert_eq!(put.encoded_payload.property_exit_index(), 1);
        assert_eq!(
            put.encoded_payload.low_tag(),
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_ne!(
            get.encoded_payload.raw_bits(),
            put.encoded_payload.raw_bits()
        );
    }

    #[test]
    fn p6_callable_side_exits_branch_to_identity_payload_return_stubs_without_ud2() {
        let result = p6_callable_semantic_emission().unwrap();
        let bytes = result.source_image.bytes();
        let mut raw_payloads = Vec::new();

        assert!(result.side_exit_placeholders.is_empty());
        assert_eq!(result.side_exit_return_stubs.len(), 10);
        for (expected_index, stub) in result.side_exit_return_stubs.iter().enumerate() {
            let expected_index = expected_index as u32;
            let expected_payload = P6X86_64BaselineSideExitReturnPayload::encode(expected_index);
            let expected_stub =
                p6_x86_64_callable_side_exit_return_stub_bytes(expected_payload.raw_bits());

            assert_eq!(stub.side_exit_index, expected_index);
            assert_eq!(stub.encoded_payload, expected_payload);
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(stub.encoded_payload.raw_bits())
                    .map(P6X86_64BaselineSideExitReturnPayload::side_exit_index),
                Some(stub.side_exit_index)
            );
            assert!(!raw_payloads.contains(&stub.encoded_payload.raw_bits()));
            raw_payloads.push(stub.encoded_payload.raw_bits());
            assert!(stub.target_offset >= result.terminal_policy.normal_path_end_offset);
            assert_eq!(stub.bytes, expected_stub);
            assert!(!stub.bytes.windows(2).any(|window| window == [0x0f, 0x0b]));
            assert_eq!(
                &bytes[stub.target_offset as usize..stub.stub_end_offset as usize],
                expected_stub
            );
            assert_eq!(
                rel32_branch_target(bytes, stub.branch_end_offset),
                i64::from(stub.target_offset)
            );
        }
        assert_eq!(raw_payloads.len(), result.side_exit_return_stubs.len());
        for record in &result.instruction_bytes {
            assert_eq!(
                record.bytes,
                bytes[record.start_offset as usize..record.end_offset as usize]
            );
        }
    }

    #[test]
    fn p8a_semantic_branches_patch_rel32_to_normal_instruction_starts_only() {
        let jump_result = p8a_semantic_emission_for_code_block(&p8a_forward_jump_code_block())
            .expect("P8a jump semantic emission");
        let jump_bytes = jump_result.source_image.bytes();
        assert_eq!(
            jump_result.emitter_kind,
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBranchSubset
        );
        assert_eq!(
            jump_result.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::BytecodeBranchesSharedNormalReturnRetThenInlineUd2SideExits
        );
        assert_eq!(jump_result.bytecode_branches.len(), 1);
        let jump = jump_result.bytecode_branches[0];
        let jump_target = jump_result
            .instruction_bytes
            .iter()
            .find(|record| record.bytecode_index == BytecodeIndex::from_offset(4))
            .unwrap();
        assert_eq!(
            jump.kind,
            P6X86_64BaselineBytecodeBranchKind::UnconditionalJump
        );
        assert_eq!(jump.target_bytecode_index, BytecodeIndex::from_offset(4));
        assert_eq!(jump.target_offset, jump_target.start_offset);
        assert_eq!(
            rel32_branch_target(jump_bytes, jump.branch_end_offset),
            i64::from(jump_target.start_offset)
        );
        assert_eq!(jump_result.source_buffer.relocations, Vec::new());
        assert_eq!(jump_result.source_image.descriptor().relocation_count, 0);
        assert_eq!(jump_result.linked_image.relocation_count, 0);
        assert!(jump_result.side_exit_placeholders.is_empty());
        assert!(jump_result.runtime_helper_native_exit_stubs.is_empty());

        let branch_with_side_exit = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![
                    Operand::Register(local(0)),
                    Operand::SignedImmediate(i32::MAX),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(BytecodeIndex::from_offset(4))],
            ),
            typed_instruction(
                2,
                CoreOpcode::AddInt32,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(local(0)),
                    Operand::Register(local(0)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(1))]),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(2)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ]);
        let callable =
            p8a_callable_semantic_emission_for_code_block(&branch_with_side_exit).unwrap();
        let bytes = callable.source_image.bytes();
        assert_eq!(
            callable.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs
        );
        assert_eq!(callable.bytecode_branches.len(), 1);
        assert!(!callable.side_exit_return_stubs.is_empty());
        assert!(callable.runtime_helper_native_exit_stubs.is_empty());
        let branch = callable.bytecode_branches[0];
        let target = callable
            .instruction_bytes
            .iter()
            .find(|record| record.bytecode_index == branch.target_bytecode_index)
            .unwrap();
        assert_eq!(branch.target_offset, target.start_offset);
        assert!(branch.target_offset < callable.terminal_policy.normal_path_end_offset);
        assert_eq!(
            rel32_branch_target(bytes, branch.branch_end_offset),
            i64::from(target.start_offset)
        );
        for stub in &callable.side_exit_return_stubs {
            assert_ne!(branch.target_offset, stub.target_offset);
            assert!(stub.target_offset >= callable.terminal_policy.normal_path_end_offset);
        }
    }

    #[test]
    fn p8a_semantic_jump_if_not_nullish_records_taken_branch_only() {
        let result = p8a_callable_semantic_emission_for_code_block(
            &p8a_jump_if_not_nullish_code_block(CoreOpcode::LoadBool),
        )
        .expect("P8a JumpIfNotNullish semantic emission");
        let bytes = result.source_image.bytes();
        assert_eq!(result.bytecode_branches.len(), 1);
        let branch = result.bytecode_branches[0];
        let target = result
            .instruction_bytes
            .iter()
            .find(|record| record.bytecode_index == BytecodeIndex::from_offset(4))
            .unwrap();
        assert_eq!(
            branch.kind,
            P6X86_64BaselineBytecodeBranchKind::JumpIfNotNullishTaken
        );
        assert_eq!(branch.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(branch.target_offset, target.start_offset);
        assert_eq!(
            rel32_branch_target(bytes, branch.branch_end_offset),
            i64::from(target.start_offset)
        );
        assert!(result.side_exit_return_stubs.is_empty());
        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert_eq!(result.source_buffer.relocations, Vec::new());
        assert_eq!(result.source_image.descriptor().relocation_count, 0);
        assert_eq!(result.linked_image.relocation_count, 0);
    }

    #[test]
    fn p8b_semantic_jump_if_false_records_taken_branch_and_truthiness_side_exit_only() {
        let non_callable =
            p8b_semantic_emission_for_code_block(&p8b_jump_if_false_local_code_block())
                .expect("P8b JumpIfFalse semantic emission");
        assert_eq!(
            non_callable.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBranchTruthinessSubset
        );
        assert_eq!(non_callable.bytecode_branches.len(), 1);
        assert_eq!(
            non_callable.bytecode_branches[0].kind,
            P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
        );
        assert_eq!(non_callable.side_exit_placeholders.len(), 1);
        assert_eq!(
            non_callable.side_exit_placeholders[0].reason,
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        );
        assert_eq!(
            non_callable.side_exit_placeholders[0].bytecode_index,
            BytecodeIndex::from_offset(0)
        );
        assert!(non_callable.side_exit_return_stubs.is_empty());
        assert!(non_callable.runtime_helper_native_exit_stubs.is_empty());

        let result = p8b_callable_semantic_emission_for_code_block(&p8b_jump_if_false_code_block(
            P8bTruthinessSource::Bool(false),
        ))
        .expect("P8b callable JumpIfFalse semantic emission");
        let bytes = result.source_image.bytes();
        assert_eq!(
            result.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs
        );
        assert_eq!(result.bytecode_branches.len(), 1);
        let branch = result.bytecode_branches[0];
        let target = result
            .instruction_bytes
            .iter()
            .find(|record| record.bytecode_index == BytecodeIndex::from_offset(4))
            .unwrap();
        assert_eq!(
            branch.kind,
            P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
        );
        assert_eq!(branch.bytecode_index, BytecodeIndex::from_offset(1));
        assert_eq!(branch.target_offset, target.start_offset);
        assert_eq!(
            rel32_branch_target(bytes, branch.branch_end_offset),
            i64::from(target.start_offset)
        );
        assert_eq!(result.side_exit_return_stubs.len(), 1);
        let side_exit = &result.side_exit_return_stubs[0];
        assert_eq!(
            side_exit.reason,
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        );
        assert_eq!(side_exit.bytecode_index, BytecodeIndex::from_offset(1));
        assert!(side_exit.target_offset >= result.terminal_policy.normal_path_end_offset);
        assert_ne!(side_exit.target_offset, branch.target_offset);
        assert_eq!(
            rel32_branch_target(bytes, side_exit.branch_end_offset),
            i64::from(side_exit.target_offset)
        );
        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert_eq!(result.source_buffer.relocations, Vec::new());
        assert_eq!(result.source_image.descriptor().relocation_count, 0);
        assert_eq!(result.linked_image.relocation_count, 0);
    }

    #[test]
    fn p14_callable_backward_branch_returns_to_retained_loop_safepoint_stub() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
            ),
            typed_instruction(
                1,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(0)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(4)),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
            ),
            typed_instruction(
                3,
                CoreOpcode::Jump,
                vec![Operand::BytecodeIndex(BytecodeIndex::from_offset(1))],
            ),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let proof = p8b_lowering_proof(&code_block);
        let authority = p14_backedge_authority(
            &proof,
            BytecodeIndex::from_offset(3),
            BytecodeIndex::from_offset(1),
            CoreOpcode::Jump,
        );
        let lowering = plan_p6_x86_64_baseline_lowering(
            P6X86_64BaselineLoweringRequest::new_with_backedge_safepoints(
                owner(),
                &code_block,
                proof,
                &[authority],
            ),
        )
        .expect("P14 lowering with exact authority");
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        let result = emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection)
            .expect("P14 callable emission");
        let bytes = result.source_image.bytes();

        assert_eq!(
            result.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs
        );
        assert_eq!(
            result
                .bytecode_branches
                .iter()
                .filter(|branch| branch.bytecode_index == BytecodeIndex::from_offset(3))
                .count(),
            0
        );
        assert_eq!(result.loop_backedge_safepoint_stubs.len(), 1);
        let stub = &result.loop_backedge_safepoint_stubs[0];
        let target = result
            .instruction_bytes
            .iter()
            .find(|record| record.bytecode_index == BytecodeIndex::from_offset(1))
            .unwrap();
        assert_eq!(stub.bytecode_index, BytecodeIndex::from_offset(3));
        assert_eq!(stub.opcode, CoreOpcode::Jump);
        assert_eq!(stub.target_bytecode_index, BytecodeIndex::from_offset(1));
        assert!(stub.target_instruction_offset > target.start_offset);
        assert!(stub.target_instruction_offset < result.terminal_policy.normal_path_end_offset);
        assert_eq!(
            &bytes[stub.target_instruction_offset as usize
                ..stub.target_instruction_offset as usize
                    + P6_X86_64_CALLABLE_PROLOGUE_BYTES.len()],
            P6_X86_64_CALLABLE_PROLOGUE_BYTES
        );
        assert!(stub.safepoint_stub_offset >= result.terminal_policy.normal_path_end_offset);
        assert_eq!(
            rel32_branch_target(bytes, stub.branch_end_offset),
            i64::from(stub.safepoint_stub_offset)
        );
        assert_ne!(
            rel32_branch_target(bytes, stub.branch_end_offset),
            i64::from(stub.target_instruction_offset)
        );
        assert_eq!(
            P14X86_64BaselineLoopBackedgeReturnPayload::decode(stub.encoded_payload.raw_bits())
                .map(P14X86_64BaselineLoopBackedgeReturnPayload::backedge_index),
            Some(stub.backedge_index)
        );
        assert_eq!(
            stub.bytes,
            bytes[stub.safepoint_stub_offset as usize..stub.stub_end_offset as usize]
        );
    }

    #[test]
    fn p6_callable_semantic_emitter_rejects_tampered_contract_and_selection_before_images() {
        let (contract, selection) = p6_backend_contract_and_selection();

        let mut tampered_abi = contract.clone();
        tampered_abi.abi.js_value_return.value = AbiValue::Pointer;
        assert_eq!(
            emit_p6_x86_64_baseline_callable_semantic_bytes(tampered_abi, selection.clone()),
            Err(P6X86_64BaselineSemanticByteEmissionError::Selection {
                error: P6X86_64BaselineInstructionSelectionError::BackendContractMismatch {
                    field: "abi",
                },
            })
        );

        let mut tampered_selection = selection;
        tampered_selection.instructions[0]
            .machine_instructions
            .clear();
        tampered_selection.proof = tampered_selection.expected_proof(&contract);
        assert_eq!(
            emit_p6_x86_64_baseline_callable_semantic_bytes(contract, tampered_selection),
            Err(P6X86_64BaselineSemanticByteEmissionError::Selection {
                error: P6X86_64BaselineInstructionSelectionError::SelectedInstructionMismatch {
                    bytecode_index: BytecodeIndex::from_offset(0),
                },
            })
        );
    }

    #[test]
    fn p6_semantic_validation_covers_disp32_length_and_entry_offset_bounds() {
        assert_eq!(
            p6_x86_64_disp32_for_frame_local(
                BytecodeIndex::from_offset(0),
                P6X86_64BaselineOperandLocation::FrameLocal {
                    local_index: 0,
                    slot_index: 0,
                    byte_offset: i32::MAX as u64,
                },
            ),
            Ok(i32::MAX)
        );
        let too_large = P6X86_64BaselineOperandLocation::FrameLocal {
            local_index: 0,
            slot_index: 0,
            byte_offset: i32::MAX as u64 + 1,
        };
        assert_eq!(
            p6_x86_64_disp32_for_frame_local(BytecodeIndex::from_offset(0), too_large),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    location: too_large,
                    byte_offset: i32::MAX as u64 + 1,
                }
            )
        );
        assert_eq!(p6_x86_64_checked_byte_len(u32::MAX as usize), Ok(u32::MAX));
        assert_eq!(
            p6_x86_64_checked_byte_len(u32::MAX as usize + 1),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: u32::MAX as usize + 1,
                }
            )
        );

        let result = p6_semantic_emission().unwrap();
        assert_eq!(
            validate_p6_x86_64_semantic_byte_images(
                &result.source_buffer,
                &result.source_image,
                &result.linked_image,
                result.source_buffer.byte_len,
                result.source_image.byte_len(),
            ),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::EntryOffsetOutOfRange {
                    entry_offset: result.source_image.byte_len(),
                    image_size_bytes: result.source_image.byte_len(),
                }
            )
        );
    }

    #[test]
    fn p6_lowering_rejects_stale_proof_snapshot() {
        let original = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p6_lowering_proof(&original);
        let changed = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(8)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &changed,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::CodeBlockSnapshotMismatch { owner: owner() })
        );
    }

    #[test]
    fn p6_lowering_rejects_wrong_owner_witness_before_lowering() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(7)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p6_lowering_proof(&code_block);
        let wrong_owner = CodeBlockId(CellId(2));

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                wrong_owner,
                &code_block,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::OwnerWitnessMismatch {
                owner_witness: wrong_owner,
                proof_owner: owner(),
            })
        );
    }

    #[test]
    fn p6_lowering_rejects_unsupported_opcode_subset() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::BitNotInt32,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ]);
        let proof = lowering_proof_for_code_block(
            &code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
        );

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::UnsupportedOpcodeSubset {
                emitter: BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
                expected: BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                actual: BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
            })
        );
    }

    #[test]
    fn p6_lowering_rejects_unsupported_decoded_opcode() {
        let instruction = typed_instruction(
            0,
            CoreOpcode::BitNotInt32,
            vec![Operand::Register(local(0)), Operand::Register(local(1))],
        );
        let stream = PackedInstructionStream::from_typed_placeholder(vec![instruction]);
        let decoded = stream.decoded_at(BytecodeIndex::from_offset(0)).unwrap();

        assert_eq!(
            lower_p6_x86_64_decoded_instruction(decoded),
            Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                bytecode_index: BytecodeIndex::from_offset(0),
                opcode: CoreOpcode::BitNotInt32.opcode(),
                core_opcode: Some(CoreOpcode::BitNotInt32),
            })
        );
    }

    #[test]
    fn p6_lowering_rejects_root_maps_before_snapshot_lowering() {
        let code_block = p6_lowering_code_block_with_root_map();
        let proof = p6_lowering_proof(&code_block);
        let actual =
            P6X86_64BaselineLoweringValidationShape::from_code_block_and_proof(&code_block, &proof);

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::UnsupportedValidationShape {
                emitter: BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
                requirement:
                    P6X86_64BaselineLoweringRequirement::MatchingCodeBlockSnapshotNoRootsNoExceptionHandlers,
                actual,
            })
        );
    }

    #[test]
    fn p6_lowering_rejects_exception_handlers_before_snapshot_lowering() {
        let proof_source = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(1)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p6_lowering_proof(&proof_source);
        let code_block = p6_lowering_code_block_with_handler();
        let actual =
            P6X86_64BaselineLoweringValidationShape::from_code_block_and_proof(&code_block, &proof);

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::UnsupportedValidationShape {
                emitter: BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
                requirement:
                    P6X86_64BaselineLoweringRequirement::MatchingCodeBlockSnapshotNoRootsNoExceptionHandlers,
                actual,
            })
        );
    }

    #[test]
    fn p6_lowering_rejects_operand_shapes_without_guessing() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(7)],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ]);
        let proof = p6_lowering_proof(&code_block);

        assert_eq!(
            plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            )),
            Err(P6X86_64BaselineLoweringError::UnsupportedOperandShape {
                bytecode_index: BytecodeIndex::from_offset(0),
                opcode: CoreOpcode::LoadInt32,
                error: OperandAccessError::UnexpectedOperandKind {
                    opcode: CoreOpcode::LoadInt32.opcode(),
                    index: 1,
                    expected: OperandKind::SignedImmediate,
                    actual: OperandKind::UnsignedImmediate,
                },
            })
        );
    }

    #[test]
    fn p6_lowering_has_no_byte_or_callability_side_effects() {
        let code_block = p6_lowering_code_block();
        let entrypoints_before = *code_block.entrypoints();
        let lifecycle_before = code_block.lifecycle();
        let proof = p6_lowering_proof(&code_block);

        let result = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            result.byte_emission,
            P6X86_64BaselineLoweringByteEmission::NotGenerated
        );
        assert_eq!(
            result.callable_authority,
            P6X86_64BaselineLoweringCallableAuthority::NoCallableAuthority
        );
        assert_eq!(*code_block.entrypoints(), entrypoints_before);
        assert_eq!(code_block.lifecycle(), lifecycle_before);
        assert!(code_block.entrypoints().baseline_jit.is_none());
        assert_eq!(result.plan.operations.len(), 11);
    }

    #[test]
    fn p6_emitter_owned_return_stub_bytes_are_recorded() {
        let result = emit_p6_x86_64_non_callable_return_stub(
            BaselineMachineCodeByteGenerationRequest::new(entry_artifact(), single_return_proof()),
        )
        .unwrap();

        assert_eq!(
            result.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset
        );
        assert_eq!(
            result.byte_shape,
            BaselineMachineCodeByteGenerationShape::P6X86_64NonCallableReturnStub
        );
        assert_eq!(
            result.authority,
            BaselineMachineCodeByteGenerationAuthority::NonExecutableByteProvenanceOnly
        );
        assert_eq!(result.source_buffer.id, AssemblerBufferId(code_id().0));
        assert_eq!(
            result.source_buffer.lifecycle,
            AssemblerBufferLifecycle::FrozenForLink
        );
        assert_eq!(
            result.source_buffer.architecture,
            Some(AssemblerArchitecture::X86_64)
        );
        assert_eq!(result.source_buffer.relocations, Vec::new());
        assert_eq!(result.source_image.id(), AssemblerByteImageId(code_id().0));
        assert_eq!(
            result.source_image.bytes(),
            p6_x86_64_non_callable_return_stub_bytes()
        );
        assert_eq!(
            result.linked_image.bytes(),
            p6_x86_64_non_callable_return_stub_bytes()
        );
        assert_eq!(result.linked_image.profile, LinkBufferProfile::Baseline);

        let record = &result.emission;
        assert_eq!(record.validate(), Ok(()));
        assert_eq!(record.owner, owner());
        assert_eq!(record.code_id, code_id());
        assert_eq!(record.native_code, native_code());
        assert_eq!(record.source_buffer, result.source_buffer.id);
        assert_eq!(record.source_image, result.source_image.id());
        assert_eq!(record.source_digest, result.source_image.digest());
        assert_eq!(record.linked_digest, result.linked_image.output_digest);
        assert_eq!(
            record.linked_size_bytes,
            p6_x86_64_non_callable_return_stub_bytes().len() as u32
        );
        assert_eq!(record.relocation_count, 0);
    }

    #[test]
    fn p6_emitter_has_no_caller_byte_input() {
        let caller_bytes = [0x31, 0xc0, 0x90, 0x90, 0x90, 0xc3];
        assert_ne!(caller_bytes, p6_x86_64_non_callable_return_stub_bytes());

        let result = emit_p6_x86_64_non_callable_return_stub(
            BaselineMachineCodeByteGenerationRequest::new(entry_artifact(), single_return_proof()),
        )
        .unwrap();

        assert_eq!(
            result.source_image.bytes(),
            p6_x86_64_non_callable_return_stub_bytes()
        );
        assert_ne!(result.source_image.bytes(), caller_bytes);
        assert_eq!(result.source_image.bytes(), result.linked_image.bytes());
    }

    #[test]
    fn p6_emitter_rejects_unsupported_proof_shape() {
        let proof = multi_instruction_proof();
        let actual = BaselineMachineCodeByteGenerationProofShape::from_proof(&proof);

        assert_eq!(
            emit_p6_x86_64_non_callable_return_stub(
                BaselineMachineCodeByteGenerationRequest::new(entry_artifact(), proof),
            ),
            Err(BaselineMachineCodeByteGenerationError::UnsupportedProofShape {
                emitter: BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
                requirement: BaselineMachineCodeByteGenerationProofRequirement::SingleInstructionSameIndexNoCheckpointNoRootsNoExceptionHandlers,
                actual,
            })
        );
    }

    #[test]
    fn p6_emitter_rejects_unsupported_subset_before_byte_generation() {
        let proof = single_return_proof_for_subset(
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
        );

        assert_eq!(
            emit_p6_x86_64_non_callable_return_stub(BaselineMachineCodeByteGenerationRequest::new(
                entry_artifact(),
                proof
            ),),
            Err(BaselineMachineCodeByteGenerationError::Emission {
                reason: BaselineMachineCodeEmissionValidationError::UnsupportedOpcodeSubset {
                    emitter: BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
                    expected: BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                    actual:
                        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwise,
                }
            })
        );
    }

    #[test]
    fn p6_emitter_rejects_machine_range_that_does_not_match_owned_bytes() {
        let mut entry_artifact = entry_artifact();
        entry_artifact.machine_code.range.size_bytes += 1;

        assert_eq!(
            emit_p6_x86_64_non_callable_return_stub(BaselineMachineCodeByteGenerationRequest::new(
                entry_artifact,
                single_return_proof(),
            ),),
            Err(BaselineMachineCodeByteGenerationError::Emission {
                reason: BaselineMachineCodeEmissionValidationError::MachineCodeRangeSizeMismatch {
                    expected: p6_x86_64_non_callable_return_stub_bytes().len() as u32,
                    actual: p6_x86_64_non_callable_return_stub_bytes().len() as u32 + 1,
                }
            })
        );
    }
}
