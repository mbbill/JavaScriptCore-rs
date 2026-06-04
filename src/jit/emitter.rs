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
    AssemblerBufferLifecycle, AssemblerByteImage, AssemblerByteImageDigest, AssemblerByteImageId,
    AssemblerDataKind, AssemblerValidationError, LinkBufferProfile, LinkBufferState,
    LinkedAssemblerByteImage,
};
use crate::bytecode::{
    BytecodeIndex, Checkpoint, CodeBlock, CoreOpcode, DecodedInstruction, InstructionDecodeError,
    Opcode, Operand, OperandAccessError, OperandWidth, PropertyCacheKey, RegisterClass,
    RegisterFrameShape, RuntimeSlot, ThisArgumentOffset, ValueProfileBucketKind,
    ValueProfileEmissionPolicy, ValueProfileJitStorageGeneration, VirtualRegister,
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
    baseline_opcode_is_generated_property_handoff, bind_baseline_bytecode_proof_owner,
    validate_baseline_bytecode_proof_code_block_snapshot,
    validate_baseline_generated_property_handoff_site_metadata, BaselineBytecodeEligibilityProof,
    BaselineBytecodeProofBindingError, BaselineBytecodeRange, BaselineBytecodeSnapshotFingerprint,
    BaselineExceptionMetadataPresence, BaselineGeneratedEffectContract,
    BaselineGeneratedJsCallNativeExitPlanMetadata, BaselineGeneratedOwnerContinuationKind,
    BaselineGeneratedOwnerContinuationMapMetadata, BaselineGeneratedPropertyHandoffPlanMetadata,
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
// C++ JSC map: r13 = GPRInfo::jitDataRegister (regCS2) seeded with the baseline
// data-IC record store base, mirroring how baseline code addresses
// `BaselineJITData` via the jitDataRegister (GPRInfo.h:355,371). r12 stays
// GPRInfo::metadataTableRegister (regCS1), untouched by this seed. Both r12 and
// r13 are SysV callee-saved, so they are push/pop balanced across the prologue/
// epilogue and seeding r13 is observationally inert for all existing generated
// code (nothing reads r13 yet; get_by_id emission is a later batch). The IC
// store base arrives as the 4th C-ABI argument in rcx (rdi=vm, rsi=frame_base,
// rdx=callee carrier, rcx=ic_store_base); `mov r13, rcx` mirrors the P9 reentry
// `mov r12, rcx` metadata-table seed. Prologue/epilogue lengths are recomputed
// from these constants everywhere via `.len()`; offset records and side-exit
// rel32 targets are computed from runtime `offset()` values, not a baked
// prologue-length constant, so the added bytes propagate automatically.
const P6_X86_64_CALLABLE_PROLOGUE_BYTES: &[u8] = &[
    0x55, // push rbp
    0x41, 0x57, // push r15
    0x41, 0x54, // push r12
    0x41, 0x55, // push r13
    0x48, 0x89, 0xf5, // mov rbp, rsi
    0x49, 0x89, 0xff, // mov r15, rdi
    0x49, 0x89, 0xd1, // mov r9, rdx (Rust ABI callee JSValue carrier)
    // FOUNDATION-BUG FIX: this was `0x49, 0x89, 0xcb` which decodes as `mov r11, rcx`
    // (ModRM rm=011=r11), NOT `mov r13, rcx`. The IC store base never reached r13, so
    // r13 held caller garbage; this was inert until the get_by_id self-load DataIC
    // became the first reader of r13 (then `[r13+idx*8]` faulted). `mov r13, rcx` is
    // ModRM mod=11 reg=001(rcx) rm=101(r13) = 0xcd under REX.WB (0x49).
    0x49, 0x89, 0xcd, // mov r13, rcx (GPRInfo::jitDataRegister = IC store base)
];
const P6_X86_64_CALLABLE_EPILOGUE_BYTES: &[u8] = &[
    0x41, 0x5d, // pop r13 (reverse of prologue push order: r13, then r12)
    0x41, 0x5c, // pop r12
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
// Cell ABI the resident `get_by_id` self-load DataIC addresses by absolute byte
// layout, mirroring the interpreter's `CoreObjectCell` `#[repr(C)]` header
// (src/interpreter/mod.rs:5455-5571, compile-time `offset_of!` asserted there):
//   - structure id (u32) at byte 0  == JSCell::structureIDOffset() (runtime/JSCell.h:293),
//   - storage pointer (8 bytes) at byte 8 == the JSObject Butterfly-pointer slot
//     analog (runtime/JSObject.h:1572-1577), the base of out_of_line_storage.
// These MUST match the interpreter constants `STRUCTURE_ID_OFFSET` / `STORAGE_PTR_DISP`;
// the resident sidecar `generated_property_load_cell_data_property_at_offset` reads
// the same `[storage_ptr + offset*8]` slot the DataIC loads. A divergence here would
// make the DataIC read the wrong word and SEGFAULT or return a wrong value, so they
// are pinned as named constants at the single codegen site.
pub const P10_X86_64_BASELINE_CELL_STRUCTURE_ID_OFFSET: i32 = 0;
pub const P10_X86_64_BASELINE_CELL_STORAGE_PTR_DISP: i32 = 8;
pub const P14_X86_64_BASELINE_LOOP_BACKEDGE_RETURN_PAYLOAD_LOW_TAG: u8 = 0xfc;
pub const P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_MAX_ARGUMENT_REGISTERS: usize = 16;
const P9_X86_64_BASELINE_OWNER_CALL_RESET_SP_NOOP_BYTE: u8 = 0x90;
const P9_X86_64_BASELINE_OWNER_CALL_RESULT_PROFILE_PENDING_BYTE: u8 = 0x90;
// The P9 owner post-call reentry stub jumps into the owner post-call block,
// which rejoins the normal path and exits through the shared
// `P6_X86_64_CALLABLE_EPILOGUE_BYTES`. That epilogue now `pop r13`s (balancing
// the P6 prologue's added `push r13`), so this reentry prologue MUST also
// `push r13` or the shared epilogue would pop into an unbalanced stack and
// corrupt the return address. The P9 reentry receives its 4th C-ABI arg (rcx) as
// the metadata-table base (seeded into r12 = metadataTableRegister later via
// `mov r12, rcx`), not an IC store base, so r13 is not seeded with a meaningful
// value here; this batch emits no get_by_id, so the reentry path never reads
// r13. Pushing the incoming r13 and popping it in the shared epilogue preserves
// the SysV callee-saved register for the caller and keeps the frame balanced.
const P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES: &[u8] = &[
    0x55, // push rbp
    0x41, 0x57, // push r15
    0x41, 0x54, // push r12
    0x41, 0x55, // push r13 (balances shared epilogue's pop r13; preserved callee-saved)
    0x48, 0x89, 0xf5, // mov rbp, rsi
    0x49, 0x89, 0xff, // mov r15, rdi
    0x4d, 0x89, 0xc1, // mov r9, r8 (Rust ABI callee JSValue carrier)
];
const P9_X86_64_BASELINE_OWNER_CALL_REENTRY_RESULT_SEED_BYTES: &[u8] = &[0x48, 0x89, 0xd0];
const P9_X86_64_BASELINE_OWNER_CALL_REENTRY_METADATA_TABLE_SEED_BYTES: &[u8] = &[0x49, 0x89, 0xcc];
const P9_X86_64_BASELINE_OWNER_CALL_RESULT_PROFILE_STORE_PREFIX: &[u8] = &[0x49, 0x89, 0x84, 0x24];

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
    pub frame_local_count: u32,
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
    LoopHint,
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
    LoadCallee {
        destination: VirtualRegister,
    },
    Move {
        destination: VirtualRegister,
        source: VirtualRegister,
    },
    ToNumberPrimitive {
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
    BitAndInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    BitOrInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    BitXorInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    RightShiftInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    EqualInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    NotEqualInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    EqualNoCallLoose {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    NotEqualNoCallLoose {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    StrictEqualPrimitive {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    StrictNotEqualPrimitive {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    LessThanInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    LessEqualInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    GreaterThanInt32 {
        destination: VirtualRegister,
        left: VirtualRegister,
        right: VirtualRegister,
    },
    GreaterEqualInt32 {
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
    GetGlobalObjectProperty {
        destination: VirtualRegister,
    },
    // op_get_length: `[dest, base, identifier]`, the same shape as GetByName. JSC
    // baseline ICs it through the same GetById machinery (op_get_length lowers to a
    // get_by_id of "length"). This batch keeps it on the slow-path native exit (binds
    // no operand locations), but it still counts as a property-handoff site so the
    // dense `property_site_index` is identical between emit and writeback (FIX 2).
    GetLength {
        destination: VirtualRegister,
        base: VirtualRegister,
    },
    PutByName {
        base: VirtualRegister,
        value: VirtualRegister,
    },
    PutGlobalObjectProperty {
        value: VirtualRegister,
    },
    GetByValue {
        destination: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
    },
    PutByValue {
        base: VirtualRegister,
        property: VirtualRegister,
        value: VirtualRegister,
    },
    InById {
        destination: VirtualRegister,
        base: VirtualRegister,
    },
    InByVal {
        destination: VirtualRegister,
        base: VirtualRegister,
        property: VirtualRegister,
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
    pub frame_local_count: u32,
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
    pub bitwise_exit_policy: Option<P6X86_64BaselineInt32BitwiseExitPolicy>,
    pub equality_exit_policy: Option<P6X86_64BaselineInt32EqualityExitPolicy>,
    pub relational_exit_policy: Option<P6X86_64BaselineInt32RelationalExitPolicy>,
    pub primitive_to_number_exit_policy: Option<P6X86_64BaselinePrimitiveToNumberExitPolicy>,
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
        byte_offset_from_frame_base: u64,
    },
    Constant {
        constant_index: u32,
        read_only: bool,
    },
    CallFrameCalleeValue,
    ReturnCarrier {
        role: RegisterRole,
        value: AbiValue,
    },
    Immediate(P6X86_64BaselineImmediateOperand),
    // C++ JSC InlineAccess uses MacroAssembler::Address(base, displacement) where
    // the base is the cell/storage pointer (PropertyBase) and displacement is a
    // byte offset relative to that pointer: JSCell::structureIDOffset() for the
    // structure guard and offsetRelativeToBase(offset) for the inline-storage
    // load. This location names such a cell-relative addressing form; the disp32
    // is the byte displacement the later opcode-wiring batch will fill from the
    // structure id offset / inline-storage layout. bytecode/InlineAccess.cpp:193,204;
    // runtime/JSObject.h offsetRelativeToBase:1572.
    CellRelative {
        disp32: i32,
    },
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
pub struct P6X86_64BaselineInt32BitwiseExitPolicy {
    pub operation: P6X86_64BaselineInt32BitwiseOperation,
    pub operand_guard: P6X86_64BaselineInt32OperandGuard,
    pub non_int32_exit: P6X86_64BaselineArithmeticSideExitContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineInt32EqualityExitPolicy {
    pub operation: P6X86_64BaselineInt32EqualityOperation,
    pub fast_path: P6X86_64BaselineEqualityFastPath,
    pub operand_guard: P6X86_64BaselineInt32OperandGuard,
    pub non_int32_exit: P6X86_64BaselineArithmeticSideExitContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselineInt32RelationalExitPolicy {
    pub operation: P6X86_64BaselineInt32RelationalOperation,
    pub operand_guard: P6X86_64BaselineInt32OperandGuard,
    pub non_int32_exit: P6X86_64BaselineArithmeticSideExitContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselinePrimitiveToNumberExitPolicy {
    pub unsupported_operand_exit: P6X86_64BaselinePrimitiveToNumberSideExitContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselinePrimitiveToNumberSideExitContract {
    pub reason: P6X86_64BaselinePrimitiveToNumberSideExitReason,
    pub destination: P6X86_64BaselineSideExitDestinationEffect,
    pub retained_bytecode_index: crate::bytecode::BytecodeIndex,
    pub may_throw: bool,
    pub runtime_call: bool,
    pub heap_allocation: bool,
    pub touches_gc_roots: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselinePrimitiveToNumberSideExitReason {
    UnsupportedOperand,
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
pub enum P6X86_64BaselineInt32BitwiseOperation {
    BitAnd,
    BitOr,
    BitXor,
    RightShift,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInt32EqualityOperation {
    Equal,
    NotEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineEqualityFastPath {
    Int32Only,
    GeneratedNoCallLoose,
    PrimitiveStrictNoDouble,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInt32RelationalOperation {
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineInt32OperandGuard {
    GuardBothOperandsWithInt32Tag,
    GuardBothOperandsWithNumberTagsAfterInt32FastPath,
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
    UnsupportedStrictEqualityOperand,
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
    // C++ JSC InlineAccess::generateSelfPropertyAccess uses propertyCache.m_baseGPR
    // (BaselineJITRegisters::GetById::baseJSR == preferredArgumentJSR<...,0>(),
    // i.e. argumentGPR0 == X86Registers::edi / rdi on x86-64) as the cell pointer
    // base for the self-property structure guard + offset-indexed load. This
    // symbolic register names that base; its physical binding is rdi in the
    // register map. See bytecode/InlineAccess.cpp:188-204 and
    // jit/BaselineJITRegisters.h GetById::baseJSR.
    PropertyBase,
    PinnedCalleeValue,
    PinnedCallFrameBase,
    PinnedVm,
    MetadataTableBase,
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
    // C++ JSC InlineAccess::generateSelfPropertyAccess emits a structure guard:
    //   branch32(NotEqual, Address(base, JSCell::structureIDOffset()),
    //            TrustedImm32(structure->id())) -> slow path.
    // We model it as: load32 [base + structure_id_offset] -> scratch; cmp32
    // scratch, imm32(cached_structure_id); jne rel32 -> side exit. The
    // structure_id_offset and cached_structure_id are parameters a later
    // opcode-wiring batch fills from the real cell layout / cached structure.
    // bytecode/InlineAccess.cpp:191-194; runtime/JSCell.h structureIDOffset:236.
    GuardStructureId {
        base: P6X86_64BaselineSymbolicRegister,
        scratch: P6X86_64BaselineSymbolicRegister,
        structure_id_offset: i32,
        cached_structure_id: u32,
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
    Int32OrNumberArithmetic {
        operation: P6X86_64BaselineInt32ArithmeticOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        tag_mask: u64,
        payload_shift: u8,
        int32_tag: u64,
        double_tag: u64,
        non_number_exit: P6X86_64BaselineSideExitLabel,
        overflow_exit: P6X86_64BaselineSideExitLabel,
        negative_zero_exit: Option<P6X86_64BaselineSideExitLabel>,
    },
    Int32Bitwise {
        operation: P6X86_64BaselineInt32BitwiseOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
    },
    Int32EqualityToBoolean {
        operation: P6X86_64BaselineInt32EqualityOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        false_value: u64,
        true_value: u64,
    },
    NoCallLooseEqualityToBoolean {
        operation: P6X86_64BaselineInt32EqualityOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        undefined_tag: u64,
        null_tag: u64,
        false_tag: u64,
        true_tag: u64,
        int32_tag: u64,
        cell_tag: u64,
        unsupported_exit: P6X86_64BaselineSideExitLabel,
        false_value: u64,
        true_value: u64,
    },
    PrimitiveStrictEqualityToBoolean {
        operation: P6X86_64BaselineInt32EqualityOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        undefined_tag: u64,
        null_tag: u64,
        false_tag: u64,
        true_tag: u64,
        int32_tag: u64,
        cell_tag: u64,
        unsupported_exit: P6X86_64BaselineSideExitLabel,
        false_value: u64,
        true_value: u64,
    },
    PrimitiveToNumber {
        value: P6X86_64BaselineSymbolicRegister,
        undefined_tag: u64,
        null_tag: u64,
        false_tag: u64,
        true_tag: u64,
        int32_tag: u64,
        double_tag: u64,
        unsupported_exit: P6X86_64BaselineSideExitLabel,
        int32_zero_value: u64,
        int32_one_value: u64,
        nan_value: u64,
    },
    Int32RelationalToBoolean {
        operation: P6X86_64BaselineInt32RelationalOperation,
        destination: P6X86_64BaselineSymbolicRegister,
        left: P6X86_64BaselineSymbolicRegister,
        right: P6X86_64BaselineSymbolicRegister,
        false_value: u64,
        true_value: u64,
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
    // Resident `get_by_id` self-load DataIC fast path, emitted INLINE just before
    // the slow-path `ReturnPropertyNativeExitPayload` stub for an admitted
    // monomorphic GetByName self-access site.
    //
    // C++ JSC map: this mirrors `InlineAccess::generateSelfPropertyAccess`
    // (bytecode/InlineAccess.cpp:188-204) wired through a baseline `GetByIdSelf`
    // DataIC (jit/JITInlineCacheGenerator.cpp:140-184): structure-guard the base,
    // then load the value at the cached inline self offset. The cached structure
    // id and offset live in the per-CodeBlock `BaselineJITData` record store
    // addressed through r13 (`GPRInfo::jitDataRegister`), at `records[record_index]`
    // = `[r13 + record_index*8]` (structure id at +0, PropertyOffset at +4) -- the
    // Rust analogue of `HandlerPropertyInlineCache` (PropertyInlineCache.h:421-422).
    //
    // Rust ABI specifics the C++ self-access path does not carry: the base frame
    // slot holds a BOXED RuntimeValue = `(CoreObjectCell_ptr << 8) | TAG_CELL`
    // (value/repr.rs:507), so the fast path FIRST guards the base is a cell
    // (low byte == cell_tag; non-cell -> slow path, mirroring a CheckTagEquals)
    // THEN unboxes by `shr 8` to recover the raw cell pointer before reading
    // `[cell + STRUCTURE_ID_OFFSET]` / `[cell + STORAGE_PTR_DISP]`. The byte
    // emitter expands this into the complete fast path plus the slow-path return
    // stub: any guard miss jumps to the slow-path stub (the existing P10 native
    // exit) and a hit stores the value to the destination frame slot and jumps
    // over the slow-path stub, staying resident. The slow path returns the same
    // `encoded_payload` as a standalone `ReturnPropertyNativeExitPayload`, so the
    // runtime native-exit dispatch (keyed on the returned payload, not byte
    // offsets) is unchanged.
    PropertyDataIcSelfLoadGetByNameWithExit {
        record_index: u32,
        base_frame_disp32: i32,
        dest_frame_disp32: i32,
        structure_id_offset: i32,
        storage_ptr_disp: i32,
        cell_tag: u64,
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
    UnsupportedToNumberOperand,
    UnsupportedStrictEqualityOperand,
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
    MissingBitwisePolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnexpectedBitwisePolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
        actual: P6X86_64BaselineInt32BitwiseExitPolicy,
    },
    MissingEqualityPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnexpectedEqualityPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
        actual: P6X86_64BaselineInt32EqualityExitPolicy,
    },
    MissingRelationalPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnexpectedRelationalPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
        actual: P6X86_64BaselineInt32RelationalExitPolicy,
    },
    MissingPrimitiveToNumberPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
    },
    UnexpectedPrimitiveToNumberPolicy {
        bytecode_index: crate::bytecode::BytecodeIndex,
        actual: P6X86_64BaselinePrimitiveToNumberExitPolicy,
    },
    ArithmeticPolicyMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_operation: P6X86_64BaselineInt32ArithmeticOperation,
        expected_checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
        actual: P6X86_64BaselineInt32ArithmeticExitPolicy,
    },
    BitwisePolicyMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_operation: P6X86_64BaselineInt32BitwiseOperation,
        actual: P6X86_64BaselineInt32BitwiseExitPolicy,
    },
    EqualityPolicyMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_operation: P6X86_64BaselineInt32EqualityOperation,
        actual: P6X86_64BaselineInt32EqualityExitPolicy,
    },
    RelationalPolicyMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_operation: P6X86_64BaselineInt32RelationalOperation,
        actual: P6X86_64BaselineInt32RelationalExitPolicy,
    },
    SideExitContractMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_reason: P6X86_64BaselineArithmeticSideExitReason,
        actual: P6X86_64BaselineArithmeticSideExitContract,
    },
    PrimitiveToNumberSideExitContractMismatch {
        bytecode_index: crate::bytecode::BytecodeIndex,
        expected_reason: P6X86_64BaselinePrimitiveToNumberSideExitReason,
        actual: P6X86_64BaselinePrimitiveToNumberSideExitContract,
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
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
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
    pub js_call_owner_post_call_stubs: Vec<P9X86_64BaselineOwnerCallPostCallStubRecord>,
    pub js_call_owner_post_call_reentry_stubs:
        Vec<P9X86_64BaselineOwnerCallPostCallReentryStubRecord>,
    pub property_native_exit_stubs: Vec<P10X86_64BaselinePropertyNativeExitStubRecord>,
    pub source_buffer: AssemblerBufferDescriptor,
    pub source_image: AssemblerByteImage,
    pub linked_image: LinkedAssemblerByteImage,
    pub entry_offset: u32,
    // FIX 3: number of resident get_by_id self-load DataIC fast paths
    // (`PropertyDataIcSelfLoadGetByNameWithExit`) the emitter baked into this
    // baseline image. The install path mirrors this into `VmTieringIntegration` so
    // DataIC residency is measurable. Counted from the accepted selection, the
    // authoritative source of which sites got the inline fast path.
    pub data_ic_self_load_fast_path_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSemanticByteEmissionShape {
    P2aSemanticX86_64FromAcceptedP6Selection,
    P3bCallableCAbiSemanticX86_64FromAcceptedP6Selection,
    P3cCallableCAbiSemanticArm64ReturnSeedFromAcceptedP6Selection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P6X86_64BaselineSemanticByteEmissionAuthority {
    NonExecutableNonCallableSemanticBytesOnly,
    NonExecutableCallableSemanticBytesOnlyNoVmOrPlatformAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6X86_64BaselinePhysicalRegisterMap {
    pub bindings: [P6X86_64BaselinePhysicalRegisterBinding; 9],
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P6BaselineNativeReentryTargetRecord {
    // Metadata-only mirror of C++ JIT bytecode-label/PC publication
    // (fastPathResumePoint/JITCodeMapBuilder); it does not authorize native
    // side-exit reentry without a backend-specific VM/rooting bridge.
    pub resume_bytecode_index: crate::bytecode::BytecodeIndex,
    pub resume_entry_offset: u32,
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
    pub resume_bytecode_index: Option<crate::bytecode::BytecodeIndex>,
    pub resume_entry_offset: Option<u32>,
    pub native_reentry_targets: Vec<P6BaselineNativeReentryTargetRecord>,
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
pub struct P9X86_64BaselineOwnerCallPostCallStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub opcode: CoreOpcode,
    pub destination: VirtualRegister,
    pub resume_bytecode_index: crate::bytecode::BytecodeIndex,
    pub resume_instruction_start_offset: u32,
    pub start_offset: u32,
    pub reset_sp_noop_offset: u32,
    pub reset_sp_noop_end_offset: u32,
    pub result_profile_placeholder_offset: u32,
    pub result_profile_placeholder_end_offset: u32,
    pub result_store_offset: u32,
    pub resume_jump_offset: u32,
    pub resume_jump_end_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P9X86_64BaselineOwnerPostCallReentryResultSeed {
    Unavailable,
    X86_64CAbiThirdArgumentRdxToRax,
    X86_64CAbiThirdArgumentRdxToRaxFourthArgumentRcxToR12,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P9X86_64BaselineOwnerCallPostCallReentryStubRecord {
    pub bytecode_index: crate::bytecode::BytecodeIndex,
    pub opcode: CoreOpcode,
    pub destination: VirtualRegister,
    pub resume_bytecode_index: crate::bytecode::BytecodeIndex,
    pub post_call_target_start_offset: u32,
    pub start_offset: u32,
    pub callable_prologue_offset: u32,
    pub callable_prologue_end_offset: u32,
    pub result_seed: P9X86_64BaselineOwnerPostCallReentryResultSeed,
    pub result_seed_offset: u32,
    pub result_seed_end_offset: u32,
    pub metadata_table_seed_offset: Option<u32>,
    pub metadata_table_seed_end_offset: Option<u32>,
    pub metadata_table_base_address: Option<usize>,
    pub post_call_jump_offset: u32,
    pub post_call_jump_end_offset: u32,
    pub end_offset: u32,
    pub byte_len: u32,
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
pub(crate) struct P6X86_64BaselineOwnerNativeLabelBinding {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) next_bytecode_index: Option<BytecodeIndex>,
    pub(crate) instruction_start_offset: u32,
    pub(crate) instruction_end_offset: u32,
    pub(crate) byte_len: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum P9X86_64BaselineOwnerCallResetSpStatus {
    /// P6 callable code pins the current call-frame base in `rbp` and has no
    /// separate JS stack-pointer register to repair after a native exit.
    P6CallableAbiNoJsStackPointerRegister,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum P9X86_64BaselineOwnerCallResultProfileStatus {
    MetadataPending,
    DisabledByPolicy,
    X86_64MetadataTableRelativeStore64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct P9X86_64BaselineOwnerCallResultProfileBinding {
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
pub(crate) enum P9X86_64BaselineOwnerCallPostCallObligation {
    ResetCallerStackPointer {
        status: P9X86_64BaselineOwnerCallResetSpStatus,
    },
    ProfileCallResult {
        status: P9X86_64BaselineOwnerCallResultProfileStatus,
        binding: Option<P9X86_64BaselineOwnerCallResultProfileBinding>,
    },
    WriteCallResult {
        destination: VirtualRegister,
    },
    NormalizeConstructResultThenWrite {
        destination: VirtualRegister,
    },
    ResumeAtBytecodeLabel {
        resume_bytecode_index: Option<BytecodeIndex>,
        resume_instruction_start_offset: Option<u32>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct P9X86_64BaselineOwnerCallPostCallNativeBlockBinding {
    pub(crate) start_offset: u32,
    pub(crate) end_offset: u32,
    pub(crate) byte_len: u32,
    pub(crate) reset_sp_status: P9X86_64BaselineOwnerCallResetSpStatus,
    pub(crate) reset_sp_offset: u32,
    pub(crate) reset_sp_end_offset: u32,
    pub(crate) result_profile_status: P9X86_64BaselineOwnerCallResultProfileStatus,
    pub(crate) result_profile_binding: Option<P9X86_64BaselineOwnerCallResultProfileBinding>,
    pub(crate) result_profile_offset: u32,
    pub(crate) result_profile_end_offset: u32,
    pub(crate) result_store_offset: u32,
    pub(crate) resume_jump_offset: u32,
    pub(crate) resume_jump_end_offset: u32,
    pub(crate) resume_instruction_start_offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct P9X86_64BaselineOwnerCallPostCallReentryStubBinding {
    pub(crate) start_offset: u32,
    pub(crate) end_offset: u32,
    pub(crate) byte_len: u32,
    pub(crate) callable_prologue_offset: u32,
    pub(crate) callable_prologue_end_offset: u32,
    pub(crate) result_seed: P9X86_64BaselineOwnerPostCallReentryResultSeed,
    pub(crate) result_seed_offset: u32,
    pub(crate) result_seed_end_offset: u32,
    pub(crate) metadata_table_seed_offset: Option<u32>,
    pub(crate) metadata_table_seed_end_offset: Option<u32>,
    pub(crate) metadata_table_base_address: Option<usize>,
    pub(crate) post_call_jump_offset: u32,
    pub(crate) post_call_jump_end_offset: u32,
    pub(crate) post_call_jump_target_start_offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct P9X86_64BaselineOwnerPostCallReturnTargetProof {
    pub(crate) owner: CodeBlockId,
    pub(crate) encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload,
    pub(crate) call_bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) destination: VirtualRegister,
    pub(crate) native_exit_stub_start_offset: u32,
    pub(crate) post_call_target_start_offset: u32,
    pub(crate) post_call_target_end_offset: u32,
    pub(crate) post_call_target_byte_len: u32,
    pub(crate) reset_sp_status: P9X86_64BaselineOwnerCallResetSpStatus,
    pub(crate) reset_sp_offset: u32,
    pub(crate) reset_sp_end_offset: u32,
    pub(crate) result_profile_status: P9X86_64BaselineOwnerCallResultProfileStatus,
    pub(crate) result_profile_binding: Option<P9X86_64BaselineOwnerCallResultProfileBinding>,
    pub(crate) result_profile_offset: u32,
    pub(crate) result_profile_end_offset: u32,
    pub(crate) result_store_offset: u32,
    pub(crate) resume_jump_offset: u32,
    pub(crate) resume_jump_end_offset: u32,
    pub(crate) resume_instruction_start_offset: u32,
    pub(crate) post_call_reentry_stub_start_offset: u32,
    pub(crate) post_call_reentry_stub_end_offset: u32,
    pub(crate) post_call_reentry_stub_byte_len: u32,
    pub(crate) post_call_reentry_callable_prologue_offset: u32,
    pub(crate) post_call_reentry_callable_prologue_end_offset: u32,
    pub(crate) post_call_reentry_result_seed: P9X86_64BaselineOwnerPostCallReentryResultSeed,
    pub(crate) post_call_reentry_result_seed_offset: u32,
    pub(crate) post_call_reentry_result_seed_end_offset: u32,
    pub(crate) post_call_reentry_metadata_table_seed_offset: Option<u32>,
    pub(crate) post_call_reentry_metadata_table_seed_end_offset: Option<u32>,
    pub(crate) post_call_reentry_metadata_table_base_address: Option<usize>,
    pub(crate) post_call_reentry_jump_offset: u32,
    pub(crate) post_call_reentry_jump_end_offset: u32,
    pub(crate) post_call_reentry_jump_target_start_offset: u32,
    pub(crate) linked_size_bytes: u32,
    pub(crate) linked_digest: AssemblerByteImageDigest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct P9X86_64BaselineOwnerCallContinuationNativeBinding {
    pub(crate) owner: CodeBlockId,
    pub(crate) call_bytecode_index: BytecodeIndex,
    pub(crate) opcode: CoreOpcode,
    pub(crate) kind: BaselineGeneratedOwnerContinuationKind,
    pub(crate) destination: VirtualRegister,
    pub(crate) argument_count_including_this: u32,
    pub(crate) resume_bytecode_index: Option<BytecodeIndex>,
    pub(crate) native_exit_stub_start_offset: u32,
    pub(crate) native_exit_stub_end_offset: u32,
    pub(crate) native_exit_stub_byte_len: u32,
    /// Current P9 lowering exits through a payload-return stub. This symbolic
    /// done label is metadata only and must not be treated as a VM resume PC.
    pub(crate) done_label_offset: u32,
    pub(crate) resume_instruction_start_offset: Option<u32>,
    pub(crate) post_call_obligations: Vec<P9X86_64BaselineOwnerCallPostCallObligation>,
    pub(crate) post_call_native_block: Option<P9X86_64BaselineOwnerCallPostCallNativeBlockBinding>,
    pub(crate) post_call_reentry_stub: Option<P9X86_64BaselineOwnerCallPostCallReentryStubBinding>,
    pub(crate) post_call_return_target_proof:
        Option<P9X86_64BaselineOwnerPostCallReturnTargetProof>,
    pub(crate) encoded_payload: P9X86_64BaselineJsCallNativeExitReturnPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct P6X86_64BaselineOwnerContinuationNativeBindingMetadata {
    pub(crate) owner: CodeBlockId,
    pub(crate) bytecode_snapshot: BaselineBytecodeSnapshotFingerprint,
    pub(crate) entry_offset: u32,
    pub(crate) linked_size_bytes: u32,
    pub(crate) linked_digest: AssemblerByteImageDigest,
    pub(crate) labels: Vec<P6X86_64BaselineOwnerNativeLabelBinding>,
    pub(crate) call_continuations: Vec<P9X86_64BaselineOwnerCallContinuationNativeBinding>,
}

#[allow(dead_code)]
impl P6X86_64BaselineOwnerContinuationNativeBindingMetadata {
    pub(crate) fn label_count(&self) -> usize {
        self.labels.len()
    }

    pub(crate) fn call_continuation_count(&self) -> usize {
        self.call_continuations.len()
    }

    pub(crate) fn label_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&P6X86_64BaselineOwnerNativeLabelBinding> {
        self.labels
            .binary_search_by_key(&bytecode_index, |label| label.bytecode_index)
            .ok()
            .and_then(|index| self.labels.get(index))
    }

    pub(crate) fn call_continuation_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&P9X86_64BaselineOwnerCallContinuationNativeBinding> {
        self.call_continuations
            .binary_search_by_key(&bytecode_index, |site| site.call_bytecode_index)
            .ok()
            .and_then(|index| self.call_continuations.get(index))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum P6X86_64BaselineOwnerContinuationNativeBindingError {
    EmptyOwnerContinuationMap,
    SnapshotMismatch {
        owner_map: BaselineBytecodeSnapshotFingerprint,
        emission: BaselineBytecodeSnapshotFingerprint,
    },
    OwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    DuplicateInstructionByteRecord {
        bytecode_index: BytecodeIndex,
    },
    InstructionRangeInvalid {
        bytecode_index: BytecodeIndex,
        start_offset: u32,
        end_offset: u32,
        byte_len: u32,
        linked_size_bytes: u32,
    },
    MissingInstructionByteRecord {
        bytecode_index: BytecodeIndex,
    },
    MissingCallNativeExitStub {
        call_bytecode_index: BytecodeIndex,
    },
    DuplicateCallNativeExitStub {
        call_bytecode_index: BytecodeIndex,
    },
    DuplicateCallPostCallNativeBlock {
        call_bytecode_index: BytecodeIndex,
    },
    DuplicateCallPostCallReentryStub {
        call_bytecode_index: BytecodeIndex,
    },
    CallNativeExitRangeInvalid {
        call_bytecode_index: BytecodeIndex,
        start_offset: u32,
        end_offset: u32,
        byte_len: u32,
        linked_size_bytes: u32,
    },
    CallPostCallNativeBlockRangeInvalid {
        call_bytecode_index: BytecodeIndex,
        start_offset: u32,
        end_offset: u32,
        byte_len: u32,
        linked_size_bytes: u32,
    },
    CallPostCallReentryStubRangeInvalid {
        call_bytecode_index: BytecodeIndex,
        start_offset: u32,
        end_offset: u32,
        byte_len: u32,
        linked_size_bytes: u32,
    },
    CallNativeExitMismatch {
        call_bytecode_index: BytecodeIndex,
        field: &'static str,
    },
    CallPostCallNativeBlockMismatch {
        call_bytecode_index: BytecodeIndex,
        field: &'static str,
    },
    CallPostCallReentryStubMismatch {
        call_bytecode_index: BytecodeIndex,
        field: &'static str,
    },
    ResumeLabelMissing {
        call_bytecode_index: BytecodeIndex,
        resume_bytecode_index: BytecodeIndex,
    },
    DoneLabelConflatesResume {
        call_bytecode_index: BytecodeIndex,
        offset: u32,
    },
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
    UnsupportedArm64SeedLoweredOperation {
        bytecode_index: crate::bytecode::BytecodeIndex,
        operation: P6X86_64BaselineLoweredOperation,
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
    ExpectedCellRelativeMemory,
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
        Self::from_lowering_plan_with_emitter(plan, emitter_kind, AssemblerArchitecture::X86_64)
    }

    pub fn from_lowering_plan_for_arm64_return_seed(
        plan: &P6X86_64BaselineLoweringPlan,
    ) -> Result<Self, P6X86_64BaselineBackendContractError> {
        Self::from_lowering_plan_with_emitter(
            plan,
            BaselineMachineCodeEmitterKind::P6Arm64NoCallNoHeapReturnSeedSubset,
            AssemblerArchitecture::Arm64,
        )
    }

    fn from_lowering_plan_with_emitter(
        plan: &P6X86_64BaselineLoweringPlan,
        emitter_kind: BaselineMachineCodeEmitterKind,
        expected_architecture: AssemblerArchitecture,
    ) -> Result<Self, P6X86_64BaselineBackendContractError> {
        validate_p6_x86_64_backend_contract_subset_and_effect_contract(
            emitter_kind,
            plan.opcode_subset,
            plan.effect_contract,
        )?;

        let architecture = emitter_kind.expected_architecture();
        if architecture != expected_architecture {
            return Err(
                P6X86_64BaselineBackendContractError::UnexpectedArchitecture {
                    expected: expected_architecture,
                    actual: architecture,
                },
            );
        }

        let value_layout = p6_x86_64_baseline_value_layout_contract()?;
        let frame_layout = p6_x86_64_baseline_frame_layout_contract(value_layout);
        let frame_local_count = plan.frame_local_count;
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
                    frame_local_count,
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
            frame_local_count,
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

pub fn record_p6_arm64_baseline_backend_contract_from_plan(
    plan: &P6X86_64BaselineLoweringPlan,
) -> Result<P6X86_64BaselineBackendContractRecord, P6X86_64BaselineBackendContractError> {
    P6X86_64BaselineBackendContractRecord::from_lowering_plan_for_arm64_return_seed(plan)
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
    let frame_local_count = p6_x86_64_frame_local_capacity(request.code_block.unlinked().frame());
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
            frame_local_count,
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
    let frame_local_count = p6_x86_64_frame_local_capacity(request.code_block.unlinked().frame());
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
            frame_local_count,
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
    let data_ic_self_load_fast_path_count = p6_x86_64_data_ic_self_load_fast_path_count(&selection);
    let encoded = encode_p6_x86_64_semantic_selection(&contract, &selection, terminal)?;
    finish_p6_x86_64_semantic_byte_emission(
        &contract,
        encoded,
        P6X86_64BaselineSemanticByteEmissionShape::P2aSemanticX86_64FromAcceptedP6Selection,
        P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableNonCallableSemanticBytesOnly,
        p6_x86_64_semantic_source_buffer_id(&contract),
        p6_x86_64_semantic_source_image_id(&contract),
        0,
        AssemblerArchitecture::X86_64,
        p6_x86_64_semantic_physical_register_map(),
        data_ic_self_load_fast_path_count,
    )
}

pub fn emit_p6_x86_64_baseline_callable_semantic_bytes(
    contract: P6X86_64BaselineBackendContractRecord,
    selection: P6X86_64BaselineInstructionSelectionPlan,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    emit_p6_x86_64_baseline_callable_semantic_bytes_with_owner_continuation_map(
        contract, selection, None,
    )
}

pub(crate) fn emit_p6_x86_64_baseline_callable_semantic_bytes_with_owner_continuation_map(
    contract: P6X86_64BaselineBackendContractRecord,
    selection: P6X86_64BaselineInstructionSelectionPlan,
    owner_continuation_map: Option<&BaselineGeneratedOwnerContinuationMapMetadata>,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    selection
        .validate_against(&contract)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::Selection { error })?;
    validate_p6_x86_64_semantic_selection_effects(&selection)?;
    let terminal = validate_p6_x86_64_semantic_terminal_policy(&selection)?;
    let data_ic_self_load_fast_path_count = p6_x86_64_data_ic_self_load_fast_path_count(&selection);
    let encoded = encode_p6_x86_64_callable_semantic_selection(
        &contract,
        &selection,
        terminal,
        owner_continuation_map,
    )?;
    finish_p6_x86_64_semantic_byte_emission(
        &contract,
        encoded,
        P6X86_64BaselineSemanticByteEmissionShape::P3bCallableCAbiSemanticX86_64FromAcceptedP6Selection,
        P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableCallableSemanticBytesOnlyNoVmOrPlatformAuthority,
        p6_x86_64_callable_semantic_source_buffer_id(&contract),
        p6_x86_64_callable_semantic_source_image_id(&contract),
        0,
        AssemblerArchitecture::X86_64,
        p6_x86_64_semantic_physical_register_map(),
        data_ic_self_load_fast_path_count,
    )
}

// FIX 3: count the resident `PropertyDataIcSelfLoadGetByNameWithExit` fast paths in
// an accepted selection (the authoritative set of sites that got the inline DataIC).
fn p6_x86_64_data_ic_self_load_fast_path_count(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> usize {
    selection
        .instructions
        .iter()
        .flat_map(|instruction| instruction.machine_instructions.iter())
        .filter(|machine_instruction| {
            matches!(
                machine_instruction,
                P6X86_64BaselineMachineInstruction::PropertyDataIcSelfLoadGetByNameWithExit { .. }
            )
        })
        .count()
}

pub(super) fn finish_p6_x86_64_semantic_byte_emission(
    contract: &P6X86_64BaselineBackendContractRecord,
    encoded: P6X86_64SemanticEncodedSelection,
    byte_shape: P6X86_64BaselineSemanticByteEmissionShape,
    authority: P6X86_64BaselineSemanticByteEmissionAuthority,
    source_buffer_id: AssemblerBufferId,
    source_image_id: AssemblerByteImageId,
    entry_offset: u32,
    architecture: AssemblerArchitecture,
    physical_registers: P6X86_64BaselinePhysicalRegisterMap,
    data_ic_self_load_fast_path_count: usize,
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
        js_call_owner_post_call_stubs,
        js_call_owner_post_call_reentry_stubs,
        property_native_exit_stubs,
    } = encoded;
    let byte_len = p6_x86_64_checked_byte_len(bytes.len())?;

    let source_buffer = AssemblerBufferBuilder::new(source_buffer_id)
        .architecture(architecture)
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
        architecture,
        byte_len,
        entry_offset,
    )?;

    Ok(P6X86_64BaselineSemanticByteEmissionResult {
        owner: contract.owner,
        bytecode_snapshot: contract.bytecode_snapshot,
        emitter_kind: contract.emitter_kind,
        byte_shape,
        authority,
        physical_registers,
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
        js_call_owner_post_call_stubs,
        js_call_owner_post_call_reentry_stubs,
        property_native_exit_stubs,
        source_buffer,
        source_image,
        linked_image,
        entry_offset,
        data_ic_self_load_fast_path_count,
    })
}

pub(crate) fn bind_p6_x86_64_owner_continuation_map_to_semantic_emission(
    owner_map: &BaselineGeneratedOwnerContinuationMapMetadata,
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
) -> Result<
    P6X86_64BaselineOwnerContinuationNativeBindingMetadata,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    if owner_map.bytecode_snapshot() != semantic_emission.bytecode_snapshot {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::SnapshotMismatch {
                owner_map: owner_map.bytecode_snapshot(),
                emission: semantic_emission.bytecode_snapshot,
            },
        );
    }

    let owner = owner_map
        .label_at(0)
        .map(|label| label.owner)
        .or_else(|| owner_map.call_site_at(0).map(|site| site.owner))
        .ok_or(P6X86_64BaselineOwnerContinuationNativeBindingError::EmptyOwnerContinuationMap)?;
    if semantic_emission.owner != owner {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::OwnerMismatch {
                expected: owner,
                actual: semantic_emission.owner,
            },
        );
    }

    validate_p6_x86_64_semantic_instruction_byte_records_for_owner_binding(semantic_emission)?;

    let mut labels = Vec::with_capacity(owner_map.label_count());
    for index in 0..owner_map.label_count() {
        let label = owner_map
            .label_at(index)
            .expect("owner continuation label index is in range");
        if label.owner != owner {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::OwnerMismatch {
                    expected: owner,
                    actual: label.owner,
                },
            );
        }
        let instruction = p6_x86_64_unique_instruction_byte_record_for_owner_binding(
            semantic_emission,
            label.bytecode_index,
        )?;
        labels.push(P6X86_64BaselineOwnerNativeLabelBinding {
            owner,
            bytecode_index: label.bytecode_index,
            opcode: label.opcode,
            next_bytecode_index: label.next_bytecode_index,
            instruction_start_offset: instruction.start_offset,
            instruction_end_offset: instruction.end_offset,
            byte_len: instruction.byte_len,
        });
    }

    let mut call_continuations = Vec::with_capacity(owner_map.call_site_count());
    for index in 0..owner_map.call_site_count() {
        let site = owner_map
            .call_site_at(index)
            .expect("owner continuation call-site index is in range");
        if site.owner != owner {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::OwnerMismatch {
                    expected: owner,
                    actual: site.owner,
                },
            );
        }
        if owner_map
            .label_for_bytecode_index(site.call_bytecode_index)
            .is_none()
        {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::MissingInstructionByteRecord {
                    bytecode_index: site.call_bytecode_index,
                },
            );
        }

        let stub = p9_x86_64_unique_js_call_native_exit_stub_for_owner_binding(
            semantic_emission,
            site.call_bytecode_index,
        )?;
        validate_p9_x86_64_js_call_native_exit_stub_for_owner_binding(
            semantic_emission,
            site.call_bytecode_index,
            stub.start_offset,
            stub.end_offset,
            stub.byte_len,
        )?;

        let expected_kind =
            match stub.opcode {
                CoreOpcode::Call | CoreOpcode::CallWithThis => {
                    BaselineGeneratedOwnerContinuationKind::Call
                }
                CoreOpcode::Construct => BaselineGeneratedOwnerContinuationKind::Construct,
                _ => return Err(
                    P6X86_64BaselineOwnerContinuationNativeBindingError::CallNativeExitMismatch {
                        call_bytecode_index: site.call_bytecode_index,
                        field: "opcode",
                    },
                ),
            };
        if stub.opcode != site.opcode
            || expected_kind != site.kind
            || stub.destination != site.destination
            || stub.resume_bytecode_index != site.resume_bytecode_index
            || stub.provided_argument_count.checked_add(1)
                != Some(site.argument_count_including_this)
        {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::CallNativeExitMismatch {
                    call_bytecode_index: site.call_bytecode_index,
                    field: "call_site",
                },
            );
        }

        let resume_instruction_start_offset = match site.resume_bytecode_index {
            Some(resume_bytecode_index) => {
                owner_map
                    .label_for_bytecode_index(resume_bytecode_index)
                    .ok_or(
                        P6X86_64BaselineOwnerContinuationNativeBindingError::ResumeLabelMissing {
                            call_bytecode_index: site.call_bytecode_index,
                            resume_bytecode_index,
                        },
                    )?;
                Some(
                    p6_x86_64_unique_instruction_byte_record_for_owner_binding(
                        semantic_emission,
                        resume_bytecode_index,
                    )?
                    .start_offset,
                )
            }
            None => None,
        };
        if resume_instruction_start_offset == Some(stub.start_offset) {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::DoneLabelConflatesResume {
                    call_bytecode_index: site.call_bytecode_index,
                    offset: stub.start_offset,
                },
            );
        }
        let post_call_native_block = p9_x86_64_unique_owner_call_post_call_stub_for_owner_binding(
            semantic_emission,
            site.call_bytecode_index,
        )?
        .map(|post_call_stub| {
            validate_p9_x86_64_owner_call_post_call_stub_for_owner_binding(
                semantic_emission,
                site,
                post_call_stub,
                resume_instruction_start_offset,
            )
        })
        .transpose()?;
        let post_call_reentry_stub = match post_call_native_block.as_ref() {
            Some(block) => {
                let reentry_stub =
                    p9_x86_64_unique_owner_call_post_call_reentry_stub_for_owner_binding(
                        semantic_emission,
                        site.call_bytecode_index,
                    )?
                    .ok_or(
                        P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                            call_bytecode_index: site.call_bytecode_index,
                            field: "missing",
                        },
                    )?;
                Some(
                    validate_p9_x86_64_owner_call_post_call_reentry_stub_for_owner_binding(
                        semantic_emission,
                        site,
                        reentry_stub,
                        block,
                    )?,
                )
            }
            None => {
                if p9_x86_64_unique_owner_call_post_call_reentry_stub_for_owner_binding(
                    semantic_emission,
                    site.call_bytecode_index,
                )?
                .is_some()
                {
                    return Err(
                        P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                            call_bytecode_index: site.call_bytecode_index,
                            field: "orphan",
                        },
                    );
                }
                None
            }
        };
        let post_call_return_target_proof = match (
            post_call_native_block.as_ref(),
            post_call_reentry_stub.as_ref(),
        ) {
            (Some(block), Some(reentry_stub)) => Some(
                p9_x86_64_owner_post_call_return_target_proof(
                    semantic_emission,
                    owner,
                    site,
                    stub,
                    block,
                    reentry_stub,
                )?,
            ),
            (None, None) => None,
            _ => {
                return Err(
                    P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                        call_bytecode_index: site.call_bytecode_index,
                        field: "return_target",
                    },
                )
            }
        };

        call_continuations.push(P9X86_64BaselineOwnerCallContinuationNativeBinding {
            owner,
            call_bytecode_index: site.call_bytecode_index,
            opcode: site.opcode,
            kind: site.kind,
            destination: site.destination,
            argument_count_including_this: site.argument_count_including_this,
            resume_bytecode_index: site.resume_bytecode_index,
            native_exit_stub_start_offset: stub.start_offset,
            native_exit_stub_end_offset: stub.end_offset,
            native_exit_stub_byte_len: stub.byte_len,
            done_label_offset: stub.start_offset,
            resume_instruction_start_offset,
            post_call_obligations: p9_x86_64_owner_call_post_call_obligations(
                site,
                resume_instruction_start_offset,
                post_call_native_block
                    .as_ref()
                    .map(|block| block.result_profile_status)
                    .unwrap_or(P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending),
            ),
            post_call_native_block,
            post_call_reentry_stub,
            post_call_return_target_proof,
            encoded_payload: stub.encoded_payload,
        });
    }

    Ok(P6X86_64BaselineOwnerContinuationNativeBindingMetadata {
        owner,
        bytecode_snapshot: owner_map.bytecode_snapshot(),
        entry_offset: semantic_emission.entry_offset,
        linked_size_bytes: semantic_emission.linked_image.output_size_bytes,
        linked_digest: semantic_emission.linked_image.output_digest,
        labels,
        call_continuations,
    })
}

fn p9_x86_64_owner_call_post_call_obligations(
    site: &crate::jit::plan::BaselineGeneratedOwnerContinuationSite,
    resume_instruction_start_offset: Option<u32>,
    result_profile_status: P9X86_64BaselineOwnerCallResultProfileStatus,
) -> Vec<P9X86_64BaselineOwnerCallPostCallObligation> {
    let mut obligations = Vec::with_capacity(4);
    obligations.push(
        P9X86_64BaselineOwnerCallPostCallObligation::ResetCallerStackPointer {
            status: P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister,
        },
    );
    obligations.push(
        P9X86_64BaselineOwnerCallPostCallObligation::ProfileCallResult {
            status: result_profile_status,
            binding: p9_x86_64_owner_call_result_profile_binding(site),
        },
    );
    obligations.push(match site.kind {
        BaselineGeneratedOwnerContinuationKind::Call => {
            P9X86_64BaselineOwnerCallPostCallObligation::WriteCallResult {
                destination: site.destination,
            }
        }
        BaselineGeneratedOwnerContinuationKind::Construct => {
            P9X86_64BaselineOwnerCallPostCallObligation::NormalizeConstructResultThenWrite {
                destination: site.destination,
            }
        }
    });
    obligations.push(
        P9X86_64BaselineOwnerCallPostCallObligation::ResumeAtBytecodeLabel {
            resume_bytecode_index: site.resume_bytecode_index,
            resume_instruction_start_offset,
        },
    );
    obligations
}

fn p9_x86_64_owner_call_result_profile_binding(
    site: &crate::jit::plan::BaselineGeneratedOwnerContinuationSite,
) -> Option<P9X86_64BaselineOwnerCallResultProfileBinding> {
    let profile = site.result_profile?;
    Some(P9X86_64BaselineOwnerCallResultProfileBinding {
        profile_slot: profile.profile_slot,
        bytecode_index: profile.bytecode_index,
        checkpoint: profile.checkpoint,
        bucket_kind: profile.bucket_kind,
        storage_generation: profile.storage_generation,
        value_profile_offset: profile.value_profile_offset,
        metadata_table_displacement: profile.metadata_table_displacement,
        metadata_table_base_address: profile.metadata_table_base_address,
        raw_bucket_address: profile.raw_bucket_address,
        raw_bucket_bytes: profile.raw_bucket_bytes,
        emission_policy: profile.emission_policy,
    })
}

fn p9_x86_64_owner_call_result_profile_store_bytes(
    binding: &P9X86_64BaselineOwnerCallResultProfileBinding,
) -> Option<[u8; 8]> {
    if !binding.emission_policy.should_emit {
        return None;
    }
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(P9_X86_64_BASELINE_OWNER_CALL_RESULT_PROFILE_STORE_PREFIX);
    bytes[4..].copy_from_slice(&binding.metadata_table_displacement.to_le_bytes());
    Some(bytes)
}

fn validate_p6_x86_64_semantic_instruction_byte_records_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
) -> Result<(), P6X86_64BaselineOwnerContinuationNativeBindingError> {
    let mut seen = Vec::with_capacity(semantic_emission.instruction_bytes.len());
    for record in &semantic_emission.instruction_bytes {
        if seen.contains(&record.bytecode_index) {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::DuplicateInstructionByteRecord {
                    bytecode_index: record.bytecode_index,
                },
            );
        }
        seen.push(record.bytecode_index);
        validate_p6_x86_64_instruction_byte_record_for_owner_binding(
            record,
            semantic_emission.linked_image.output_size_bytes,
        )?;
    }
    Ok(())
}

fn validate_p6_x86_64_instruction_byte_record_for_owner_binding(
    record: &P6X86_64BaselineInstructionByteRecord,
    linked_size_bytes: u32,
) -> Result<(), P6X86_64BaselineOwnerContinuationNativeBindingError> {
    let valid_range = record.start_offset < record.end_offset
        && record.end_offset <= linked_size_bytes
        && record.end_offset.checked_sub(record.start_offset) == Some(record.byte_len)
        && record.bytes.len() == record.byte_len as usize;
    if !valid_range {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::InstructionRangeInvalid {
                bytecode_index: record.bytecode_index,
                start_offset: record.start_offset,
                end_offset: record.end_offset,
                byte_len: record.byte_len,
                linked_size_bytes,
            },
        );
    }
    Ok(())
}

fn p6_x86_64_unique_instruction_byte_record_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    bytecode_index: BytecodeIndex,
) -> Result<
    &P6X86_64BaselineInstructionByteRecord,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let mut matches = semantic_emission
        .instruction_bytes
        .iter()
        .filter(|record| record.bytecode_index == bytecode_index);
    let Some(record) = matches.next() else {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::MissingInstructionByteRecord {
                bytecode_index,
            },
        );
    };
    if matches.next().is_some() {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::DuplicateInstructionByteRecord {
                bytecode_index,
            },
        );
    }
    Ok(record)
}

fn p9_x86_64_unique_js_call_native_exit_stub_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    call_bytecode_index: BytecodeIndex,
) -> Result<
    &P9X86_64BaselineJsCallNativeExitStubRecord,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let mut matches = semantic_emission
        .js_call_native_exit_stubs
        .iter()
        .filter(|stub| stub.bytecode_index == call_bytecode_index);
    let Some(stub) = matches.next() else {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::MissingCallNativeExitStub {
                call_bytecode_index,
            },
        );
    };
    if matches.next().is_some() {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::DuplicateCallNativeExitStub {
                call_bytecode_index,
            },
        );
    }
    Ok(stub)
}

fn p9_x86_64_unique_owner_call_post_call_stub_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    call_bytecode_index: BytecodeIndex,
) -> Result<
    Option<&P9X86_64BaselineOwnerCallPostCallStubRecord>,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let mut matches = semantic_emission
        .js_call_owner_post_call_stubs
        .iter()
        .filter(|stub| stub.bytecode_index == call_bytecode_index);
    let stub = matches.next();
    if matches.next().is_some() {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::DuplicateCallPostCallNativeBlock {
                call_bytecode_index,
            },
        );
    }
    Ok(stub)
}

fn p9_x86_64_unique_owner_call_post_call_reentry_stub_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    call_bytecode_index: BytecodeIndex,
) -> Result<
    Option<&P9X86_64BaselineOwnerCallPostCallReentryStubRecord>,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let mut matches = semantic_emission
        .js_call_owner_post_call_reentry_stubs
        .iter()
        .filter(|stub| stub.bytecode_index == call_bytecode_index);
    let stub = matches.next();
    if matches.next().is_some() {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::DuplicateCallPostCallReentryStub {
                call_bytecode_index,
            },
        );
    }
    Ok(stub)
}

fn p9_x86_64_rel32_target_from_record_bytes(
    bytes: &[u8],
    record_start_offset: u32,
    branch_end_offset: u32,
) -> Option<u32> {
    let rel32_start = branch_end_offset
        .checked_sub(record_start_offset)?
        .checked_sub(4)? as usize;
    let rel32_end = rel32_start.checked_add(4)?;
    let rel32_bytes = bytes.get(rel32_start..rel32_end)?;
    let displacement = i32::from_le_bytes(rel32_bytes.try_into().ok()?);
    let target = i64::from(branch_end_offset) + i64::from(displacement);
    u32::try_from(target).ok()
}

fn validate_p9_x86_64_js_call_native_exit_stub_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    call_bytecode_index: BytecodeIndex,
    start_offset: u32,
    end_offset: u32,
    byte_len: u32,
) -> Result<(), P6X86_64BaselineOwnerContinuationNativeBindingError> {
    let valid_range = start_offset < end_offset
        && end_offset <= semantic_emission.linked_image.output_size_bytes
        && end_offset.checked_sub(start_offset) == Some(byte_len);
    if !valid_range {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallNativeExitRangeInvalid {
                call_bytecode_index,
                start_offset,
                end_offset,
                byte_len,
                linked_size_bytes: semantic_emission.linked_image.output_size_bytes,
            },
        );
    }
    Ok(())
}

fn validate_p9_x86_64_owner_call_post_call_stub_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    site: &crate::jit::plan::BaselineGeneratedOwnerContinuationSite,
    stub: &P9X86_64BaselineOwnerCallPostCallStubRecord,
    resume_instruction_start_offset: Option<u32>,
) -> Result<
    P9X86_64BaselineOwnerCallPostCallNativeBlockBinding,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let valid_range = stub.start_offset < stub.end_offset
        && stub.end_offset <= semantic_emission.linked_image.output_size_bytes
        && stub.end_offset.checked_sub(stub.start_offset) == Some(stub.byte_len)
        && stub.bytes.len() == stub.byte_len as usize;
    if !valid_range {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockRangeInvalid {
                call_bytecode_index: site.call_bytecode_index,
                start_offset: stub.start_offset,
                end_offset: stub.end_offset,
                byte_len: stub.byte_len,
                linked_size_bytes: semantic_emission.linked_image.output_size_bytes,
            },
        );
    }

    if stub.opcode != site.opcode
        || site.kind != BaselineGeneratedOwnerContinuationKind::Call
        || stub.destination != site.destination
        || Some(stub.resume_bytecode_index) != site.resume_bytecode_index
        || Some(stub.resume_instruction_start_offset) != resume_instruction_start_offset
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "site",
            },
        );
    }
    let result_profile_binding = p9_x86_64_owner_call_result_profile_binding(site).ok_or(
        P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
            call_bytecode_index: site.call_bytecode_index,
            field: "result_profile_binding",
        },
    )?;

    if stub.reset_sp_noop_offset != stub.start_offset
        || stub.reset_sp_noop_end_offset != stub.reset_sp_noop_offset.saturating_add(1)
        || stub.result_profile_placeholder_offset != stub.reset_sp_noop_end_offset
        || stub.result_profile_placeholder_end_offset <= stub.result_profile_placeholder_offset
        || stub.result_store_offset != stub.result_profile_placeholder_end_offset
        || stub.resume_jump_offset <= stub.result_store_offset
        || stub.resume_jump_end_offset > stub.end_offset
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "order",
            },
        );
    }

    let reset_sp_byte_index = stub
        .reset_sp_noop_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "reset_sp_offset",
            },
        )? as usize;
    let result_profile_byte_index = stub
        .result_profile_placeholder_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "result_profile_offset",
            },
        )? as usize;
    let result_profile_end_index = stub
        .result_profile_placeholder_end_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "result_profile_end_offset",
            },
        )? as usize;
    if stub.bytes.get(reset_sp_byte_index)
        != Some(&P9_X86_64_BASELINE_OWNER_CALL_RESET_SP_NOOP_BYTE)
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "reset_sp_byte",
            },
        );
    }
    let profile_bytes = stub
        .bytes
        .get(result_profile_byte_index..result_profile_end_index)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "result_profile_bytes",
            },
        )?;
    let expected_store = p9_x86_64_owner_call_result_profile_store_bytes(&result_profile_binding);
    let result_profile_status =
        if profile_bytes == [P9_X86_64_BASELINE_OWNER_CALL_RESULT_PROFILE_PENDING_BYTE] {
            if result_profile_binding.emission_policy.should_emit {
                P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending
            } else {
                P9X86_64BaselineOwnerCallResultProfileStatus::DisabledByPolicy
            }
        } else if expected_store
            .as_ref()
            .is_some_and(|store| profile_bytes == store)
        {
            P9X86_64BaselineOwnerCallResultProfileStatus::X86_64MetadataTableRelativeStore64
        } else {
            return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "result_profile_bytes",
            },
        );
        };

    Ok(P9X86_64BaselineOwnerCallPostCallNativeBlockBinding {
        start_offset: stub.start_offset,
        end_offset: stub.end_offset,
        byte_len: stub.byte_len,
        reset_sp_status:
            P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister,
        reset_sp_offset: stub.reset_sp_noop_offset,
        reset_sp_end_offset: stub.reset_sp_noop_end_offset,
        result_profile_status,
        result_profile_binding: Some(result_profile_binding),
        result_profile_offset: stub.result_profile_placeholder_offset,
        result_profile_end_offset: stub.result_profile_placeholder_end_offset,
        result_store_offset: stub.result_store_offset,
        resume_jump_offset: stub.resume_jump_offset,
        resume_jump_end_offset: stub.resume_jump_end_offset,
        resume_instruction_start_offset: stub.resume_instruction_start_offset,
    })
}

fn validate_p9_x86_64_owner_call_post_call_reentry_stub_for_owner_binding(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    site: &crate::jit::plan::BaselineGeneratedOwnerContinuationSite,
    stub: &P9X86_64BaselineOwnerCallPostCallReentryStubRecord,
    post_call_block: &P9X86_64BaselineOwnerCallPostCallNativeBlockBinding,
) -> Result<
    P9X86_64BaselineOwnerCallPostCallReentryStubBinding,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let valid_range = stub.start_offset < stub.end_offset
        && stub.end_offset <= semantic_emission.linked_image.output_size_bytes
        && stub.end_offset.checked_sub(stub.start_offset) == Some(stub.byte_len)
        && stub.bytes.len() == stub.byte_len as usize;
    if !valid_range {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubRangeInvalid {
                call_bytecode_index: site.call_bytecode_index,
                start_offset: stub.start_offset,
                end_offset: stub.end_offset,
                byte_len: stub.byte_len,
                linked_size_bytes: semantic_emission.linked_image.output_size_bytes,
            },
        );
    }

    if stub.opcode != site.opcode
        || site.kind != BaselineGeneratedOwnerContinuationKind::Call
        || stub.destination != site.destination
        || Some(stub.resume_bytecode_index) != site.resume_bytecode_index
        || stub.post_call_target_start_offset != post_call_block.start_offset
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "site",
            },
        );
    }

    let metadata_seed_required = post_call_block.result_profile_status
        == P9X86_64BaselineOwnerCallResultProfileStatus::X86_64MetadataTableRelativeStore64;
    let expected_result_seed = if metadata_seed_required {
        P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRaxFourthArgumentRcxToR12
    } else {
        P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRax
    };
    let expected_metadata_base = post_call_block
        .result_profile_binding
        .filter(|_| metadata_seed_required)
        .map(|binding| binding.metadata_table_base_address);
    let metadata_seed_order_valid = if metadata_seed_required {
        stub.metadata_table_seed_offset == Some(stub.result_seed_end_offset)
            && stub
                .metadata_table_seed_end_offset
                .is_some_and(|end| end > stub.result_seed_end_offset)
            && stub.post_call_jump_offset == stub.metadata_table_seed_end_offset.unwrap_or(0)
            && stub.metadata_table_base_address == expected_metadata_base
    } else {
        stub.metadata_table_seed_offset.is_none()
            && stub.metadata_table_seed_end_offset.is_none()
            && stub.metadata_table_base_address.is_none()
            && stub.post_call_jump_offset == stub.result_seed_end_offset
    };

    if stub.callable_prologue_offset != stub.start_offset
        || stub.callable_prologue_end_offset <= stub.callable_prologue_offset
        || stub.result_seed != expected_result_seed
        || stub.result_seed_offset != stub.callable_prologue_end_offset
        || stub.result_seed_end_offset <= stub.result_seed_offset
        || !metadata_seed_order_valid
        || stub.post_call_jump_end_offset > stub.end_offset
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "order",
            },
        );
    }

    let prologue_start = stub
        .callable_prologue_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "callable_prologue_offset",
            },
        )? as usize;
    let prologue_end = stub
        .callable_prologue_end_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "callable_prologue_end_offset",
            },
        )? as usize;
    let seed_start = stub
        .result_seed_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "result_seed_offset",
            },
        )? as usize;
    let seed_end = stub
        .result_seed_end_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "result_seed_end_offset",
            },
        )? as usize;
    let metadata_seed_range = match (
        stub.metadata_table_seed_offset,
        stub.metadata_table_seed_end_offset,
    ) {
        (Some(start), Some(end)) => Some((
            start.checked_sub(stub.start_offset).ok_or(
                P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                    call_bytecode_index: site.call_bytecode_index,
                    field: "metadata_table_seed_offset",
                },
            )? as usize,
            end.checked_sub(stub.start_offset).ok_or(
                P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                    call_bytecode_index: site.call_bytecode_index,
                    field: "metadata_table_seed_end_offset",
                },
            )? as usize,
        )),
        (None, None) => None,
        _ => {
            return Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                    call_bytecode_index: site.call_bytecode_index,
                    field: "metadata_table_seed_range",
                },
            )
        }
    };
    let jump_start = stub
        .post_call_jump_offset
        .checked_sub(stub.start_offset)
        .ok_or(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "post_call_jump_offset",
            },
        )? as usize;

    let metadata_seed_bytes_valid = match metadata_seed_range {
        Some((start, end)) => {
            stub.bytes.get(start..end)
                == Some(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_METADATA_TABLE_SEED_BYTES)
        }
        None => true,
    };

    if stub.bytes.get(prologue_start..prologue_end)
        != Some(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES)
        || stub.bytes.get(seed_start..seed_end)
            != Some(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_RESULT_SEED_BYTES)
        || !metadata_seed_bytes_valid
        || stub.bytes.get(jump_start) != Some(&0xe9)
        || p9_x86_64_rel32_target_from_record_bytes(
            &stub.bytes,
            stub.start_offset,
            stub.post_call_jump_end_offset,
        ) != Some(post_call_block.start_offset)
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallReentryStubMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "operation_bytes",
            },
        );
    }

    Ok(P9X86_64BaselineOwnerCallPostCallReentryStubBinding {
        start_offset: stub.start_offset,
        end_offset: stub.end_offset,
        byte_len: stub.byte_len,
        callable_prologue_offset: stub.callable_prologue_offset,
        callable_prologue_end_offset: stub.callable_prologue_end_offset,
        result_seed: stub.result_seed,
        result_seed_offset: stub.result_seed_offset,
        result_seed_end_offset: stub.result_seed_end_offset,
        metadata_table_seed_offset: stub.metadata_table_seed_offset,
        metadata_table_seed_end_offset: stub.metadata_table_seed_end_offset,
        metadata_table_base_address: stub.metadata_table_base_address,
        post_call_jump_offset: stub.post_call_jump_offset,
        post_call_jump_end_offset: stub.post_call_jump_end_offset,
        post_call_jump_target_start_offset: post_call_block.start_offset,
    })
}

fn p9_x86_64_owner_post_call_return_target_proof(
    semantic_emission: &P6X86_64BaselineSemanticByteEmissionResult,
    owner: CodeBlockId,
    site: &crate::jit::plan::BaselineGeneratedOwnerContinuationSite,
    native_exit_stub: &P9X86_64BaselineJsCallNativeExitStubRecord,
    post_call_block: &P9X86_64BaselineOwnerCallPostCallNativeBlockBinding,
    post_call_reentry_stub: &P9X86_64BaselineOwnerCallPostCallReentryStubBinding,
) -> Result<
    P9X86_64BaselineOwnerPostCallReturnTargetProof,
    P6X86_64BaselineOwnerContinuationNativeBindingError,
> {
    let result_profile_binding = p9_x86_64_owner_call_result_profile_binding(site).ok_or(
        P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
            call_bytecode_index: site.call_bytecode_index,
            field: "result_profile_binding",
        },
    )?;
    let metadata_seed_required = post_call_block.result_profile_status
        == P9X86_64BaselineOwnerCallResultProfileStatus::X86_64MetadataTableRelativeStore64;
    let expected_result_seed = if metadata_seed_required {
        P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRaxFourthArgumentRcxToR12
    } else {
        P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRax
    };
    let metadata_seed_shape_valid = if metadata_seed_required {
        post_call_reentry_stub.metadata_table_seed_offset
            == Some(post_call_reentry_stub.result_seed_end_offset)
            && post_call_reentry_stub
                .metadata_table_seed_end_offset
                .is_some_and(|end| end == post_call_reentry_stub.post_call_jump_offset)
            && post_call_reentry_stub.metadata_table_base_address
                == Some(result_profile_binding.metadata_table_base_address)
    } else {
        post_call_reentry_stub.metadata_table_seed_offset.is_none()
            && post_call_reentry_stub
                .metadata_table_seed_end_offset
                .is_none()
            && post_call_reentry_stub.metadata_table_base_address.is_none()
            && post_call_reentry_stub.result_seed_end_offset
                == post_call_reentry_stub.post_call_jump_offset
    };
    if site.kind != BaselineGeneratedOwnerContinuationKind::Call
        || !matches!(site.opcode, CoreOpcode::Call | CoreOpcode::CallWithThis)
        || native_exit_stub.encoded_payload.low_tag()
            != P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        || post_call_block.start_offset == native_exit_stub.start_offset
        || post_call_block.start_offset == post_call_block.resume_instruction_start_offset
        || post_call_block.reset_sp_status
            != P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister
        || post_call_block.reset_sp_offset != post_call_block.start_offset
        || post_call_block.reset_sp_end_offset != post_call_block.result_profile_offset
        || post_call_block.result_profile_binding != Some(result_profile_binding)
        || post_call_block.result_profile_end_offset != post_call_block.result_store_offset
        || post_call_reentry_stub.start_offset == native_exit_stub.start_offset
        || post_call_reentry_stub.start_offset == post_call_block.start_offset
        || post_call_reentry_stub.start_offset == post_call_block.resume_instruction_start_offset
        || post_call_reentry_stub.callable_prologue_offset != post_call_reentry_stub.start_offset
        || post_call_reentry_stub.result_seed != expected_result_seed
        || post_call_reentry_stub.result_seed_offset
            != post_call_reentry_stub.callable_prologue_end_offset
        || !metadata_seed_shape_valid
        || post_call_reentry_stub.post_call_jump_target_start_offset != post_call_block.start_offset
        || post_call_reentry_stub.end_offset > semantic_emission.linked_image.output_size_bytes
        || post_call_block.end_offset > semantic_emission.linked_image.output_size_bytes
    {
        return Err(
            P6X86_64BaselineOwnerContinuationNativeBindingError::CallPostCallNativeBlockMismatch {
                call_bytecode_index: site.call_bytecode_index,
                field: "return_target",
            },
        );
    }

    Ok(P9X86_64BaselineOwnerPostCallReturnTargetProof {
        owner,
        encoded_payload: native_exit_stub.encoded_payload,
        call_bytecode_index: site.call_bytecode_index,
        opcode: site.opcode,
        destination: site.destination,
        native_exit_stub_start_offset: native_exit_stub.start_offset,
        post_call_target_start_offset: post_call_block.start_offset,
        post_call_target_end_offset: post_call_block.end_offset,
        post_call_target_byte_len: post_call_block.byte_len,
        reset_sp_status: post_call_block.reset_sp_status,
        reset_sp_offset: post_call_block.reset_sp_offset,
        reset_sp_end_offset: post_call_block.reset_sp_end_offset,
        result_profile_status: post_call_block.result_profile_status,
        result_profile_binding: post_call_block.result_profile_binding,
        result_profile_offset: post_call_block.result_profile_offset,
        result_profile_end_offset: post_call_block.result_profile_end_offset,
        result_store_offset: post_call_block.result_store_offset,
        resume_jump_offset: post_call_block.resume_jump_offset,
        resume_jump_end_offset: post_call_block.resume_jump_end_offset,
        resume_instruction_start_offset: post_call_block.resume_instruction_start_offset,
        post_call_reentry_stub_start_offset: post_call_reentry_stub.start_offset,
        post_call_reentry_stub_end_offset: post_call_reentry_stub.end_offset,
        post_call_reentry_stub_byte_len: post_call_reentry_stub.byte_len,
        post_call_reentry_callable_prologue_offset: post_call_reentry_stub.callable_prologue_offset,
        post_call_reentry_callable_prologue_end_offset: post_call_reentry_stub
            .callable_prologue_end_offset,
        post_call_reentry_result_seed: post_call_reentry_stub.result_seed,
        post_call_reentry_result_seed_offset: post_call_reentry_stub.result_seed_offset,
        post_call_reentry_result_seed_end_offset: post_call_reentry_stub.result_seed_end_offset,
        post_call_reentry_metadata_table_seed_offset: post_call_reentry_stub
            .metadata_table_seed_offset,
        post_call_reentry_metadata_table_seed_end_offset: post_call_reentry_stub
            .metadata_table_seed_end_offset,
        post_call_reentry_metadata_table_base_address: post_call_reentry_stub
            .metadata_table_base_address,
        post_call_reentry_jump_offset: post_call_reentry_stub.post_call_jump_offset,
        post_call_reentry_jump_end_offset: post_call_reentry_stub.post_call_jump_end_offset,
        post_call_reentry_jump_target_start_offset: post_call_reentry_stub
            .post_call_jump_target_start_offset,
        linked_size_bytes: semantic_emission.linked_image.output_size_bytes,
        linked_digest: semantic_emission.linked_image.output_digest,
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
pub(super) struct P6X86_64SemanticTerminalSelection {
    return_instruction_indices: Vec<usize>,
    final_return_instruction_index: usize,
    pub(super) return_bytecode_index: crate::bytecode::BytecodeIndex,
    pub(super) branch_aware_returns: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct P6X86_64SemanticEncodedSelection {
    pub(super) bytes: Vec<u8>,
    pub(super) terminal_policy: P6X86_64BaselineTerminalPolicyRecord,
    pub(super) callable_prologue: Option<P6X86_64BaselineCallablePrologueRecord>,
    pub(super) callable_normal_epilogue: Option<P6X86_64BaselineCallableEpilogueRecord>,
    pub(super) instruction_bytes: Vec<P6X86_64BaselineInstructionByteRecord>,
    pub(super) bytecode_branches: Vec<P6X86_64BaselineBytecodeBranchRecord>,
    pub(super) side_exit_placeholders: Vec<P6X86_64BaselineSideExitPlaceholderRecord>,
    pub(super) side_exit_return_stubs: Vec<P6X86_64BaselineSideExitReturnStubRecord>,
    pub(super) loop_backedge_safepoint_stubs: Vec<P14X86_64BaselineLoopBackedgeSafepointStubRecord>,
    pub(super) runtime_helper_native_exit_stubs:
        Vec<P6X86_64BaselineRuntimeHelperNativeExitStubRecord>,
    pub(super) js_call_native_exit_stubs: Vec<P9X86_64BaselineJsCallNativeExitStubRecord>,
    pub(super) js_call_owner_post_call_stubs: Vec<P9X86_64BaselineOwnerCallPostCallStubRecord>,
    pub(super) js_call_owner_post_call_reentry_stubs:
        Vec<P9X86_64BaselineOwnerCallPostCallReentryStubRecord>,
    pub(super) property_native_exit_stubs: Vec<P10X86_64BaselinePropertyNativeExitStubRecord>,
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
struct P6X86_64CallableSideExitResumeTarget {
    side_exit_bytecode_index: crate::bytecode::BytecodeIndex,
    resume_bytecode_index: crate::bytecode::BytecodeIndex,
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

    fn emit_jmp_rel32_side_exit(
        &mut self,
        bytecode_index: crate::bytecode::BytecodeIndex,
        label: P6X86_64BaselineSideExitLabel,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        validate_p6_x86_64_semantic_side_exit_label(bytecode_index, label)?;
        let branch_offset = self.offset()?;
        self.emit(&[0xe9])?;
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

    // `jnz` shares the `jne` encoding (0x0F 0x85); a distinct name documents the
    // "jump if the prior `test`/result was nonzero" intent at the prototype DataIC
    // holder branch (holder_ptr != 0 -> prototype load).
    fn emit_jnz_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x85)
    }

    fn emit_jl_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x8c)
    }

    fn emit_jge_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x8d)
    }

    fn emit_jle_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x8e)
    }

    fn emit_jg_rel32_internal(
        &mut self,
    ) -> Result<P6X86_64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_jcc_rel32_internal(0x8f)
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
        side_exit_resume_targets: &[P6X86_64CallableSideExitResumeTarget],
        callable_reentry_offsets: &[(crate::bytecode::BytecodeIndex, u32)],
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
            let resume_bytecode_index = side_exit_resume_targets
                .iter()
                .find(|target| {
                    target.side_exit_bytecode_index == branch.label.retained_bytecode_index
                })
                .map(|target| target.resume_bytecode_index);
            let resume_entry_offset = resume_bytecode_index
                .map(|resume_bytecode_index| {
                    callable_reentry_offsets
                        .iter()
                        .find(|(bytecode_index, _)| *bytecode_index == resume_bytecode_index)
                        .map(|(_, offset)| *offset)
                        .ok_or(
                            P6X86_64BaselineSemanticByteEmissionError::BranchTargetMissing {
                                bytecode_index: branch.label.retained_bytecode_index,
                                target: resume_bytecode_index,
                            },
                        )
                })
                .transpose()?;
            let native_reentry_targets = match (resume_bytecode_index, resume_entry_offset) {
                (Some(resume_bytecode_index), Some(resume_entry_offset)) => {
                    vec![P6BaselineNativeReentryTargetRecord {
                        resume_bytecode_index,
                        resume_entry_offset,
                    }]
                }
                _ => Vec::new(),
            };
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
                resume_bytecode_index,
                resume_entry_offset,
                native_reentry_targets,
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
            // C++ JSC GetById::baseJSR == argumentGPR0 == X86Registers::edi (rdi).
            // InlineAccess::generateSelfPropertyAccess reads the cell + butterfly
            // through this base register. bytecode/InlineAccess.cpp:188-204.
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PropertyBase,
                physical: "rdi",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCalleeValue,
                physical: "r9",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                physical: "rbp",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedVm,
                physical: "r15",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::MetadataTableBase,
                physical: "r12",
            },
        ],
    }
}

pub(super) fn validate_p6_x86_64_semantic_selection_effects(
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

pub(super) fn validate_p6_x86_64_semantic_terminal_policy(
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

fn p6_x86_64_callable_side_exit_resume_targets(
    selection: &P6X86_64BaselineInstructionSelectionPlan,
) -> Vec<P6X86_64CallableSideExitResumeTarget> {
    let mut targets = Vec::new();
    for (index, instruction) in selection.instructions.iter().enumerate() {
        if !p6_x86_64_selected_instruction_needs_side_exit_reentry(instruction) {
            continue;
        }
        let Some(next_instruction) = selection.instructions.get(index.saturating_add(1)) else {
            continue;
        };
        if targets
            .iter()
            .any(|target: &P6X86_64CallableSideExitResumeTarget| {
                target.side_exit_bytecode_index == instruction.bytecode_index
            })
        {
            continue;
        }
        targets.push(P6X86_64CallableSideExitResumeTarget {
            side_exit_bytecode_index: instruction.bytecode_index,
            resume_bytecode_index: next_instruction.bytecode_index,
        });
    }
    targets
}

fn p6_x86_64_selected_instruction_needs_side_exit_reentry(
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> bool {
    let eligible_opcode = matches!(
        instruction.lowered.operation,
        P6X86_64BaselineLoweredOperation::AddInt32 { .. }
            | P6X86_64BaselineLoweredOperation::SubInt32 { .. }
            | P6X86_64BaselineLoweredOperation::MulInt32 { .. }
            | P6X86_64BaselineLoweredOperation::ToNumberPrimitive { .. }
    );
    eligible_opcode
        && instruction
            .machine_instructions
            .iter()
            .any(p6_x86_64_machine_instruction_has_retained_side_exit)
}

fn p6_x86_64_machine_instruction_has_retained_side_exit(
    instruction: &P6X86_64BaselineMachineInstruction,
) -> bool {
    matches!(
        instruction,
        P6X86_64BaselineMachineInstruction::CheckTagEquals { .. }
            | P6X86_64BaselineMachineInstruction::CheckedInt32Arithmetic { .. }
            | P6X86_64BaselineMachineInstruction::Int32OrNumberArithmetic { .. }
            | P6X86_64BaselineMachineInstruction::PrimitiveToNumber { .. }
            | P6X86_64BaselineMachineInstruction::CheckMulNegativeZero { .. }
    )
}

fn p6_x86_64_callable_reentry_targets(
    loop_backedge_targets: &[crate::bytecode::BytecodeIndex],
    side_exit_resume_targets: &[P6X86_64CallableSideExitResumeTarget],
) -> Vec<crate::bytecode::BytecodeIndex> {
    let mut targets = loop_backedge_targets.to_vec();
    for target in side_exit_resume_targets {
        if !targets.contains(&target.resume_bytecode_index) {
            targets.push(target.resume_bytecode_index);
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
        js_call_owner_post_call_stubs: Vec::new(),
        js_call_owner_post_call_reentry_stubs: Vec::new(),
        property_native_exit_stubs: Vec::new(),
    })
}

fn encode_p6_x86_64_callable_semantic_selection(
    contract: &P6X86_64BaselineBackendContractRecord,
    selection: &P6X86_64BaselineInstructionSelectionPlan,
    terminal: P6X86_64SemanticTerminalSelection,
    owner_continuation_map: Option<&BaselineGeneratedOwnerContinuationMapMetadata>,
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
    let side_exit_resume_targets = p6_x86_64_callable_side_exit_resume_targets(selection);
    let callable_reentry_targets =
        p6_x86_64_callable_reentry_targets(&loop_backedge_targets, &side_exit_resume_targets);
    let mut callable_reentry_offsets = Vec::new();
    for (index, instruction) in selection.instructions.iter().enumerate() {
        let start_offset = builder.offset()?;
        if callable_reentry_targets.contains(&instruction.bytecode_index) {
            let normal_flow_skip = builder.emit_jmp_rel32_internal()?;
            let reentry_offset = builder.offset()?;
            builder.emit_callable_loop_reentry_prologue()?;
            callable_reentry_offsets.push((instruction.bytecode_index, reentry_offset));
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
    let side_exit_return_stubs = builder
        .finish_side_exit_return_stubs(&side_exit_resume_targets, &callable_reentry_offsets)?;
    let loop_backedge_safepoint_stubs = builder
        .finish_loop_backedge_safepoint_stubs(&instruction_bytes, &callable_reentry_offsets)?;
    let (js_call_owner_post_call_stubs, js_call_owner_post_call_reentry_stubs) =
        emit_p9_x86_64_owner_call_post_call_stubs(
            &mut builder,
            contract,
            &instruction_bytes,
            &js_call_native_exit_stubs,
            owner_continuation_map,
        )?;
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
        js_call_owner_post_call_stubs,
        js_call_owner_post_call_reentry_stubs,
        property_native_exit_stubs,
    })
}

fn emit_p9_x86_64_owner_call_post_call_stubs(
    builder: &mut P6X86_64SemanticByteBuilder,
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction_bytes: &[P6X86_64BaselineInstructionByteRecord],
    js_call_native_exit_stubs: &[P9X86_64BaselineJsCallNativeExitStubRecord],
    owner_continuation_map: Option<&BaselineGeneratedOwnerContinuationMapMetadata>,
) -> Result<
    (
        Vec<P9X86_64BaselineOwnerCallPostCallStubRecord>,
        Vec<P9X86_64BaselineOwnerCallPostCallReentryStubRecord>,
    ),
    P6X86_64BaselineSemanticByteEmissionError,
> {
    let mut records = Vec::new();
    let mut reentry_records = Vec::new();
    for stub in js_call_native_exit_stubs {
        if !matches!(stub.opcode, CoreOpcode::Call | CoreOpcode::CallWithThis) {
            continue;
        }
        let Some(resume_bytecode_index) = stub.resume_bytecode_index else {
            continue;
        };
        let Some(destination_disp32) = p9_x86_64_owner_call_destination_disp32(
            contract,
            stub.bytecode_index,
            stub.destination,
        )?
        else {
            continue;
        };
        let resume_instruction_start_offset = instruction_bytes
            .iter()
            .find(|record| record.bytecode_index == resume_bytecode_index)
            .map(|record| record.start_offset)
            .ok_or(
                P6X86_64BaselineSemanticByteEmissionError::BranchTargetMissing {
                    bytecode_index: stub.bytecode_index,
                    target: resume_bytecode_index,
                },
            )?;
        let result_profile_binding = owner_continuation_map
            .and_then(|owner_map| owner_map.call_site_for_bytecode_index(stub.bytecode_index))
            .and_then(p9_x86_64_owner_call_result_profile_binding);
        let result_profile_store = result_profile_binding
            .and_then(|binding| p9_x86_64_owner_call_result_profile_store_bytes(&binding));
        let result_profile_metadata_table_base_address = result_profile_binding
            .filter(|binding| binding.emission_policy.should_emit)
            .map(|binding| binding.metadata_table_base_address);

        let start_offset = builder.offset()?;
        let reset_sp_noop_offset =
            builder.emit_u8(P9_X86_64_BASELINE_OWNER_CALL_RESET_SP_NOOP_BYTE)?;
        let reset_sp_noop_end_offset = builder.offset()?;
        let result_profile_placeholder_offset = builder.offset()?;
        if let Some(store) = result_profile_store {
            builder.emit(&store)?;
        } else {
            builder.emit_u8(P9_X86_64_BASELINE_OWNER_CALL_RESULT_PROFILE_PENDING_BYTE)?;
        }
        let result_profile_placeholder_end_offset = builder.offset()?;
        let result_store_offset = builder.offset()?;
        builder.emit(&[0x48, 0x89, 0x85])?;
        builder.emit_i32_le(destination_disp32)?;
        let resume_jump = builder.emit_jmp_rel32_internal()?;
        builder.patch_internal_branch_to_target(resume_jump, resume_instruction_start_offset)?;
        let end_offset = builder.offset()?;
        records.push(P9X86_64BaselineOwnerCallPostCallStubRecord {
            bytecode_index: stub.bytecode_index,
            opcode: stub.opcode,
            destination: stub.destination,
            resume_bytecode_index,
            resume_instruction_start_offset,
            start_offset,
            reset_sp_noop_offset,
            reset_sp_noop_end_offset,
            result_profile_placeholder_offset,
            result_profile_placeholder_end_offset,
            result_store_offset,
            resume_jump_offset: resume_jump.branch_offset,
            resume_jump_end_offset: resume_jump.branch_end_offset,
            end_offset,
            byte_len: end_offset.checked_sub(start_offset).ok_or(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: builder.bytes.len(),
                },
            )?,
            bytes: builder.bytes_for_range(start_offset, end_offset)?,
        });

        let reentry_start_offset = builder.offset()?;
        let callable_prologue_offset = reentry_start_offset;
        builder.emit(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES)?;
        let callable_prologue_end_offset = builder.offset()?;
        let result_seed_offset = builder.offset()?;
        builder.emit(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_RESULT_SEED_BYTES)?;
        let result_seed_end_offset = builder.offset()?;
        let metadata_table_seed_offset = match result_profile_metadata_table_base_address {
            Some(_) => {
                let offset = builder.offset()?;
                builder.emit(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_METADATA_TABLE_SEED_BYTES)?;
                Some(offset)
            }
            None => None,
        };
        let metadata_table_seed_end_offset = match metadata_table_seed_offset {
            Some(_) => Some(builder.offset()?),
            None => None,
        };
        let post_call_jump = builder.emit_jmp_rel32_internal()?;
        builder.patch_internal_branch_to_target(post_call_jump, start_offset)?;
        let reentry_end_offset = builder.offset()?;
        reentry_records.push(P9X86_64BaselineOwnerCallPostCallReentryStubRecord {
            bytecode_index: stub.bytecode_index,
            opcode: stub.opcode,
            destination: stub.destination,
            resume_bytecode_index,
            post_call_target_start_offset: start_offset,
            start_offset: reentry_start_offset,
            callable_prologue_offset,
            callable_prologue_end_offset,
            result_seed: if result_profile_metadata_table_base_address.is_some() {
                P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRaxFourthArgumentRcxToR12
            } else {
                P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRax
            },
            result_seed_offset,
            result_seed_end_offset,
            metadata_table_seed_offset,
            metadata_table_seed_end_offset,
            metadata_table_base_address: result_profile_metadata_table_base_address,
            post_call_jump_offset: post_call_jump.branch_offset,
            post_call_jump_end_offset: post_call_jump.branch_end_offset,
            end_offset: reentry_end_offset,
            byte_len: reentry_end_offset.checked_sub(reentry_start_offset).ok_or(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: builder.bytes.len(),
                },
            )?,
            bytes: builder.bytes_for_range(reentry_start_offset, reentry_end_offset)?,
        });
    }
    Ok((records, reentry_records))
}

fn p9_x86_64_owner_call_destination_disp32(
    contract: &P6X86_64BaselineBackendContractRecord,
    bytecode_index: BytecodeIndex,
    destination: VirtualRegister,
) -> Result<Option<i32>, P6X86_64BaselineSemanticByteEmissionError> {
    match destination.classify(contract.frame_layout.this_argument_offset) {
        RegisterClass::Local(index) => {
            let byte_offset = u64::from(index)
                .checked_mul(u64::from(contract.frame_layout.local_slot_stride_bytes))
                .ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                        bytecode_index,
                        location: P6X86_64BaselineOperandLocation::FrameLocal {
                            local_index: index,
                            slot_index: index,
                            byte_offset: u64::MAX,
                        },
                        byte_offset: u64::MAX,
                    },
                )?;
            if byte_offset <= i32::MAX as u64 {
                Ok(Some(byte_offset as i32))
            } else {
                Err(
                    P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                        bytecode_index,
                        location: P6X86_64BaselineOperandLocation::FrameLocal {
                            local_index: index,
                            slot_index: index,
                            byte_offset,
                        },
                        byte_offset,
                    },
                )
            }
        }
        // C++ JSC divergence note: baseline JITCall.cpp emitPutCallResult
        // (JITCall.cpp:58-61) stores the call result via
        // emitPutVirtualRegister(destinationFor(...).virtualRegister(), ...),
        // which targets the dst VirtualRegister's stack slot REGARDLESS of
        // whether the bytecode classifies that register as a frame local or as
        // an argument-including-this slot. The owner-post-call-reentry stub must
        // mirror that for both classes: richards keeps hot call results in
        // argument-classified slots (the exact call-path analog of the
        // get_by_id base FrameLocal->FrameLocal+FrameArgument widening in
        // e5ceee2). Compute the frame-base disp32 with the SAME formula proven
        // for every other argument read/write in p6_operand_location
        // (emitter.rs ~7992): frame_local_count*local_slot_stride_bytes +
        // index*value_slot_width_bytes, reusing the identical i32::MAX bound and
        // FrameOffsetOutOfDisp32 error so deep frames keep the faithful slow
        // path. CallFrameHeader/Constant/Invalid stay None: a call result is
        // never legitimately stored to a header or constant slot.
        RegisterClass::ArgumentIncludingThis(index) => {
            let byte_offset = u64::from(contract.frame_local_count)
                .checked_mul(u64::from(contract.frame_layout.local_slot_stride_bytes))
                .and_then(|frame_base| {
                    u64::from(index)
                        .checked_mul(u64::from(contract.frame_layout.value_slot_width_bytes))
                        .and_then(|argument_offset| frame_base.checked_add(argument_offset))
                })
                .ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                        bytecode_index,
                        location: P6X86_64BaselineOperandLocation::FrameArgument {
                            argument_index_including_this: index,
                            raw_virtual_register: destination.raw(),
                            byte_offset_from_argument_base: u64::MAX,
                            byte_offset_from_frame_base: u64::MAX,
                        },
                        byte_offset: u64::MAX,
                    },
                )?;
            if byte_offset <= i32::MAX as u64 {
                Ok(Some(byte_offset as i32))
            } else {
                Err(
                    P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                        bytecode_index,
                        location: P6X86_64BaselineOperandLocation::FrameArgument {
                            argument_index_including_this: index,
                            raw_virtual_register: destination.raw(),
                            byte_offset_from_argument_base: u64::from(index)
                                * u64::from(contract.frame_layout.value_slot_width_bytes),
                            byte_offset_from_frame_base: byte_offset,
                        },
                        byte_offset,
                    },
                )
            }
        }
        _ => Ok(None),
    }
}

fn emit_p6_x86_64_int32_or_number_arithmetic(
    builder: &mut P6X86_64SemanticByteBuilder,
    contract: &P6X86_64BaselineBackendContractRecord,
    bytecode_index: crate::bytecode::BytecodeIndex,
    instruction: P6X86_64BaselineMachineInstruction,
    operation: P6X86_64BaselineInt32ArithmeticOperation,
    tag_mask: u64,
    payload_shift: u8,
    int32_tag: u64,
    double_tag: u64,
    non_number_exit: P6X86_64BaselineSideExitLabel,
    overflow_exit: P6X86_64BaselineSideExitLabel,
    negative_zero_exit: Option<P6X86_64BaselineSideExitLabel>,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    if tag_mask != contract.value_layout.tag_mask || tag_mask != 0xff {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "tag_mask",
                actual: tag_mask,
            },
        );
    }
    if payload_shift != contract.value_layout.payload_shift || payload_shift != 8 {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "payload_shift",
                actual: u64::from(payload_shift),
            },
        );
    }
    if int32_tag != contract.value_layout.immediate_int32_tag {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "int32_tag",
                actual: int32_tag,
            },
        );
    }
    if double_tag != contract.value_layout.immediate_double_tag
        || double_tag != contract.value_layout.double_tag
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedValueLayout {
                field: "double_tag",
                actual: double_tag,
            },
        );
    }
    if (operation == P6X86_64BaselineInt32ArithmeticOperation::Mul) != negative_zero_exit.is_some()
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        );
    }

    let int32_tag = p6_x86_64_semantic_u8_tag(bytecode_index, int32_tag)?;
    let double_tag = p6_x86_64_semantic_u8_tag(bytecode_index, double_tag)?;

    builder.emit(&[0x41, 0x80, 0xfa, int32_tag])?;
    let left_not_int32 = builder.emit_jne_rel32_internal()?;
    builder.emit(&[0x41, 0x80, 0xfb, int32_tag])?;
    let right_not_int32 = builder.emit_jne_rel32_internal()?;

    builder.emit(&[0x49, 0xc1, 0xea, 0x08])?;
    builder.emit(&[0x49, 0xc1, 0xeb, 0x08])?;
    builder.emit(&[0x44, 0x89, 0xd1])?;
    match operation {
        P6X86_64BaselineInt32ArithmeticOperation::Add => builder.emit(&[0x44, 0x01, 0xd9])?,
        P6X86_64BaselineInt32ArithmeticOperation::Sub => builder.emit(&[0x44, 0x29, 0xd9])?,
        P6X86_64BaselineInt32ArithmeticOperation::Mul => builder.emit(&[0x41, 0x0f, 0xaf, 0xcb])?,
    }
    builder.emit_jcc_rel32_side_exit(bytecode_index, 0x80, overflow_exit)?;

    if let Some(on_negative_zero) = negative_zero_exit {
        builder.emit(&[0x85, 0xc9])?;
        let product_non_zero = builder.emit_jne_rel32_internal()?;
        builder.emit(&[0x45, 0x31, 0xda])?;
        builder.emit(&[0x45, 0x85, 0xd2])?;
        builder.emit_jcc_rel32_side_exit(bytecode_index, 0x88, on_negative_zero)?;
        builder.patch_internal_branch_to_current(product_non_zero)?;
    }

    builder.emit(&[0x41, 0x89, 0xca])?;
    builder.emit(&[0x49, 0xc1, 0xe2, 0x08])?;
    builder.emit(&[0x41, 0x80, 0xca, int32_tag])?;
    let done = builder.emit_jmp_rel32_internal()?;

    builder.patch_internal_branch_to_current(left_not_int32)?;
    builder.patch_internal_branch_to_current(right_not_int32)?;

    emit_p6_x86_64_number_operand_to_xmm0(
        builder,
        bytecode_index,
        int32_tag,
        double_tag,
        non_number_exit,
    )?;
    emit_p6_x86_64_number_operand_to_xmm1(
        builder,
        bytecode_index,
        int32_tag,
        double_tag,
        non_number_exit,
    )?;

    match operation {
        P6X86_64BaselineInt32ArithmeticOperation::Add => builder.emit(&[0xf2, 0x0f, 0x58, 0xc1])?,
        P6X86_64BaselineInt32ArithmeticOperation::Sub => builder.emit(&[0xf2, 0x0f, 0x5c, 0xc1])?,
        P6X86_64BaselineInt32ArithmeticOperation::Mul => builder.emit(&[0xf2, 0x0f, 0x59, 0xc1])?,
    }
    builder.emit(&[0x66, 0x49, 0x0f, 0x7e, 0xc2])?;
    builder.emit(&[0x49, 0x81, 0xe2, 0x00, 0xff, 0xff, 0xff])?;
    builder.emit(&[0x41, 0x80, 0xca, double_tag])?;

    builder.patch_internal_branch_to_current(done)
}

fn emit_p6_x86_64_number_operand_to_xmm0(
    builder: &mut P6X86_64SemanticByteBuilder,
    bytecode_index: crate::bytecode::BytecodeIndex,
    int32_tag: u8,
    double_tag: u8,
    non_number_exit: P6X86_64BaselineSideExitLabel,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    builder.emit(&[0x41, 0x80, 0xfa, int32_tag])?;
    let is_int32 = builder.emit_je_rel32_internal()?;
    builder.emit(&[0x41, 0x80, 0xfa, double_tag])?;
    builder.emit_jcc_rel32_side_exit(bytecode_index, 0x85, non_number_exit)?;
    builder.emit(&[0x49, 0x81, 0xe2, 0x00, 0xff, 0xff, 0xff])?;
    builder.emit(&[0x66, 0x49, 0x0f, 0x6e, 0xc2])?;
    let done = builder.emit_jmp_rel32_internal()?;

    builder.patch_internal_branch_to_current(is_int32)?;
    builder.emit(&[0x49, 0xc1, 0xea, 0x08])?;
    builder.emit(&[0xf2, 0x41, 0x0f, 0x2a, 0xc2])?;

    builder.patch_internal_branch_to_current(done)
}

fn emit_p6_x86_64_number_operand_to_xmm1(
    builder: &mut P6X86_64SemanticByteBuilder,
    bytecode_index: crate::bytecode::BytecodeIndex,
    int32_tag: u8,
    double_tag: u8,
    non_number_exit: P6X86_64BaselineSideExitLabel,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    builder.emit(&[0x41, 0x80, 0xfb, int32_tag])?;
    let is_int32 = builder.emit_je_rel32_internal()?;
    builder.emit(&[0x41, 0x80, 0xfb, double_tag])?;
    builder.emit_jcc_rel32_side_exit(bytecode_index, 0x85, non_number_exit)?;
    builder.emit(&[0x49, 0x81, 0xe3, 0x00, 0xff, 0xff, 0xff])?;
    builder.emit(&[0x66, 0x49, 0x0f, 0x6e, 0xcb])?;
    let done = builder.emit_jmp_rel32_internal()?;

    builder.patch_internal_branch_to_current(is_int32)?;
    builder.emit(&[0x49, 0xc1, 0xeb, 0x08])?;
    builder.emit(&[0xf2, 0x41, 0x0f, 0x2a, 0xcb])?;

    builder.patch_internal_branch_to_current(done)
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
        P6X86_64BaselineMachineInstruction::MoveQ {
            destination:
                P6X86_64BaselineMachineOperand::Register(P6X86_64BaselineSymbolicRegister::Scratch0),
            source:
                P6X86_64BaselineMachineOperand::Register(
                    P6X86_64BaselineSymbolicRegister::PinnedCalleeValue,
                ),
        } => builder.emit(&[0x4d, 0x8b, 0xd1]),
        P6X86_64BaselineMachineInstruction::MoveQ { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        // C++ JSC InlineAccess self-property load:
        //   loadValue(Address(storage /* base == PropertyBase/rdi */,
        //                     offsetRelativeToBase(offset)), value).
        // For an inline-storage property the storage register is the base cell
        // pointer, so this is a single quadword mov reg64, [rdi + disp32].
        // Encoded REX.W 8B /r with ModRM mod=10 (disp32), rm=111 (rdi).
        // bytecode/InlineAccess.cpp:196-204.
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination,
            source:
                P6X86_64BaselineMachineOperand::Memory(
                    memory @ P6X86_64BaselineMachineMemoryOperand {
                        base: P6X86_64BaselineSymbolicRegister::PropertyBase,
                        ..
                    },
                ),
        } => {
            let disp32 = p6_x86_64_semantic_cell_relative_disp32(bytecode_index, memory)?;
            match destination {
                // mov rax, [rdi+disp32]: REX.W 8B, ModRM 10 000 111 = 0x87.
                P6X86_64BaselineSymbolicRegister::ReturnGpr => {
                    builder.emit(&[0x48, 0x8b, 0x87])?;
                    builder.emit_i32_le(disp32)
                }
                // mov r10, [rdi+disp32]: REX.WR 8B, ModRM 10 010 111 = 0x97.
                P6X86_64BaselineSymbolicRegister::Scratch0 => {
                    builder.emit(&[0x4c, 0x8b, 0x97])?;
                    builder.emit_i32_le(disp32)
                }
                // mov r11, [rdi+disp32]: REX.WR 8B, ModRM 10 011 111 = 0x9F.
                P6X86_64BaselineSymbolicRegister::Scratch1 => {
                    builder.emit(&[0x4c, 0x8b, 0x9f])?;
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
        // C++ JSC writes a self-property via storeValue(value, Address(storage,
        // offsetRelativeToBase(offset))); the inline-storage store mirrors the
        // load: mov [rdi + disp32], reg64. Encoded REX.W 89 /r ModRM mod=10
        // rm=111. bytecode/InlineAccess.cpp (put-by-id self property path).
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination:
                P6X86_64BaselineMachineOperand::Memory(
                    memory @ P6X86_64BaselineMachineMemoryOperand {
                        base: P6X86_64BaselineSymbolicRegister::PropertyBase,
                        ..
                    },
                ),
            source,
        } => {
            let disp32 = p6_x86_64_semantic_cell_relative_disp32(bytecode_index, memory)?;
            match source {
                // mov [rdi+disp32], rax: REX.W 89, ModRM 10 000 111 = 0x87.
                P6X86_64BaselineSymbolicRegister::ReturnGpr => {
                    builder.emit(&[0x48, 0x89, 0x87])?;
                    builder.emit_i32_le(disp32)
                }
                // mov [rdi+disp32], r10: REX.WR 89, ModRM 10 010 111 = 0x97.
                P6X86_64BaselineSymbolicRegister::Scratch0 => {
                    builder.emit(&[0x4c, 0x89, 0x97])?;
                    builder.emit_i32_le(disp32)
                }
                // mov [rdi+disp32], r11: REX.WR 89, ModRM 10 011 111 = 0x9F.
                P6X86_64BaselineSymbolicRegister::Scratch1 => {
                    builder.emit(&[0x4c, 0x89, 0x9f])?;
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
        // C++ JSC InlineAccess::generateSelfPropertyAccess structure guard:
        //   branch32(NotEqual, Address(base, JSCell::structureIDOffset()),
        //            TrustedImm32(structure->id())) -> slow path.
        // Modeled as load32 [base+structure_id_offset] -> scratch; cmp32 scratch,
        // imm32(cached_structure_id); jne rel32 -> side exit. base must be
        // PropertyBase (rdi); scratch is r10/r11. bytecode/InlineAccess.cpp:191-194.
        P6X86_64BaselineMachineInstruction::GuardStructureId {
            base,
            scratch,
            structure_id_offset,
            cached_structure_id,
            on_not_equal,
        } => {
            if base != P6X86_64BaselineSymbolicRegister::PropertyBase {
                return Err(
                    P6X86_64BaselineSemanticByteEmissionError::UnsupportedMemoryBase {
                        bytecode_index,
                        base,
                    },
                );
            }
            match scratch {
                // load32: mov r10d, [rdi+disp32] = REX.R 8B, ModRM 10 010 111 = 0x97.
                // cmp32:  cmp r10d, imm32 = REX.B 81 /7, ModRM 11 111 010 = 0xFA.
                P6X86_64BaselineSymbolicRegister::Scratch0 => {
                    builder.emit(&[0x44, 0x8b, 0x97])?;
                    builder.emit_i32_le(structure_id_offset)?;
                    builder.emit(&[0x41, 0x81, 0xfa])?;
                    builder.emit(&cached_structure_id.to_le_bytes())?;
                }
                // load32: mov r11d, [rdi+disp32] = REX.R 8B, ModRM 10 011 111 = 0x9F.
                // cmp32:  cmp r11d, imm32 = REX.B 81 /7, ModRM 11 111 011 = 0xFB.
                P6X86_64BaselineSymbolicRegister::Scratch1 => {
                    builder.emit(&[0x44, 0x8b, 0x9f])?;
                    builder.emit_i32_le(structure_id_offset)?;
                    builder.emit(&[0x41, 0x81, 0xfb])?;
                    builder.emit(&cached_structure_id.to_le_bytes())?;
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
        P6X86_64BaselineMachineInstruction::Int32OrNumberArithmetic {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            tag_mask,
            payload_shift,
            int32_tag,
            double_tag,
            non_number_exit,
            overflow_exit,
            negative_zero_exit,
        } => emit_p6_x86_64_int32_or_number_arithmetic(
            builder,
            contract,
            bytecode_index,
            instruction,
            operation,
            tag_mask,
            payload_shift,
            int32_tag,
            double_tag,
            non_number_exit,
            overflow_exit,
            negative_zero_exit,
        ),
        P6X86_64BaselineMachineInstruction::Int32OrNumberArithmetic { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::Int32Bitwise {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch2,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
        } => match operation {
            P6X86_64BaselineInt32BitwiseOperation::BitAnd => {
                builder.emit(&[0x44, 0x89, 0xd1])?;
                builder.emit(&[0x44, 0x21, 0xd9])
            }
            P6X86_64BaselineInt32BitwiseOperation::BitOr => {
                builder.emit(&[0x44, 0x89, 0xd1])?;
                builder.emit(&[0x44, 0x09, 0xd9])
            }
            P6X86_64BaselineInt32BitwiseOperation::BitXor => {
                builder.emit(&[0x44, 0x89, 0xd1])?;
                builder.emit(&[0x44, 0x31, 0xd9])
            }
            P6X86_64BaselineInt32BitwiseOperation::RightShift => {
                builder.emit(&[0x44, 0x89, 0xd9])?;
                builder.emit(&[0x83, 0xe1, 0x1f])?;
                builder.emit(&[0x41, 0xd3, 0xfa])?;
                builder.emit(&[0x44, 0x89, 0xd1])
            }
        },
        P6X86_64BaselineMachineInstruction::Int32Bitwise { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::Int32EqualityToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            false_value,
            true_value,
        } => {
            builder.emit(&[0x45, 0x39, 0xda])?;
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(false_value)?;
            let done = match operation {
                P6X86_64BaselineInt32EqualityOperation::Equal => {
                    builder.emit_jne_rel32_internal()?
                }
                P6X86_64BaselineInt32EqualityOperation::NotEqual => {
                    builder.emit_je_rel32_internal()?
                }
            };
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(true_value)?;
            builder.patch_internal_branch_to_current(done)
        }
        P6X86_64BaselineMachineInstruction::Int32EqualityToBoolean { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::NoCallLooseEqualityToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            undefined_tag,
            null_tag,
            false_tag,
            true_tag,
            int32_tag,
            cell_tag,
            unsupported_exit,
            false_value,
            true_value,
        } => {
            let undefined_tag = p6_x86_64_semantic_u8_tag(bytecode_index, undefined_tag)?;
            let null_tag = p6_x86_64_semantic_u8_tag(bytecode_index, null_tag)?;
            let false_tag = p6_x86_64_semantic_u8_tag(bytecode_index, false_tag)?;
            let true_tag = p6_x86_64_semantic_u8_tag(bytecode_index, true_tag)?;
            let int32_tag = p6_x86_64_semantic_u8_tag(bytecode_index, int32_tag)?;
            let cell_tag = p6_x86_64_semantic_u8_tag(bytecode_index, cell_tag)?;

            let (equality_false_value, equality_true_value) = match operation {
                P6X86_64BaselineInt32EqualityOperation::Equal => (false_value, true_value),
                P6X86_64BaselineInt32EqualityOperation::NotEqual => (true_value, false_value),
            };

            let mut equality_true_branches = Vec::new();
            let mut equality_false_branches = Vec::new();

            builder.emit(&[0x4d, 0x39, 0xda])?;
            let exact_equal = builder.emit_je_rel32_internal()?;

            builder.emit(&[0x41, 0x80, 0xfa, undefined_tag])?;
            let left_undefined = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, null_tag])?;
            let left_null = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfb, undefined_tag])?;
            equality_false_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit(&[0x41, 0x80, 0xfb, null_tag])?;
            equality_false_branches.push(builder.emit_je_rel32_internal()?);

            builder.emit(&[0x41, 0x80, 0xfa, false_tag])?;
            let left_false = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, true_tag])?;
            let left_true = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, int32_tag])?;
            let left_int32 = builder.emit_je_rel32_internal()?;
            builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;

            builder.patch_internal_branch_to_current(left_undefined)?;
            builder.patch_internal_branch_to_current(left_null)?;
            builder.emit(&[0x41, 0x80, 0xfb, undefined_tag])?;
            equality_true_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit(&[0x41, 0x80, 0xfb, null_tag])?;
            equality_true_branches.push(builder.emit_je_rel32_internal()?);
            equality_false_branches.push(builder.emit_jmp_rel32_internal()?);

            builder.patch_internal_branch_to_current(left_false)?;
            builder.emit(&[0x41, 0x80, 0xfb, false_tag])?;
            equality_true_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit(&[0x41, 0x80, 0xfb, true_tag])?;
            equality_false_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;

            builder.patch_internal_branch_to_current(left_true)?;
            builder.emit(&[0x41, 0x80, 0xfb, true_tag])?;
            equality_true_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit(&[0x41, 0x80, 0xfb, false_tag])?;
            equality_false_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;

            builder.patch_internal_branch_to_current(left_int32)?;
            builder.emit(&[0x41, 0x80, 0xfb, int32_tag])?;
            equality_false_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;

            builder.patch_internal_branch_to_current(exact_equal)?;
            for tag in [
                undefined_tag,
                null_tag,
                false_tag,
                true_tag,
                int32_tag,
                cell_tag,
            ] {
                builder.emit(&[0x41, 0x80, 0xfa, tag])?;
                equality_true_branches.push(builder.emit_je_rel32_internal()?);
            }
            builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;

            let equality_false_offset = builder.offset()?;
            for branch in equality_false_branches {
                builder.patch_internal_branch_to_target(branch, equality_false_offset)?;
            }
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(equality_false_value)?;
            let done = builder.emit_jmp_rel32_internal()?;

            let equality_true_offset = builder.offset()?;
            for branch in equality_true_branches {
                builder.patch_internal_branch_to_target(branch, equality_true_offset)?;
            }
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(equality_true_value)?;
            builder.patch_internal_branch_to_current(done)
        }
        P6X86_64BaselineMachineInstruction::NoCallLooseEqualityToBoolean { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::PrimitiveStrictEqualityToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            undefined_tag,
            null_tag,
            false_tag,
            true_tag,
            int32_tag,
            cell_tag,
            unsupported_exit,
            false_value,
            true_value,
        } => {
            let undefined_tag = p6_x86_64_semantic_u8_tag(bytecode_index, undefined_tag)?;
            let null_tag = p6_x86_64_semantic_u8_tag(bytecode_index, null_tag)?;
            let false_tag = p6_x86_64_semantic_u8_tag(bytecode_index, false_tag)?;
            let true_tag = p6_x86_64_semantic_u8_tag(bytecode_index, true_tag)?;
            let int32_tag = p6_x86_64_semantic_u8_tag(bytecode_index, int32_tag)?;
            let cell_tag = p6_x86_64_semantic_u8_tag(bytecode_index, cell_tag)?;
            let known_non_double_tags = [
                undefined_tag,
                null_tag,
                false_tag,
                true_tag,
                int32_tag,
                cell_tag,
            ];

            p6_emit_known_strict_equality_operand_guard(
                builder,
                bytecode_index,
                P6X86_64BaselineSymbolicRegister::Scratch0,
                &known_non_double_tags,
                unsupported_exit,
                instruction,
            )?;
            p6_emit_known_strict_equality_operand_guard(
                builder,
                bytecode_index,
                P6X86_64BaselineSymbolicRegister::Scratch1,
                &known_non_double_tags,
                unsupported_exit,
                instruction,
            )?;

            let (equality_false_value, equality_true_value) = match operation {
                P6X86_64BaselineInt32EqualityOperation::Equal => (false_value, true_value),
                P6X86_64BaselineInt32EqualityOperation::NotEqual => (true_value, false_value),
            };

            builder.emit(&[0x4d, 0x39, 0xda])?;
            let exact_equal = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, cell_tag])?;
            builder.emit_jcc_rel32_side_exit(bytecode_index, 0x84, unsupported_exit)?;
            builder.emit(&[0x41, 0x80, 0xfb, cell_tag])?;
            builder.emit_jcc_rel32_side_exit(bytecode_index, 0x84, unsupported_exit)?;

            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(equality_false_value)?;
            let done = builder.emit_jmp_rel32_internal()?;

            builder.patch_internal_branch_to_current(exact_equal)?;
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(equality_true_value)?;
            builder.patch_internal_branch_to_current(done)
        }
        P6X86_64BaselineMachineInstruction::PrimitiveStrictEqualityToBoolean { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::PrimitiveToNumber {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag,
            null_tag,
            false_tag,
            true_tag,
            int32_tag,
            double_tag,
            unsupported_exit,
            int32_zero_value,
            int32_one_value,
            nan_value,
        } => {
            let undefined_tag = p6_x86_64_semantic_u8_tag(bytecode_index, undefined_tag)?;
            let null_tag = p6_x86_64_semantic_u8_tag(bytecode_index, null_tag)?;
            let false_tag = p6_x86_64_semantic_u8_tag(bytecode_index, false_tag)?;
            let true_tag = p6_x86_64_semantic_u8_tag(bytecode_index, true_tag)?;
            let int32_tag = p6_x86_64_semantic_u8_tag(bytecode_index, int32_tag)?;
            let double_tag = p6_x86_64_semantic_u8_tag(bytecode_index, double_tag)?;

            let mut done_branches = Vec::new();

            builder.emit(&[0x41, 0x80, 0xfa, int32_tag])?;
            done_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit(&[0x41, 0x80, 0xfa, double_tag])?;
            done_branches.push(builder.emit_je_rel32_internal()?);
            builder.emit(&[0x41, 0x80, 0xfa, false_tag])?;
            let false_to_zero = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, null_tag])?;
            let null_to_zero = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, true_tag])?;
            let true_to_one = builder.emit_je_rel32_internal()?;
            builder.emit(&[0x41, 0x80, 0xfa, undefined_tag])?;
            let undefined_to_nan = builder.emit_je_rel32_internal()?;
            builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;

            builder.patch_internal_branch_to_current(false_to_zero)?;
            builder.patch_internal_branch_to_current(null_to_zero)?;
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(int32_zero_value)?;
            done_branches.push(builder.emit_jmp_rel32_internal()?);

            builder.patch_internal_branch_to_current(true_to_one)?;
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(int32_one_value)?;
            done_branches.push(builder.emit_jmp_rel32_internal()?);

            builder.patch_internal_branch_to_current(undefined_to_nan)?;
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(nan_value)?;

            let done_offset = builder.offset()?;
            for branch in done_branches {
                builder.patch_internal_branch_to_target(branch, done_offset)?;
            }
            Ok(())
        }
        P6X86_64BaselineMachineInstruction::PrimitiveToNumber { .. } => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
        P6X86_64BaselineMachineInstruction::Int32RelationalToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            false_value,
            true_value,
        } => {
            builder.emit(&[0x45, 0x39, 0xda])?;
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(false_value)?;
            let done = match operation {
                P6X86_64BaselineInt32RelationalOperation::LessThan => {
                    builder.emit_jge_rel32_internal()?
                }
                P6X86_64BaselineInt32RelationalOperation::LessEqual => {
                    builder.emit_jg_rel32_internal()?
                }
                P6X86_64BaselineInt32RelationalOperation::GreaterThan => {
                    builder.emit_jle_rel32_internal()?
                }
                P6X86_64BaselineInt32RelationalOperation::GreaterEqual => {
                    builder.emit_jl_rel32_internal()?
                }
            };
            builder.emit(&[0x49, 0xba])?;
            builder.emit_u64_le(true_value)?;
            builder.patch_internal_branch_to_current(done)
        }
        P6X86_64BaselineMachineInstruction::Int32RelationalToBoolean { .. } => Err(
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
        P6X86_64BaselineMachineInstruction::PropertyDataIcSelfLoadGetByNameWithExit {
            record_index,
            base_frame_disp32,
            dest_frame_disp32,
            structure_id_offset,
            storage_ptr_disp,
            cell_tag,
            encoded_payload,
        } => emit_p10_x86_64_property_data_ic_self_load_get_by_name_with_exit(
            builder,
            bytecode_index,
            record_index,
            base_frame_disp32,
            dest_frame_disp32,
            structure_id_offset,
            storage_ptr_disp,
            cell_tag,
            encoded_payload,
        ),
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

fn p6_emit_known_strict_equality_operand_guard(
    builder: &mut P6X86_64SemanticByteBuilder,
    bytecode_index: crate::bytecode::BytecodeIndex,
    value: P6X86_64BaselineSymbolicRegister,
    known_tags: &[u8],
    unsupported_exit: P6X86_64BaselineSideExitLabel,
    instruction: P6X86_64BaselineMachineInstruction,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let mut known_branches = Vec::new();
    for tag in known_tags {
        p6_emit_symbolic_register_tag_compare(builder, bytecode_index, value, *tag, instruction)?;
        known_branches.push(builder.emit_je_rel32_internal()?);
    }
    builder.emit_jmp_rel32_side_exit(bytecode_index, unsupported_exit)?;
    let known_offset = builder.offset()?;
    for branch in known_branches {
        builder.patch_internal_branch_to_target(branch, known_offset)?;
    }
    Ok(())
}

fn p6_emit_symbolic_register_tag_compare(
    builder: &mut P6X86_64SemanticByteBuilder,
    bytecode_index: crate::bytecode::BytecodeIndex,
    value: P6X86_64BaselineSymbolicRegister,
    tag: u8,
    instruction: P6X86_64BaselineMachineInstruction,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    match value {
        P6X86_64BaselineSymbolicRegister::Scratch0 => builder.emit(&[0x41, 0x80, 0xfa, tag]),
        P6X86_64BaselineSymbolicRegister::Scratch1 => builder.emit(&[0x41, 0x80, 0xfb, tag]),
        _ => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                bytecode_index,
                instruction,
            },
        ),
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

// C++ JSC InlineAccess addresses the cell/storage through Address(base, disp)
// where base is propertyCache.m_baseGPR (PropertyBase / rdi) and disp is a
// byte displacement relative to the cell pointer (offsetRelativeToBase(offset)).
// This extracts that disp32 and enforces base == PropertyBase, mirroring the
// frame-local helper but for cell-relative addressing.
// bytecode/InlineAccess.cpp:203-204.
fn p6_x86_64_semantic_cell_relative_disp32(
    bytecode_index: crate::bytecode::BytecodeIndex,
    memory: P6X86_64BaselineMachineMemoryOperand,
) -> Result<i32, P6X86_64BaselineSemanticByteEmissionError> {
    let disp32 = match memory.location {
        P6X86_64BaselineOperandLocation::CellRelative { disp32 } => disp32,
        location => {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                    bytecode_index,
                    location,
                    reason:
                        P6X86_64BaselineSemanticOperandRejectionReason::ExpectedCellRelativeMemory,
                },
            );
        }
    };
    if memory.base != P6X86_64BaselineSymbolicRegister::PropertyBase {
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
        P6X86_64BaselineOperandLocation::FrameArgument {
            byte_offset_from_frame_base,
            ..
        } => {
            if byte_offset_from_frame_base <= i32::MAX as u64 {
                Ok(byte_offset_from_frame_base as i32)
            } else {
                Err(
                    P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                        bytecode_index,
                        location,
                        byte_offset: byte_offset_from_frame_base,
                    },
                )
            }
        }
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
        P6X86_64BaselineOperandLocation::CallFrameCalleeValue => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location,
                reason: P6X86_64BaselineSemanticOperandRejectionReason::ExpectedFrameLocalMemory,
            },
        ),
        P6X86_64BaselineOperandLocation::Immediate(_) => Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                bytecode_index,
                location,
                reason: P6X86_64BaselineSemanticOperandRejectionReason::ExpectedFrameLocalMemory,
            },
        ),
        // A cell-relative address is not a frame local; it is handled by
        // p6_x86_64_semantic_cell_relative_disp32 for the PropertyBase path.
        P6X86_64BaselineOperandLocation::CellRelative { .. } => Err(
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

pub(super) fn validate_p6_x86_64_semantic_value_layout(
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

pub(super) fn p6_x86_64_checked_byte_len(
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

// Emit the resident `get_by_id` self-OR-prototype-load DataIC fast path
// immediately followed by the slow-path native-exit return stub. Mirrors C++
// `generateGetByIdInlineAccessBaselineDataIC` (jit/JITInlineCacheGenerator.cpp:140-183),
// which shares the receiver structure guard between `CacheType::GetByIdSelf`
// (:146-152) and `CacheType::GetByIdPrototype` (:154-161); the prototype case is
// byte-for-byte the self case PLUS one delta: instead of `loadProperty` from the
// receiver, it `loadPtr`s the constant HOLDER from `offsetOfInlineHolder()` (:158)
// and `loadProperty`s from that holder (:159). The cached
// `{structure_id:u32@+0, offset:i32@+4, holder_ptr:u64@+8}` lives in the
// per-CodeBlock `BaselineJITData` record store at `[r13 + record_index*16]`
// (r13 == `GPRInfo::jitDataRegister`).
//
// C++ emits two distinct `CacheType`s patched into the inline access at runtime;
// the Rust artifact emits ONE inline-access shape per get_by_id site (the artifact
// is built once and never repatched), so we cannot statically pick the self vs
// prototype shape at emit time -- the IC outcome (own vs prototype load) is only
// known at the residency safepoint. PERMANENT DIVERGENCE: we emit a UNIFIED fast
// path whose only runtime branch is `holder_ptr == 0`. `holder_ptr == 0` is the
// self case (storage base = the unboxed receiver, byte-for-byte the prior
// self-load behavior); a nonzero `holder_ptr` is the prototype case (storage base
// = the baked holder, the `offsetOfInlineHolder()` analog, NOT unboxed because the
// arm stores the raw pinned `CoreObjectCell*`). The receiver structure guard is
// IDENTICAL in both cases (C++ guards the receiver by `m_inlineAccessBaseStructureID`
// for both `CacheType`s); prototype/holder validity is the StructureTransition
// watchpoint's job (it resets `holder_ptr` to 0 on a prototype shape change), never
// a per-call re-guard, exactly as C++ pins the holder via `m_conditionSet`.
//
// Rust ABI specifics not present in the C++ access path:
//   - rbp == value frame base (P6 prologue `mov rbp, rsi`), so the base value and
//     destination are `[rbp + disp32]` frame slots.
//   - the base frame slot holds a BOXED RuntimeValue `(cell_ptr << 8) | TAG_CELL`
//     (value/repr.rs:507); the fast path guards the low-byte cell tag, then `shr 8`
//     to recover the raw `CoreObjectCell` pointer before reading the cell header.
//     The baked `holder_ptr` is already a raw cell pointer, so it is NOT unboxed.
//
// Register use (all caller-clobberable here; rbp/r12/r13/r15/r9 are preserved):
//   rax = boxed base -> cell ptr -> loaded value; r10 = structure id / storage base;
//   r11 = cached PropertyOffset.
//
// Any guard miss jumps to the slow-path stub (the existing P10 native exit, which
// returns `encoded_payload`); a hit stores the value to the destination frame slot
// and jumps over the slow-path stub, staying resident. SENTINEL records
// (structure_id == 0) never match a real `StructureID`, so an unfilled site always
// takes the slow path until the first miss writes back the cached id/offset
// (self load) or the prototype arm bakes the holder (prototype load).
#[allow(clippy::too_many_arguments)]
fn emit_p10_x86_64_property_data_ic_self_load_get_by_name_with_exit(
    builder: &mut P6X86_64SemanticByteBuilder,
    bytecode_index: crate::bytecode::BytecodeIndex,
    record_index: u32,
    base_frame_disp32: i32,
    dest_frame_disp32: i32,
    structure_id_offset: i32,
    storage_ptr_disp: i32,
    cell_tag: u64,
    encoded_payload: P10X86_64BaselinePropertyNativeExitReturnPayload,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let cell_tag = p6_x86_64_semantic_u8_tag(bytecode_index, cell_tag)?;
    // record_index*16 + {0,4,8} must fit a disp32 from r13. record_index is the
    // dense property-site ordinal, which is tiny, but check rather than silently
    // wrap into a wrong record read. (Record stride is 16 since the record grew to
    // carry the prototype holder pointer at +8.)
    let record_byte_offset = (record_index as i64)
        .checked_mul(16)
        .filter(|&value| value <= i64::from(i32::MAX) - 8)
        .ok_or(
            P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                bytecode_index,
                location: P6X86_64BaselineOperandLocation::FrameLocal {
                    local_index: record_index,
                    slot_index: record_index,
                    byte_offset: u64::from(record_index).saturating_mul(16),
                },
                byte_offset: u64::from(record_index).saturating_mul(16),
            },
        )? as i32;
    let structure_id_record_disp32 = record_byte_offset;
    let offset_record_disp32 = record_byte_offset + 4;
    let holder_record_disp32 = record_byte_offset + 8;

    // mov rax, [rbp + base_frame_disp32]  (load boxed base value)
    builder.emit(&[0x48, 0x8b, 0x85])?;
    builder.emit_i32_le(base_frame_disp32)?;
    // cmp al, cell_tag ; jne -> slow path (base is not a cell)
    builder.emit(&[0x3c, cell_tag])?;
    let not_cell = builder.emit_jne_rel32_internal()?;
    // shr rax, 8  (unbox: recover the raw CoreObjectCell pointer)
    builder.emit(&[0x48, 0xc1, 0xe8, 0x08])?;
    // mov r10d, [rax + structure_id_offset]  (receiver structure id)
    builder.emit(&[0x44, 0x8b, 0x90])?;
    builder.emit_i32_le(structure_id_offset)?;
    // cmp r10d, [r13 + structure_id_record_disp32]  (receiver id vs cached id)
    // C++ JITInlineCacheGenerator.cpp:155-156/:147-148 -- IDENTICAL guard for both
    // the self and prototype CacheTypes.
    builder.emit(&[0x45, 0x3b, 0x95])?;
    builder.emit_i32_le(structure_id_record_disp32)?;
    let structure_miss = builder.emit_jne_rel32_internal()?;
    // mov r11d, [r13 + offset_record_disp32]  (cached PropertyOffset; C++ :149/:157)
    builder.emit(&[0x45, 0x8b, 0x9d])?;
    builder.emit_i32_le(offset_record_disp32)?;
    // mov r10, [r13 + holder_record_disp32]  (baked holder CoreObjectCell* or 0;
    // the offsetOfInlineHolder() analog, C++ :158 loadPtr -- but loaded into the
    // storage-base register r10, not resultJSR, since Rust reads the storage
    // pointer one indirection further). REX.WRB=0x4D: r13 as the rm base needs
    // REX.B=1 (rm=101 with REX.B addresses r13, not rbp).
    builder.emit(&[0x4d, 0x8b, 0x95])?;
    builder.emit_i32_le(holder_record_disp32)?;
    // test r10, r10 ; jnz -> use the holder as storage base (prototype load).
    builder.emit(&[0x4d, 0x85, 0xd2])?;
    let use_holder = builder.emit_jnz_rel32_internal()?;
    // SELF case (holder_ptr == 0): storage base comes from the unboxed receiver
    // (rax), byte-for-byte the prior self-load behavior. C++ :150 loadProperty from
    // baseJSR.
    // mov r10, [rax + storage_ptr_disp]  (receiver storage base pointer)
    builder.emit(&[0x4c, 0x8b, 0x90])?;
    builder.emit_i32_le(storage_ptr_disp)?;
    let storage_base_ready = builder.emit_jmp_rel32_internal()?;
    // PROTOTYPE case (holder_ptr != 0): storage base comes from the baked holder
    // (already in r10 as a raw cell ptr, so NO unbox). C++ :159 loadProperty from
    // the holder (resultJSR). The receiver-structure guard above is the only
    // per-call guard; the holder's validity is the watchpoint's job.
    let use_holder_offset = builder.offset()?;
    builder.patch_internal_branch_to_target(use_holder, use_holder_offset)?;
    // mov r10, [r10 + storage_ptr_disp]  (holder storage base pointer)
    builder.emit(&[0x4d, 0x8b, 0x92])?;
    builder.emit_i32_le(storage_ptr_disp)?;

    // storage_base_ready: r10 == storage base (receiver's or holder's).
    builder.patch_internal_branch_to_current(storage_base_ready)?;
    // mov rax, [r10 + r11*8]  (value at storage_base + offset*8, scale-8 SIB;
    // IDENTICAL tail for self and prototype, just rooted at the chosen storage)
    builder.emit(&[0x4b, 0x8b, 0x04, 0xda])?;
    // mov [rbp + dest_frame_disp32], rax  (store value to destination frame slot)
    builder.emit(&[0x48, 0x89, 0x85])?;
    builder.emit_i32_le(dest_frame_disp32)?;
    // jmp -> done (resident fall-through, over the slow-path stub)
    let resident_done = builder.emit_jmp_rel32_internal()?;

    // slow path: same bytes a standalone `ReturnPropertyNativeExitPayload` emits.
    let slow_path_offset = builder.offset()?;
    builder.patch_internal_branch_to_target(not_cell, slow_path_offset)?;
    builder.patch_internal_branch_to_target(structure_miss, slow_path_offset)?;
    builder.emit(&p10_x86_64_callable_property_native_exit_return_stub_bytes(
        encoded_payload.raw_bits(),
    ))?;

    // done: resident path rejoins here, past the slow-path stub.
    builder.patch_internal_branch_to_current(resident_done)?;
    Ok(())
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
    expected_architecture: AssemblerArchitecture,
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
    if source_buffer.architecture != Some(expected_architecture) {
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
    if source_descriptor.architecture != Some(expected_architecture) {
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

pub(super) fn p6_x86_64_semantic_source_buffer_id(
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
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOr => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrSubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBitAndOrBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrBranchTruthinessSubset
        }
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrEqualitySubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBitAndOrEqualityBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrEqualityBranchTruthinessSubset
        }
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrEqualityRelationalSubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBitAndOrEqualityRelationalBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrEqualityRelationalBranchTruthinessSubset
        }
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualitySubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityBranchTruthinessSubset
        }
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalSubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalBranchTruthinessSubset
        }
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberSubset
        }
        BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish => {
            BaselineMachineCodeEmitterKind::P8aX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchSubset
        }
        BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse => {
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchTruthinessSubset
        }
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean => {
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanSubset
        }
        _ => BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapSubset,
    }
}

fn p6_opcode_subset_uses_no_call_loose_equality(subset: BaselineSupportedOpcodeSubset) -> bool {
    matches!(
        subset,
        BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality
            | BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullish
            | BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelational
            | BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullish
            | BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse
            | BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber
            | BaselineSupportedOpcodeSubset::P8aConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullish
            | BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberBranchNullishFalse
    )
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

fn p6_x86_64_frame_local_capacity(shape: RegisterFrameShape) -> u32 {
    shape
        .num_callee_locals
        .max(shape.num_vars.saturating_add(shape.num_temporaries))
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
    frame_local_count: u32,
    return_carrier: P6X86_64BaselineReturnRegisterContract,
) -> Result<P6X86_64BaselineBackendInstructionContract, P6X86_64BaselineBackendContractError> {
    let bytecode_index = lowered.bytecode_index;
    let mut operand_locations = Vec::new();
    let mut arithmetic_exit_policy = None;
    let mut bitwise_exit_policy = None;
    let mut equality_exit_policy = None;
    let mut relational_exit_policy = None;
    let mut primitive_to_number_exit_policy = None;
    let mut branch_target = None;

    match lowered.operation {
        P6X86_64BaselineLoweredOperation::LoopHint => {}
        P6X86_64BaselineLoweredOperation::LoadUndefined { destination } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
                frame_local_count,
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
                frame_local_count,
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
                frame_local_count,
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
                frame_local_count,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::Immediate(
                    P6X86_64BaselineImmediateOperand::Int32(value),
                ),
            });
        }
        P6X86_64BaselineLoweredOperation::LoadCallee { destination } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
                frame_local_count,
            )?);
            operand_locations.push(P6X86_64BaselineOperandLocationRecord {
                role: P6X86_64BaselineOperandRole::Source,
                location: P6X86_64BaselineOperandLocation::CallFrameCalleeValue,
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
                frame_local_count,
            )?);
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Source,
                source,
                false,
                frame_layout,
                frame_local_count,
            )?);
        }
        P6X86_64BaselineLoweredOperation::ToNumberPrimitive {
            destination,
            source,
        } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Destination,
                destination,
                true,
                frame_layout,
                frame_local_count,
            )?);
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::Source,
                source,
                false,
                frame_layout,
                frame_local_count,
            )?);
            primitive_to_number_exit_policy =
                Some(p6_primitive_to_number_exit_policy(bytecode_index));
        }
        P6X86_64BaselineLoweredOperation::Return { source } => {
            operand_locations.push(p6_operand_location_record(
                bytecode_index,
                P6X86_64BaselineOperandRole::ReturnValue,
                source,
                false,
                frame_layout,
                frame_local_count,
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
                frame_local_count,
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
                frame_local_count,
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
                frame_local_count,
            )?;
            arithmetic_exit_policy = Some(p6_int32_arithmetic_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32ArithmeticOperation::Mul,
                P6X86_64BaselineCheckedInt32Arithmetic::CheckedMul,
            ));
        }
        P6X86_64BaselineLoweredOperation::BitAndInt32 {
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
                frame_local_count,
            )?;
            bitwise_exit_policy = Some(p6_int32_bitwise_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32BitwiseOperation::BitAnd,
            ));
        }
        P6X86_64BaselineLoweredOperation::BitOrInt32 {
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
                frame_local_count,
            )?;
            bitwise_exit_policy = Some(p6_int32_bitwise_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32BitwiseOperation::BitOr,
            ));
        }
        P6X86_64BaselineLoweredOperation::BitXorInt32 {
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
                frame_local_count,
            )?;
            bitwise_exit_policy = Some(p6_int32_bitwise_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32BitwiseOperation::BitXor,
            ));
        }
        P6X86_64BaselineLoweredOperation::RightShiftInt32 {
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
                frame_local_count,
            )?;
            bitwise_exit_policy = Some(p6_int32_bitwise_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32BitwiseOperation::RightShift,
            ));
        }
        P6X86_64BaselineLoweredOperation::EqualInt32 {
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
                frame_local_count,
            )?;
            equality_exit_policy = Some(p6_int32_equality_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32EqualityOperation::Equal,
            ));
        }
        P6X86_64BaselineLoweredOperation::NotEqualInt32 {
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
                frame_local_count,
            )?;
            equality_exit_policy = Some(p6_int32_equality_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
            ));
        }
        P6X86_64BaselineLoweredOperation::EqualNoCallLoose {
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
                frame_local_count,
            )?;
            equality_exit_policy = Some(p6_no_call_loose_equality_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32EqualityOperation::Equal,
            ));
        }
        P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose {
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
                frame_local_count,
            )?;
            equality_exit_policy = Some(p6_no_call_loose_equality_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
            ));
        }
        P6X86_64BaselineLoweredOperation::StrictEqualPrimitive {
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
                frame_local_count,
            )?;
            equality_exit_policy = Some(p6_strict_equality_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32EqualityOperation::Equal,
            ));
        }
        P6X86_64BaselineLoweredOperation::StrictNotEqualPrimitive {
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
                frame_local_count,
            )?;
            equality_exit_policy = Some(p6_strict_equality_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
            ));
        }
        P6X86_64BaselineLoweredOperation::LessThanInt32 {
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
                frame_local_count,
            )?;
            relational_exit_policy = Some(p6_int32_relational_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32RelationalOperation::LessThan,
            ));
        }
        P6X86_64BaselineLoweredOperation::LessEqualInt32 {
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
                frame_local_count,
            )?;
            relational_exit_policy = Some(p6_int32_relational_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32RelationalOperation::LessEqual,
            ));
        }
        P6X86_64BaselineLoweredOperation::GreaterThanInt32 {
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
                frame_local_count,
            )?;
            relational_exit_policy = Some(p6_int32_relational_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32RelationalOperation::GreaterThan,
            ));
        }
        P6X86_64BaselineLoweredOperation::GreaterEqualInt32 {
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
                frame_local_count,
            )?;
            relational_exit_policy = Some(p6_int32_relational_exit_policy(
                bytecode_index,
                P6X86_64BaselineInt32RelationalOperation::GreaterEqual,
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
                frame_local_count,
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
                frame_local_count,
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
        | P6X86_64BaselineLoweredOperation::JsCallNativeExit { .. } => {}
        // Bind the destination + base frame slots for a GetByName self-load site so
        // the backend can emit the inline DataIC fast path's frame-relative
        // `mov rax, [rbp + base]` / `mov [rbp + dest], rax`. The base is read (not a
        // destination) and the destination is written; both must be plain frame
        // locals. Other property-handoff operand shapes (GetByValue, PutByName, ...)
        // bind nothing and keep the slow-path-only native exit -- this batch only
        // emits the self-load DataIC for GetByName.
        P6X86_64BaselineLoweredOperation::PropertyNativeExit { operands, .. } => {
            if let P10X86_64BaselinePropertyNativeExitOperands::GetByName { destination, base } =
                operands
            {
                operand_locations.push(p6_operand_location_record(
                    bytecode_index,
                    P6X86_64BaselineOperandRole::Destination,
                    destination,
                    true,
                    frame_layout,
                    frame_local_count,
                )?);
                operand_locations.push(p6_operand_location_record(
                    bytecode_index,
                    P6X86_64BaselineOperandRole::Source,
                    base,
                    false,
                    frame_layout,
                    frame_local_count,
                )?);
            }
        }
    }

    Ok(P6X86_64BaselineBackendInstructionContract {
        lowered,
        operand_locations,
        arithmetic_exit_policy,
        bitwise_exit_policy,
        equality_exit_policy,
        relational_exit_policy,
        primitive_to_number_exit_policy,
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
    frame_local_count: u32,
) -> Result<(), P6X86_64BaselineBackendContractError> {
    operand_locations.push(p6_operand_location_record(
        bytecode_index,
        P6X86_64BaselineOperandRole::Destination,
        destination,
        true,
        frame_layout,
        frame_local_count,
    )?);
    operand_locations.push(p6_operand_location_record(
        bytecode_index,
        P6X86_64BaselineOperandRole::Left,
        left,
        false,
        frame_layout,
        frame_local_count,
    )?);
    operand_locations.push(p6_operand_location_record(
        bytecode_index,
        P6X86_64BaselineOperandRole::Right,
        right,
        false,
        frame_layout,
        frame_local_count,
    )?);
    Ok(())
}

fn p6_operand_location_record(
    bytecode_index: crate::bytecode::BytecodeIndex,
    role: P6X86_64BaselineOperandRole,
    register: VirtualRegister,
    is_destination: bool,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
    frame_local_count: u32,
) -> Result<P6X86_64BaselineOperandLocationRecord, P6X86_64BaselineBackendContractError> {
    let location = p6_operand_location(
        bytecode_index,
        role,
        register,
        is_destination,
        frame_layout,
        frame_local_count,
    )?;
    Ok(P6X86_64BaselineOperandLocationRecord { role, location })
}

fn p6_operand_location(
    bytecode_index: crate::bytecode::BytecodeIndex,
    role: P6X86_64BaselineOperandRole,
    register: VirtualRegister,
    is_destination: bool,
    frame_layout: P6X86_64BaselineFrameLayoutContract,
    frame_local_count: u32,
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
            let byte_offset_from_argument_base =
                u64::from(index) * u64::from(frame_layout.value_slot_width_bytes);
            let byte_offset_from_frame_base = u64::from(frame_local_count)
                .saturating_mul(u64::from(frame_layout.local_slot_stride_bytes))
                .saturating_add(byte_offset_from_argument_base);
            P6X86_64BaselineOperandLocation::FrameArgument {
                argument_index_including_this: index,
                raw_virtual_register: register.raw(),
                byte_offset_from_argument_base,
                byte_offset_from_frame_base,
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
        operand_guard:
            P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithNumberTagsAfterInt32FastPath,
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

fn p6_int32_bitwise_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operation: P6X86_64BaselineInt32BitwiseOperation,
) -> P6X86_64BaselineInt32BitwiseExitPolicy {
    P6X86_64BaselineInt32BitwiseExitPolicy {
        operation,
        operand_guard: P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag,
        non_int32_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        ),
    }
}

fn p6_int32_equality_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operation: P6X86_64BaselineInt32EqualityOperation,
) -> P6X86_64BaselineInt32EqualityExitPolicy {
    P6X86_64BaselineInt32EqualityExitPolicy {
        operation,
        fast_path: P6X86_64BaselineEqualityFastPath::Int32Only,
        operand_guard: P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag,
        non_int32_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        ),
    }
}

fn p6_no_call_loose_equality_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operation: P6X86_64BaselineInt32EqualityOperation,
) -> P6X86_64BaselineInt32EqualityExitPolicy {
    P6X86_64BaselineInt32EqualityExitPolicy {
        operation,
        fast_path: P6X86_64BaselineEqualityFastPath::GeneratedNoCallLoose,
        operand_guard: P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag,
        non_int32_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        ),
    }
}

fn p6_strict_equality_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operation: P6X86_64BaselineInt32EqualityOperation,
) -> P6X86_64BaselineInt32EqualityExitPolicy {
    P6X86_64BaselineInt32EqualityExitPolicy {
        operation,
        fast_path: P6X86_64BaselineEqualityFastPath::PrimitiveStrictNoDouble,
        operand_guard: P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag,
        non_int32_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::UnsupportedStrictEqualityOperand,
        ),
    }
}

fn p6_int32_relational_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    operation: P6X86_64BaselineInt32RelationalOperation,
) -> P6X86_64BaselineInt32RelationalExitPolicy {
    P6X86_64BaselineInt32RelationalExitPolicy {
        operation,
        operand_guard: P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag,
        non_int32_exit: p6_arithmetic_side_exit_contract(
            bytecode_index,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        ),
    }
}

fn p6_primitive_to_number_exit_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
) -> P6X86_64BaselinePrimitiveToNumberExitPolicy {
    P6X86_64BaselinePrimitiveToNumberExitPolicy {
        unsupported_operand_exit: P6X86_64BaselinePrimitiveToNumberSideExitContract {
            reason: P6X86_64BaselinePrimitiveToNumberSideExitReason::UnsupportedOperand,
            destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
            retained_bytecode_index: bytecode_index,
            may_throw: false,
            runtime_call: false,
            heap_allocation: false,
            touches_gc_roots: false,
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
    let expected_emitter = match contract.emitter_kind {
        BaselineMachineCodeEmitterKind::P6Arm64NoCallNoHeapReturnSeedSubset => {
            BaselineMachineCodeEmitterKind::P6Arm64NoCallNoHeapReturnSeedSubset
        }
        _ => p6_x86_64_emitter_kind_for_subset(contract.opcode_subset),
    };
    if contract.emitter_kind != expected_emitter {
        return Err(P6X86_64BaselineInstructionSelectionError::Contract {
            error: P6X86_64BaselineBackendContractError::UnexpectedEmitterKind {
                expected: expected_emitter,
                actual: contract.emitter_kind,
            },
        });
    }
    let expected_architecture = contract.emitter_kind.expected_architecture();
    if contract.architecture != expected_architecture {
        return Err(P6X86_64BaselineInstructionSelectionError::Contract {
            error: P6X86_64BaselineBackendContractError::UnexpectedArchitecture {
                expected: expected_architecture,
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
            contract.frame_local_count,
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
    frame_local_count: u32,
    return_carrier: P6X86_64BaselineReturnRegisterContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    let expected = p6_x86_64_baseline_backend_instruction_contract(
        instruction.lowered,
        frame_layout,
        frame_local_count,
        return_carrier,
    )
    .map_err(|error| P6X86_64BaselineInstructionSelectionError::Contract { error })?;

    p6_validate_operand_locations_match_lowered(instruction, &expected)?;
    p6_validate_arithmetic_policy_match_lowered(instruction, &expected)?;
    p6_validate_bitwise_policy_match_lowered(instruction, &expected)?;
    p6_validate_equality_policy_match_lowered(instruction, &expected)?;
    p6_validate_relational_policy_match_lowered(instruction, &expected)?;
    p6_validate_primitive_to_number_policy_match_lowered(instruction, &expected)?;
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

fn p6_validate_bitwise_policy_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    match (
        instruction.bitwise_exit_policy,
        expected.bitwise_exit_policy,
    ) {
        (None, None) => Ok(()),
        (Some(actual), None) => Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        ),
        (None, Some(_)) => Err(
            P6X86_64BaselineInstructionSelectionError::MissingBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
            },
        ),
        (Some(actual), Some(expected)) => {
            p6_validate_bitwise_policy(
                instruction.lowered.bytecode_index,
                expected.operation,
                actual,
            )?;
            p6_side_exit_label_from_contract(
                instruction.lowered.bytecode_index,
                P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                actual.non_int32_exit,
            )?;
            Ok(())
        }
    }
}

fn p6_validate_equality_policy_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    match (
        instruction.equality_exit_policy,
        expected.equality_exit_policy,
    ) {
        (None, None) => Ok(()),
        (Some(actual), None) => Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedEqualityPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        ),
        (None, Some(_)) => Err(
            P6X86_64BaselineInstructionSelectionError::MissingEqualityPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
            },
        ),
        (Some(actual), Some(expected)) => {
            p6_validate_equality_policy(
                instruction.lowered.bytecode_index,
                expected.operation,
                expected.fast_path,
                actual,
            )?;
            let expected_reason = match expected.fast_path {
                P6X86_64BaselineEqualityFastPath::PrimitiveStrictNoDouble => {
                    P6X86_64BaselineArithmeticSideExitReason::UnsupportedStrictEqualityOperand
                }
                P6X86_64BaselineEqualityFastPath::Int32Only
                | P6X86_64BaselineEqualityFastPath::GeneratedNoCallLoose => {
                    P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand
                }
            };
            p6_side_exit_label_from_contract(
                instruction.lowered.bytecode_index,
                expected_reason,
                actual.non_int32_exit,
            )?;
            Ok(())
        }
    }
}

fn p6_validate_relational_policy_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    match (
        instruction.relational_exit_policy,
        expected.relational_exit_policy,
    ) {
        (None, None) => Ok(()),
        (Some(actual), None) => Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedRelationalPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        ),
        (None, Some(_)) => Err(
            P6X86_64BaselineInstructionSelectionError::MissingRelationalPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
            },
        ),
        (Some(actual), Some(expected)) => {
            p6_validate_relational_policy(
                instruction.lowered.bytecode_index,
                expected.operation,
                actual,
            )?;
            p6_side_exit_label_from_contract(
                instruction.lowered.bytecode_index,
                P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                actual.non_int32_exit,
            )?;
            Ok(())
        }
    }
}

fn p6_validate_primitive_to_number_policy_match_lowered(
    instruction: &P6X86_64BaselineBackendInstructionContract,
    expected: &P6X86_64BaselineBackendInstructionContract,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    match (
        instruction.primitive_to_number_exit_policy,
        expected.primitive_to_number_exit_policy,
    ) {
        (None, None) => Ok(()),
        (Some(actual), None) => Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedPrimitiveToNumberPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        ),
        (None, Some(_)) => Err(
            P6X86_64BaselineInstructionSelectionError::MissingPrimitiveToNumberPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
            },
        ),
        (Some(actual), Some(_)) => {
            p6_primitive_to_number_side_exit_label_from_contract(
                instruction.lowered.bytecode_index,
                P6X86_64BaselinePrimitiveToNumberSideExitReason::UnsupportedOperand,
                actual.unsupported_operand_exit,
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
        P6X86_64BaselineLoweredOperation::LoopHint => Vec::new(),
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
        P6X86_64BaselineLoweredOperation::LoadCallee { .. } => p6_select_load_callee(instruction)?,
        P6X86_64BaselineLoweredOperation::Move { .. } => p6_select_move(instruction)?,
        P6X86_64BaselineLoweredOperation::ToNumberPrimitive { .. } => {
            p6_select_primitive_to_number(contract, instruction)?
        }
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
        P6X86_64BaselineLoweredOperation::BitAndInt32 { .. } => p6_select_int32_bitwise(
            contract,
            instruction,
            P6X86_64BaselineInt32BitwiseOperation::BitAnd,
        )?,
        P6X86_64BaselineLoweredOperation::BitOrInt32 { .. } => p6_select_int32_bitwise(
            contract,
            instruction,
            P6X86_64BaselineInt32BitwiseOperation::BitOr,
        )?,
        P6X86_64BaselineLoweredOperation::BitXorInt32 { .. } => p6_select_int32_bitwise(
            contract,
            instruction,
            P6X86_64BaselineInt32BitwiseOperation::BitXor,
        )?,
        P6X86_64BaselineLoweredOperation::RightShiftInt32 { .. } => p6_select_int32_bitwise(
            contract,
            instruction,
            P6X86_64BaselineInt32BitwiseOperation::RightShift,
        )?,
        P6X86_64BaselineLoweredOperation::EqualInt32 { .. } => p6_select_int32_equality(
            contract,
            instruction,
            P6X86_64BaselineInt32EqualityOperation::Equal,
        )?,
        P6X86_64BaselineLoweredOperation::NotEqualInt32 { .. } => p6_select_int32_equality(
            contract,
            instruction,
            P6X86_64BaselineInt32EqualityOperation::NotEqual,
        )?,
        P6X86_64BaselineLoweredOperation::EqualNoCallLoose { .. } => {
            p6_select_no_call_loose_equality(
                contract,
                instruction,
                P6X86_64BaselineInt32EqualityOperation::Equal,
            )?
        }
        P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose { .. } => {
            p6_select_no_call_loose_equality(
                contract,
                instruction,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
            )?
        }
        P6X86_64BaselineLoweredOperation::StrictEqualPrimitive { .. } => {
            p6_select_primitive_strict_equality(
                contract,
                instruction,
                P6X86_64BaselineInt32EqualityOperation::Equal,
            )?
        }
        P6X86_64BaselineLoweredOperation::StrictNotEqualPrimitive { .. } => {
            p6_select_primitive_strict_equality(
                contract,
                instruction,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
            )?
        }
        P6X86_64BaselineLoweredOperation::LessThanInt32 { .. } => p6_select_int32_relational(
            contract,
            instruction,
            P6X86_64BaselineInt32RelationalOperation::LessThan,
        )?,
        P6X86_64BaselineLoweredOperation::LessEqualInt32 { .. } => p6_select_int32_relational(
            contract,
            instruction,
            P6X86_64BaselineInt32RelationalOperation::LessEqual,
        )?,
        P6X86_64BaselineLoweredOperation::GreaterThanInt32 { .. } => p6_select_int32_relational(
            contract,
            instruction,
            P6X86_64BaselineInt32RelationalOperation::GreaterThan,
        )?,
        P6X86_64BaselineLoweredOperation::GreaterEqualInt32 { .. } => p6_select_int32_relational(
            contract,
            instruction,
            P6X86_64BaselineInt32RelationalOperation::GreaterEqual,
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
            operands,
            encoded_payload,
            ..
        } => p10_select_property_native_exit(contract, instruction, operands, encoded_payload)?,
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
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operands: P10X86_64BaselinePropertyNativeExitOperands,
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

    // Admission: only a GetByName self-access site with its destination + base both
    // bound to plain frame locals (so the inline DataIC can `mov rax, [rbp+disp]` /
    // `mov [rbp+disp], rax`) gets the resident self-load fast path. Every other
    // property-handoff opcode -- and any GetByName whose operands do not resolve to
    // frame locals -- binds no operand locations in the contract and keeps the
    // slow-path-only native exit. The record store starts SENTINEL (structure_id ==
    // 0), so an admitted-but-unfilled site always takes the slow path until the
    // first P10 miss writes back the cached structure/offset (STEP C).
    //
    // C++ baseline emits the IC fast path for every get_by_id; we admit only the
    // structurally simple self-load shape this batch can encode and fall back to the
    // existing interpreter handoff for the rest, keeping the fallback fully intact.
    if let P10X86_64BaselinePropertyNativeExitOperands::GetByName { .. } = operands {
        if let Some((dest_frame_disp32, base_frame_disp32)) =
            p10_self_load_frame_disp32_for_get_by_name(instruction)?
        {
            let record_index = encoded_payload.property_exit_index();
            return Ok(vec![
                P6X86_64BaselineMachineInstruction::PropertyDataIcSelfLoadGetByNameWithExit {
                    record_index,
                    base_frame_disp32,
                    dest_frame_disp32,
                    structure_id_offset: P10_X86_64_BASELINE_CELL_STRUCTURE_ID_OFFSET,
                    storage_ptr_disp: P10_X86_64_BASELINE_CELL_STORAGE_PTR_DISP,
                    cell_tag: contract.value_layout.cell_tag,
                    encoded_payload,
                },
            ]);
        }
        // GetByName whose operands the contract bound but that did not resolve to the
        // simple frame-local self-load shape (e.g. an argument base): keep the
        // slow-path exit, but do not require empty operand locations -- the contract
        // legitimately bound (Destination, Source) for every GetByName site.
        return Ok(vec![
            P6X86_64BaselineMachineInstruction::ReturnPropertyNativeExitPayload { encoded_payload },
        ]);
    }

    p6_exact_operand_locations(instruction, &[])?;
    Ok(vec![
        P6X86_64BaselineMachineInstruction::ReturnPropertyNativeExitPayload { encoded_payload },
    ])
}

// Resolve the (destination, base) frame displacements for a GetByName self-load
// DataIC, or `None` when the contract bound no operands (the non-admitted shape) or
// the operands are not an admissible self-load shape fitting a disp32.
//
// Both the destination slot and the base value are addressed from rbp (the P6
// prologue's `mov rbp, rsi` value frame base), the same base the existing frame
// `LoadQ`/`StoreQ` use. The destination must be a plain frame local (a get_by_id
// destination is always a temp/local, never a callee argument slot). The BASE,
// however, may be EITHER a frame local OR a frame ARGUMENT (FIX 1): real richards'
// hot get_by_id receivers (`this.currentTcb`, `this.list`, ...) put `this` in a
// frame ARGUMENT slot, so admitting only frame-local bases left 231 hot sites on the
// slow path. A frame argument's rbp-relative byte offset is
// `byte_offset_from_frame_base` -- computed by the same `p6_operand_location`
// VirtualRegister->byte_offset mechanism the call frame uses for argument slots --
// so the DataIC reads `mov rax, [rbp + argument_disp32]` exactly like a local
// (`p6_x86_64_disp32_for_frame_local` already treats both as positive rbp-relative
// displacements for LoadQ/StoreQ). Every other base shape (constant/computed) stays
// on the slow path.
fn p10_self_load_frame_disp32_for_get_by_name(
    instruction: &P6X86_64BaselineBackendInstructionContract,
) -> Result<Option<(i32, i32)>, P6X86_64BaselineInstructionSelectionError> {
    let records = instruction.operand_locations.as_slice();
    let [destination, base] = records else {
        // No bound operands => non-admitted site; keep the slow-path exit.
        return Ok(None);
    };
    if destination.role != P6X86_64BaselineOperandRole::Destination
        || base.role != P6X86_64BaselineOperandRole::Source
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedOperandRoles {
                bytecode_index: instruction.lowered.bytecode_index,
                expected: vec![
                    P6X86_64BaselineOperandRole::Destination,
                    P6X86_64BaselineOperandRole::Source,
                ],
                actual: vec![destination.role, base.role],
            },
        );
    }
    let (Some(dest_disp32), Some(base_disp32)) = (
        // Destination: frame local only (the write target frame slot).
        p10_frame_local_disp32(destination.location),
        // Base: frame local OR frame argument, both rbp-relative (FIX 1).
        p10_self_load_base_disp32(base.location),
    ) else {
        // A non-frame-resident operand (constant/computed dest, or a non-frame base)
        // is not the simple self-load shape; keep the slow-path exit rather than
        // encode an address we cannot.
        return Ok(None);
    };
    Ok(Some((dest_disp32, base_disp32)))
}

fn p10_frame_local_disp32(location: P6X86_64BaselineOperandLocation) -> Option<i32> {
    match location {
        P6X86_64BaselineOperandLocation::FrameLocal { byte_offset, .. } => {
            i32::try_from(byte_offset).ok()
        }
        _ => None,
    }
}

// rbp-relative disp32 for a self-load BASE: a frame local (positive
// `byte_offset`) or a frame argument (positive `byte_offset_from_frame_base`),
// both addressed from rbp == the value frame base, identical to
// `p6_x86_64_disp32_for_frame_local`'s frame-local/frame-argument handling for
// LoadQ/StoreQ. Any other location (constant/computed) is not an admissible base.
fn p10_self_load_base_disp32(location: P6X86_64BaselineOperandLocation) -> Option<i32> {
    match location {
        P6X86_64BaselineOperandLocation::FrameLocal { byte_offset, .. } => {
            i32::try_from(byte_offset).ok()
        }
        P6X86_64BaselineOperandLocation::FrameArgument {
            byte_offset_from_frame_base,
            ..
        } => i32::try_from(byte_offset_from_frame_base).ok(),
        _ => None,
    }
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
    if let Some(actual) = instruction.equality_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedEqualityPolicy {
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

fn p6_select_primitive_to_number(
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
    if let Some(actual) = instruction.bitwise_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.equality_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedEqualityPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.relational_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedRelationalPolicy {
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
    let policy = instruction.primitive_to_number_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingPrimitiveToNumberPolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    let unsupported_exit = p6_primitive_to_number_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselinePrimitiveToNumberSideExitReason::UnsupportedOperand,
        policy.unsupported_operand_exit,
    )?;
    let int32_zero_value = contract.value_layout.immediate_int32_tag;
    let int32_one_value =
        (1_u64 << contract.value_layout.payload_shift) | contract.value_layout.immediate_int32_tag;
    let nan_value = p6_primitive_to_number_undefined_nan_value(contract.value_layout);

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source,
        },
        P6X86_64BaselineMachineInstruction::PrimitiveToNumber {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag: contract.value_layout.immediate_undefined_tag,
            null_tag: contract.value_layout.immediate_null_tag,
            false_tag: contract.value_layout.immediate_false_tag,
            true_tag: contract.value_layout.immediate_true_tag,
            int32_tag: contract.value_layout.immediate_int32_tag,
            double_tag: contract.value_layout.immediate_double_tag,
            unsupported_exit,
            int32_zero_value,
            int32_one_value,
            nan_value,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_load_callee(
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
        P6X86_64BaselineMachineInstruction::MoveQ {
            destination: P6X86_64BaselineMachineOperand::Register(
                P6X86_64BaselineSymbolicRegister::Scratch0,
            ),
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

    let non_number_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        policy.non_int32_exit,
    )?;
    let overflow_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::Overflow,
        policy.overflow_exit,
    )?;

    let negative_zero_exit = if operation == P6X86_64BaselineInt32ArithmeticOperation::Mul {
        Some(p6_negative_zero_side_exit_label(
            instruction.lowered.bytecode_index,
            policy,
        )?)
    } else {
        None
    };

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source: left,
        },
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch1,
            source: right,
        },
        P6X86_64BaselineMachineInstruction::Int32OrNumberArithmetic {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            tag_mask: contract.value_layout.tag_mask,
            payload_shift: contract.value_layout.payload_shift,
            int32_tag: contract.value_layout.immediate_int32_tag,
            double_tag: contract.value_layout.immediate_double_tag,
            non_number_exit,
            overflow_exit,
            negative_zero_exit,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_int32_bitwise(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operation: P6X86_64BaselineInt32BitwiseOperation,
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
    let policy = instruction.bitwise_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingBitwisePolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_validate_bitwise_policy(instruction.lowered.bytecode_index, operation, policy)?;
    let non_int32_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        policy.non_int32_exit,
    )?;

    Ok(vec![
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
        P6X86_64BaselineMachineInstruction::Int32Bitwise {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch2,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
        },
        P6X86_64BaselineMachineInstruction::RetagInt32 {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            payload: P6X86_64BaselineSymbolicRegister::Scratch2,
            payload_shift: contract.value_layout.payload_shift,
            tag: contract.value_layout.immediate_int32_tag,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_int32_equality(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operation: P6X86_64BaselineInt32EqualityOperation,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.bitwise_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

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
    let policy = instruction.equality_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingEqualityPolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_validate_equality_policy(
        instruction.lowered.bytecode_index,
        operation,
        P6X86_64BaselineEqualityFastPath::Int32Only,
        policy,
    )?;
    let non_int32_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        policy.non_int32_exit,
    )?;

    Ok(vec![
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
        P6X86_64BaselineMachineInstruction::Int32EqualityToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            false_value: contract.value_layout.immediate_false_tag,
            true_value: contract.value_layout.immediate_true_tag,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_no_call_loose_equality(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operation: P6X86_64BaselineInt32EqualityOperation,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.bitwise_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

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
    let policy = instruction.equality_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingEqualityPolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_validate_equality_policy(
        instruction.lowered.bytecode_index,
        operation,
        P6X86_64BaselineEqualityFastPath::GeneratedNoCallLoose,
        policy,
    )?;
    let unsupported_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        policy.non_int32_exit,
    )?;

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source: left,
        },
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch1,
            source: right,
        },
        P6X86_64BaselineMachineInstruction::NoCallLooseEqualityToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            undefined_tag: contract.value_layout.immediate_undefined_tag,
            null_tag: contract.value_layout.immediate_null_tag,
            false_tag: contract.value_layout.immediate_false_tag,
            true_tag: contract.value_layout.immediate_true_tag,
            int32_tag: contract.value_layout.immediate_int32_tag,
            cell_tag: contract.value_layout.cell_tag,
            unsupported_exit,
            false_value: contract.value_layout.immediate_false_tag,
            true_value: contract.value_layout.immediate_true_tag,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_primitive_strict_equality(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operation: P6X86_64BaselineInt32EqualityOperation,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.bitwise_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

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
    let policy = instruction.equality_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingEqualityPolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_validate_equality_policy(
        instruction.lowered.bytecode_index,
        operation,
        P6X86_64BaselineEqualityFastPath::PrimitiveStrictNoDouble,
        policy,
    )?;
    let unsupported_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::UnsupportedStrictEqualityOperand,
        policy.non_int32_exit,
    )?;

    Ok(vec![
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            source: left,
        },
        P6X86_64BaselineMachineInstruction::LoadQ {
            destination: P6X86_64BaselineSymbolicRegister::Scratch1,
            source: right,
        },
        P6X86_64BaselineMachineInstruction::PrimitiveStrictEqualityToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            undefined_tag: contract.value_layout.immediate_undefined_tag,
            null_tag: contract.value_layout.immediate_null_tag,
            false_tag: contract.value_layout.immediate_false_tag,
            true_tag: contract.value_layout.immediate_true_tag,
            int32_tag: contract.value_layout.immediate_int32_tag,
            cell_tag: contract.value_layout.cell_tag,
            unsupported_exit,
            false_value: contract.value_layout.immediate_false_tag,
            true_value: contract.value_layout.immediate_true_tag,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_select_int32_relational(
    contract: &P6X86_64BaselineBackendContractRecord,
    instruction: &P6X86_64BaselineBackendInstructionContract,
    operation: P6X86_64BaselineInt32RelationalOperation,
) -> Result<Vec<P6X86_64BaselineMachineInstruction>, P6X86_64BaselineInstructionSelectionError> {
    if let Some(actual) = instruction.arithmetic_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedArithmeticPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.bitwise_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedBitwisePolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }
    if let Some(actual) = instruction.equality_exit_policy {
        return Err(
            P6X86_64BaselineInstructionSelectionError::UnexpectedEqualityPolicy {
                bytecode_index: instruction.lowered.bytecode_index,
                actual,
            },
        );
    }

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
    let policy = instruction.relational_exit_policy.ok_or(
        P6X86_64BaselineInstructionSelectionError::MissingRelationalPolicy {
            bytecode_index: instruction.lowered.bytecode_index,
        },
    )?;
    p6_validate_relational_policy(instruction.lowered.bytecode_index, operation, policy)?;
    let non_int32_exit = p6_side_exit_label_from_contract(
        instruction.lowered.bytecode_index,
        P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
        policy.non_int32_exit,
    )?;

    Ok(vec![
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
        P6X86_64BaselineMachineInstruction::Int32RelationalToBoolean {
            operation,
            destination: P6X86_64BaselineSymbolicRegister::Scratch0,
            left: P6X86_64BaselineSymbolicRegister::Scratch0,
            right: P6X86_64BaselineSymbolicRegister::Scratch1,
            false_value: contract.value_layout.immediate_false_tag,
            true_value: contract.value_layout.immediate_true_tag,
        },
        P6X86_64BaselineMachineInstruction::StoreQ {
            destination,
            source: P6X86_64BaselineSymbolicRegister::Scratch0,
        },
    ])
}

fn p6_validate_arithmetic_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_operation: P6X86_64BaselineInt32ArithmeticOperation,
    expected_checked_arithmetic: P6X86_64BaselineCheckedInt32Arithmetic,
    actual: P6X86_64BaselineInt32ArithmeticExitPolicy,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    if actual.operation != expected_operation
        || actual.operand_guard
            != P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithNumberTagsAfterInt32FastPath
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

fn p6_validate_bitwise_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_operation: P6X86_64BaselineInt32BitwiseOperation,
    actual: P6X86_64BaselineInt32BitwiseExitPolicy,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    if actual.operation != expected_operation
        || actual.operand_guard != P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::BitwisePolicyMismatch {
                bytecode_index,
                expected_operation,
                actual,
            },
        );
    }

    Ok(())
}

fn p6_validate_equality_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_operation: P6X86_64BaselineInt32EqualityOperation,
    expected_fast_path: P6X86_64BaselineEqualityFastPath,
    actual: P6X86_64BaselineInt32EqualityExitPolicy,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    if actual.operation != expected_operation
        || actual.fast_path != expected_fast_path
        || actual.operand_guard != P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::EqualityPolicyMismatch {
                bytecode_index,
                expected_operation,
                actual,
            },
        );
    }

    Ok(())
}

fn p6_validate_relational_policy(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_operation: P6X86_64BaselineInt32RelationalOperation,
    actual: P6X86_64BaselineInt32RelationalExitPolicy,
) -> Result<(), P6X86_64BaselineInstructionSelectionError> {
    if actual.operation != expected_operation
        || actual.operand_guard != P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
    {
        return Err(
            P6X86_64BaselineInstructionSelectionError::RelationalPolicyMismatch {
                bytecode_index,
                expected_operation,
                actual,
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
            P6X86_64BaselineArithmeticSideExitReason::UnsupportedStrictEqualityOperand => {
                P6X86_64BaselineSelectedSideExitReason::UnsupportedStrictEqualityOperand
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

fn p6_primitive_to_number_side_exit_label_from_contract(
    bytecode_index: crate::bytecode::BytecodeIndex,
    expected_reason: P6X86_64BaselinePrimitiveToNumberSideExitReason,
    actual: P6X86_64BaselinePrimitiveToNumberSideExitContract,
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
            P6X86_64BaselineInstructionSelectionError::PrimitiveToNumberSideExitContractMismatch {
                bytecode_index,
                expected_reason,
                actual,
            },
        );
    }

    Ok(P6X86_64BaselineSideExitLabel {
        reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedToNumberOperand,
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
        (
            P6X86_64BaselineSelectionOperandAccess::Source,
            P6X86_64BaselineOperandLocation::CallFrameCalleeValue,
        ) => Ok(P6X86_64BaselineMachineOperand::Register(
            P6X86_64BaselineSymbolicRegister::PinnedCalleeValue,
        )),
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

fn p6_primitive_to_number_undefined_nan_value(
    value_layout: P6X86_64BaselineValueLayoutContract,
) -> u64 {
    const JSC_PURE_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;
    (JSC_PURE_NAN_BITS & !value_layout.tag_mask) | value_layout.double_tag
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
    let unlinked_root_maps_match_runtime_helper_shape =
        p6_x86_64_unlinked_root_maps_match_linked_runtime_helper_root_maps(
            code_block,
            runtime_helper_proof_count,
        );
    if shape.proof_safepoint_count == runtime_helper_proof_count
        && shape.proof_complete_safepoint_root_map_count == runtime_helper_proof_count
        && shape.proof_root_map_count == runtime_helper_proof_count
        && shape.proof_exception_metadata
            == (BaselineExceptionMetadataPresence::Present { handler_count: 0 })
        && shape.code_block_linked_root_map_count == runtime_helper_proof_count
        && unlinked_root_maps_match_runtime_helper_shape
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

fn p6_x86_64_unlinked_root_maps_match_linked_runtime_helper_root_maps(
    code_block: &CodeBlock,
    runtime_helper_proof_count: usize,
) -> bool {
    let unlinked_root_maps = &code_block.unlinked().side_tables().root_maps;
    if unlinked_root_maps.is_empty() {
        return true;
    }
    let linked_root_maps = &code_block.side_tables().root_maps;
    if unlinked_root_maps.len() != runtime_helper_proof_count
        || linked_root_maps.len() != runtime_helper_proof_count
    {
        return false;
    }

    unlinked_root_maps
        .iter()
        .zip(linked_root_maps)
        .all(|(unlinked, linked)| {
            unlinked.id == linked.id
                && (unlinked.owner.is_none() || unlinked.owner == linked.owner)
                && linked.owner.is_some()
                && unlinked.bytecode_range_start == linked.bytecode_range_start
                && unlinked.bytecode_range_end == linked.bytecode_range_end
                && unlinked.slots == linked.slots
                && unlinked.complete == linked.complete
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
                // FIX 2 reconciliation: the emit-time `property_site_index` baked into
                // the DataIC machine code MUST equal the writeback's positional index
                // over the plan's `sites` (`property_site_index_for_bytecode_index`).
                // Both now filter decoded instructions in bytecode order by THE same
                // canonical predicate, so they are provably the identical enumeration.
                // Assert it here so any future predicate/order drift fails loudly in
                // debug instead of reading/writing the wrong resident record.
                debug_assert_eq!(
                    property_native_exit_plan.and_then(|plan| plan
                        .borrowed_plan()
                        .property_site_index_for_bytecode_index(instruction.bytecode_index)),
                    Some(property_site_index as usize),
                    "emit-index != writeback-index for property-handoff site at {:?}",
                    instruction.bytecode_index,
                );
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
        CoreOpcode::LoopHint => {
            validate_p6_operand_count(instruction, opcode, 0)?;
            // C++ baseline JIT's emit_op_loop_hint increments optimization
            // counters and may enter the slow optimizer path at this bytecode
            // index. Rust lowers the hint as a native no-op for now: the
            // generated-body executor reports LoopHint telemetry through
            // BaselineGeneratedExecutionMetrics, while P14 backward-branch
            // safepoints remain a separate native reentry mechanism rather
            // than the JSC baseline LoopHint counter.
            P6X86_64BaselineLoweredOperation::LoopHint
        }
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
        CoreOpcode::LoadCallee => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            P6X86_64BaselineLoweredOperation::LoadCallee {
                destination: p6_register_operand(instruction, opcode, 0)?,
            }
        }
        CoreOpcode::Move => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            P6X86_64BaselineLoweredOperation::Move {
                destination: p6_register_operand(instruction, opcode, 0)?,
                source: p6_register_operand(instruction, opcode, 1)?,
            }
        }
        CoreOpcode::ToNumber => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            P6X86_64BaselineLoweredOperation::ToNumberPrimitive {
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
        CoreOpcode::BitAndInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::BitAndInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::BitOrInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::BitOrInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::BitXorInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::BitXorInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::RightShiftInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::RightShiftInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::Equal => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            if p6_opcode_subset_uses_no_call_loose_equality(opcode_subset) {
                P6X86_64BaselineLoweredOperation::EqualNoCallLoose {
                    destination,
                    left,
                    right,
                }
            } else {
                P6X86_64BaselineLoweredOperation::EqualInt32 {
                    destination,
                    left,
                    right,
                }
            }
        }
        CoreOpcode::NotEqual => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            if p6_opcode_subset_uses_no_call_loose_equality(opcode_subset) {
                P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose {
                    destination,
                    left,
                    right,
                }
            } else {
                P6X86_64BaselineLoweredOperation::NotEqualInt32 {
                    destination,
                    left,
                    right,
                }
            }
        }
        CoreOpcode::StrictEqual => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::StrictEqualPrimitive {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::StrictNotEqual => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::StrictNotEqualPrimitive {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::LessThanInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::LessThanInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::LessEqualInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::LessEqualInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::GreaterThanInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::GreaterThanInt32 {
                destination,
                left,
                right,
            }
        }
        CoreOpcode::GreaterEqualInt32 => {
            let (destination, left, right) = lower_p6_int32_binary_operands(instruction, opcode)?;
            P6X86_64BaselineLoweredOperation::GreaterEqualInt32 {
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
            | CoreOpcode::LoadCapture
            | CoreOpcode::NewClosureCell
            | CoreOpcode::GetClosureCell
            | CoreOpcode::PutClosureCell
            | CoreOpcode::ArrayAppend
            | CoreOpcode::TypeOf
            | CoreOpcode::Throw
    )
}

const fn p9_x86_64_js_call_native_exit_opcode(opcode: CoreOpcode) -> bool {
    matches!(
        opcode,
        CoreOpcode::Call | CoreOpcode::CallWithThis | CoreOpcode::Construct
    )
}

// FIX 2: delegate to THE canonical property-handoff predicate so the emitter's
// `property_site_index` counter enumerates the EXACT same set, in the same bytecode
// order, as the plan's `sites` (record-store size + writeback positional index).
// Previously this list omitted GetLength while the plan included it, so any DataIC
// site that followed a GetLength in bytecode order baked record_index = i but the
// writeback wrote records[i+1] -> wrong value / segfault.
const fn p10_x86_64_property_native_exit_opcode(opcode: CoreOpcode) -> bool {
    baseline_opcode_is_generated_property_handoff(opcode)
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
    let expected_operand_count = if matches!(
        opcode,
        CoreOpcode::GetGlobalObjectProperty | CoreOpcode::PutGlobalObjectProperty
    ) {
        2
    } else {
        3
    };
    validate_p6_operand_count(instruction, opcode, expected_operand_count)?;
    let operands = match opcode {
        CoreOpcode::GetByName => {
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let base = p6_register_operand(instruction, opcode, 1)?;
            let identifier_index = p6_identifier_index_operand(instruction, opcode, 2)?;
            let property_key = PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(identifier_index),
            ));
            if site.property_key != PropertyCacheKey::Key(property_key) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::GetByName { destination, base }
        }
        CoreOpcode::GetLength => {
            // Same `[dest, base, identifier]` shape as GetByName. Kept on the slow
            // path (the contract binds no operand locations for GetLength), but it
            // must be a valid p10 site so the canonical `property_site_index`
            // enumeration matches the plan's (FIX 2).
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let base = p6_register_operand(instruction, opcode, 1)?;
            let identifier_index = p6_identifier_index_operand(instruction, opcode, 2)?;
            let property_key = PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(identifier_index),
            ));
            if site.property_key != PropertyCacheKey::Key(property_key) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::GetLength { destination, base }
        }
        CoreOpcode::GetGlobalObjectProperty => {
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let identifier_index = p6_identifier_index_operand(instruction, opcode, 1)?;
            let property_key = PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(identifier_index),
            ));
            if site.property_key != PropertyCacheKey::Key(property_key) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::GetGlobalObjectProperty { destination }
        }
        CoreOpcode::PutByName => {
            let base = p6_register_operand(instruction, opcode, 0)?;
            let identifier_index = p6_identifier_index_operand(instruction, opcode, 1)?;
            let value = p6_register_operand(instruction, opcode, 2)?;
            let property_key = PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(identifier_index),
            ));
            if site.property_key != PropertyCacheKey::Key(property_key) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::PutByName { base, value }
        }
        CoreOpcode::PutGlobalObjectProperty => {
            let identifier_index = p6_identifier_index_operand(instruction, opcode, 0)?;
            let value = p6_register_operand(instruction, opcode, 1)?;
            let property_key = PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(identifier_index),
            ));
            if site.property_key != PropertyCacheKey::Key(property_key) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::PutGlobalObjectProperty { value }
        }
        CoreOpcode::GetByValue => {
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let base = p6_register_operand(instruction, opcode, 1)?;
            let property = p6_register_operand(instruction, opcode, 2)?;
            if site.property_key != PropertyCacheKey::RuntimeValue(property) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::GetByValue {
                destination,
                base,
                property,
            }
        }
        CoreOpcode::PutByValue => {
            let base = p6_register_operand(instruction, opcode, 0)?;
            let property = p6_register_operand(instruction, opcode, 1)?;
            let value = p6_register_operand(instruction, opcode, 2)?;
            if site.property_key != PropertyCacheKey::RuntimeValue(property) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::PutByValue {
                base,
                property,
                value,
            }
        }
        CoreOpcode::InById => {
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let base = p6_register_operand(instruction, opcode, 1)?;
            let identifier_index = p6_identifier_index_operand(instruction, opcode, 2)?;
            let property_key = PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(identifier_index),
            ));
            if site.property_key != PropertyCacheKey::Key(property_key) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::InById { destination, base }
        }
        CoreOpcode::InByVal => {
            let destination = p6_register_operand(instruction, opcode, 0)?;
            let base = p6_register_operand(instruction, opcode, 1)?;
            let property = p6_register_operand(instruction, opcode, 2)?;
            if site.property_key != PropertyCacheKey::RuntimeValue(property) {
                return Err(P6X86_64BaselineLoweringError::UnsupportedOpcode {
                    bytecode_index: instruction.bytecode_index,
                    opcode: instruction.opcode,
                    core_opcode: Some(opcode),
                });
            }
            P10X86_64BaselinePropertyNativeExitOperands::InByVal {
                destination,
                base,
                property,
            }
        }
        _ => unreachable!(),
    };
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
        CoreOpcode::Call | CoreOpcode::Construct => (None, 2usize, 3usize),
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
        CoreOpcode::LoadCapture => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let _destination = p6_register_operand(instruction, opcode, 0)?;
            let _capture_index = p6_unsigned_immediate_operand(instruction, opcode, 1)?;
        }
        CoreOpcode::NewClosureCell | CoreOpcode::GetClosureCell => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let _destination = p6_register_operand(instruction, opcode, 0)?;
            let _source = p6_register_operand(instruction, opcode, 1)?;
        }
        CoreOpcode::PutClosureCell => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let _cell = p6_register_operand(instruction, opcode, 0)?;
            let _value = p6_register_operand(instruction, opcode, 1)?;
        }
        CoreOpcode::ArrayAppend => {
            validate_p6_operand_count(instruction, opcode, 2)?;
            let _array = p6_register_operand(instruction, opcode, 0)?;
            let _value = p6_register_operand(instruction, opcode, 1)?;
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
        CoreOpcode::Throw => {
            validate_p6_operand_count(instruction, opcode, 1)?;
            let _source = p6_register_operand(instruction, opcode, 0)?;
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

        if !operations
            .iter()
            .any(|candidate| candidate.bytecode_index == target)
        {
            return Err(P6X86_64BaselineLoweringError::InvalidBranchTarget {
                bytecode_index,
                opcode,
                target,
                reason: P6X86_64BaselineBranchTargetRejectionReason::SparseInstructionStart,
            });
        };
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

    fn p6_load_callee_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(0, CoreOpcode::LoadCallee, vec![Operand::Register(local(0))]),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    fn p6_to_number_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadBool,
                vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
            ),
            typed_instruction(
                1,
                CoreOpcode::ToNumber,
                vec![Operand::Register(local(1)), Operand::Register(local(0))],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p6_equality_code_block(opcode: CoreOpcode, left: i32, right: i32) -> CodeBlock {
        assert!(matches!(opcode, CoreOpcode::Equal | CoreOpcode::NotEqual));
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(left)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(right)],
            ),
            typed_instruction(
                2,
                opcode,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ])
    }

    fn p6_strict_equality_code_block(opcode: CoreOpcode, left: i32, right: i32) -> CodeBlock {
        assert!(matches!(
            opcode,
            CoreOpcode::StrictEqual | CoreOpcode::StrictNotEqual
        ));
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(left)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(right)],
            ),
            typed_instruction(
                2,
                opcode,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ])
    }

    fn p6_bitwise_code_block(opcode: CoreOpcode, left: i32, right: i32) -> CodeBlock {
        assert!(matches!(
            opcode,
            CoreOpcode::BitAndInt32
                | CoreOpcode::BitOrInt32
                | CoreOpcode::BitXorInt32
                | CoreOpcode::RightShiftInt32
        ));
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(left)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(right)],
            ),
            typed_instruction(
                2,
                opcode,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ])
    }

    fn p6_relational_code_block(opcode: CoreOpcode, left: i32, right: i32) -> CodeBlock {
        assert!(matches!(
            opcode,
            CoreOpcode::LessThanInt32
                | CoreOpcode::LessEqualInt32
                | CoreOpcode::GreaterThanInt32
                | CoreOpcode::GreaterEqualInt32
        ));
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(left)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(right)],
            ),
            typed_instruction(
                2,
                opcode,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ])
    }

    fn p8b_not_equal_jump_if_false_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(5)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(5)],
            ),
            typed_instruction(
                2,
                CoreOpcode::NotEqual,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(
                3,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(2)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(6)),
                ],
            ),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(11)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(3))]),
            typed_instruction(
                6,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(7, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ])
    }

    fn p8b_relational_jump_if_false_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(5)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(5)],
            ),
            typed_instruction(
                2,
                CoreOpcode::LessThanInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(
                3,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(2)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(6)),
                ],
            ),
            typed_instruction(
                4,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(11)],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(3))]),
            typed_instruction(
                6,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(42)],
            ),
            typed_instruction(7, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ])
    }

    fn p8b_mixed_relational_no_call_loose_equality_jump_if_false_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(5)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(1)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                2,
                CoreOpcode::LessThanInt32,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(
                3,
                CoreOpcode::NotEqual,
                vec![
                    Operand::Register(local(3)),
                    Operand::Register(local(0)),
                    Operand::Register(local(1)),
                ],
            ),
            typed_instruction(
                4,
                CoreOpcode::JumpIfFalse,
                vec![
                    Operand::Register(local(2)),
                    Operand::BytecodeIndex(BytecodeIndex::from_offset(7)),
                ],
            ),
            typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(3))]),
            typed_instruction(6, CoreOpcode::Return, vec![Operand::Register(local(2))]),
            typed_instruction(7, CoreOpcode::Return, vec![Operand::Register(local(1))]),
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
            typed_instruction(
                3,
                CoreOpcode::Construct,
                vec![
                    Operand::Register(local(3)),
                    Operand::Register(argument_including_this(1)),
                    Operand::UnsignedImmediate(1),
                    Operand::Register(argument_including_this(3)),
                ],
            ),
            typed_instruction(4, CoreOpcode::Return, vec![Operand::Register(local(3))]),
        ])
    }

    // CallWithThis whose RESULT destination is an argument-including-this slot
    // (not a frame local). richards keeps hot call results in argument-classified
    // slots, so the owner-post-call-reentry stub must be emitted for this shape,
    // exactly as C++ JSC emitPutCallResult stores to the dst VirtualRegister's
    // slot regardless of class. Destination argument_including_this(2),
    // callee argument_including_this(1), this argument_including_this(2)'s base
    // expressed via distinct argument slots for callee/this/args.
    fn p9_call_with_this_argument_destination_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::CallWithThis,
                vec![
                    Operand::Register(argument_including_this(2)),
                    Operand::Register(argument_including_this(1)),
                    Operand::Register(argument_including_this(2)),
                    Operand::UnsignedImmediate(2),
                    Operand::Register(argument_including_this(3)),
                    Operand::Register(argument_including_this(4)),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(0))]),
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

    // GetByName whose base is a plain frame LOCAL (not an argument), so the
    // self-load DataIC fast path is admitted. Destination local(2), base local(1).
    fn p10_get_by_name_self_load_frame_local_code_block() -> CodeBlock {
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
                    Operand::Register(local(2)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(2))]),
        ])
    }

    fn p10_get_global_object_property_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(7)],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetGlobalObjectProperty,
                vec![Operand::Register(local(1)), Operand::IdentifierIndex(11)],
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

    fn p10_get_by_value_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(0)],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetByValue,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(argument_including_this(1)),
                    Operand::Register(local(3)),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p10_put_by_value_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(0)), Operand::SignedImmediate(43)],
            ),
            typed_instruction(
                1,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(0)],
            ),
            typed_instruction(
                2,
                CoreOpcode::PutByValue,
                vec![
                    Operand::Register(argument_including_this(1)),
                    Operand::Register(local(3)),
                    Operand::Register(local(0)),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(0))]),
        ])
    }

    fn p10_in_by_id_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::InById,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(argument_including_this(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            typed_instruction(1, CoreOpcode::Return, vec![Operand::Register(local(1))]),
        ])
    }

    fn p10_in_by_val_native_exit_code_block() -> CodeBlock {
        code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::LoadInt32,
                vec![Operand::Register(local(3)), Operand::SignedImmediate(0)],
            ),
            typed_instruction(
                1,
                CoreOpcode::InByVal,
                vec![
                    Operand::Register(local(1)),
                    Operand::Register(argument_including_this(1)),
                    Operand::Register(local(3)),
                ],
            ),
            typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(local(1))]),
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

    fn p6_equality_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality,
        )
    }

    fn p6_bitwise_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOr,
        )
    }

    fn p8b_equality_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse,
        )
    }

    fn p6_no_call_loose_equality_lowering_proof(
        code_block: &CodeBlock,
    ) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality,
        )
    }

    fn p6_strict_equality_lowering_proof(
        code_block: &CodeBlock,
    ) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean,
        )
    }

    fn p6_primitive_to_number_lowering_proof(
        code_block: &CodeBlock,
    ) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber,
        )
    }

    fn p8b_no_call_loose_equality_lowering_proof(
        code_block: &CodeBlock,
    ) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse,
        )
    }

    fn p8b_no_call_loose_relational_lowering_proof(
        code_block: &CodeBlock,
    ) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse,
        )
    }

    fn p6_relational_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational,
        )
    }

    fn p8b_relational_lowering_proof(code_block: &CodeBlock) -> BaselineBytecodeEligibilityProof {
        lowering_proof_for_code_block(
            code_block,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse,
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

    fn p9_callable_semantic_emission_with_owner_map_for_code_block(
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
        let owner_map =
            crate::jit::plan::derive_baseline_generated_owner_continuation_map_from_code_block(
                code_block,
                owner(),
            )
            .unwrap()
            .metadata
            .expect("owner continuation metadata");
        let lowering = plan_p6_x86_64_baseline_lowering_with_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), code_block, proof),
            None,
            Some(&js_call_plan),
            None,
        )
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        emit_p6_x86_64_baseline_callable_semantic_bytes_with_owner_continuation_map(
            contract,
            selection,
            Some(&owner_map),
        )
    }

    fn p9_owner_post_call_stub_resume_target_for_test(
        stub: &P9X86_64BaselineOwnerCallPostCallStubRecord,
    ) -> u32 {
        let rel32_start = (stub.resume_jump_offset - stub.start_offset + 1) as usize;
        let rel32_end = rel32_start + 4;
        let displacement = i32::from_le_bytes(
            stub.bytes[rel32_start..rel32_end]
                .try_into()
                .expect("rel32 bytes"),
        );
        u32::try_from(i64::from(stub.resume_jump_end_offset) + i64::from(displacement))
            .expect("resume target offset")
    }

    fn p9_owner_post_call_reentry_stub_target_for_test(
        stub: &P9X86_64BaselineOwnerCallPostCallReentryStubRecord,
    ) -> u32 {
        let rel32_start = (stub.post_call_jump_offset - stub.start_offset + 1) as usize;
        let rel32_end = rel32_start + 4;
        let displacement = i32::from_le_bytes(
            stub.bytes[rel32_start..rel32_end]
                .try_into()
                .expect("rel32 bytes"),
        );
        u32::try_from(i64::from(stub.post_call_jump_end_offset) + i64::from(displacement))
            .expect("post-call target offset")
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

    fn frame_argument_location(
        index: u32,
        frame_local_count: u32,
    ) -> P6X86_64BaselineOperandLocation {
        let byte_offset_from_argument_base = u64::from(index) * 8;
        P6X86_64BaselineOperandLocation::FrameArgument {
            argument_index_including_this: index,
            raw_virtual_register: argument_including_this(index).raw(),
            byte_offset_from_argument_base,
            byte_offset_from_frame_base: u64::from(frame_local_count) * 8
                + byte_offset_from_argument_base,
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
        let non_number_exit = side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            bytecode_offset,
        );
        let overflow_exit = side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::Overflow,
            bytecode_offset,
        );
        vec![
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                source: frame_memory_operand(left),
            },
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch1,
                source: frame_memory_operand(right),
            },
            P6X86_64BaselineMachineInstruction::Int32OrNumberArithmetic {
                operation,
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                tag_mask: contract.value_layout.tag_mask,
                payload_shift: contract.value_layout.payload_shift,
                int32_tag: contract.value_layout.immediate_int32_tag,
                double_tag: contract.value_layout.immediate_double_tag,
                non_number_exit,
                overflow_exit,
                negative_zero_exit: (operation == P6X86_64BaselineInt32ArithmeticOperation::Mul)
                    .then_some(side_exit_label(
                        P6X86_64BaselineSelectedSideExitReason::NegativeZero,
                        bytecode_offset,
                    )),
            },
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: frame_memory_operand(destination),
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
        ]
    }

    fn expected_int32_equality_selection(
        contract: &P6X86_64BaselineBackendContractRecord,
        bytecode_offset: u32,
        operation: P6X86_64BaselineInt32EqualityOperation,
        destination: u32,
        left: u32,
        right: u32,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        let non_int32_exit = side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            bytecode_offset,
        );
        vec![
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
            P6X86_64BaselineMachineInstruction::Int32EqualityToBoolean {
                operation,
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                false_value: contract.value_layout.immediate_false_tag,
                true_value: contract.value_layout.immediate_true_tag,
            },
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: frame_memory_operand(destination),
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
        ]
    }

    fn expected_no_call_loose_equality_selection(
        contract: &P6X86_64BaselineBackendContractRecord,
        bytecode_offset: u32,
        operation: P6X86_64BaselineInt32EqualityOperation,
        destination: u32,
        left: u32,
        right: u32,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        vec![
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                source: frame_memory_operand(left),
            },
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch1,
                source: frame_memory_operand(right),
            },
            P6X86_64BaselineMachineInstruction::NoCallLooseEqualityToBoolean {
                operation,
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                undefined_tag: contract.value_layout.immediate_undefined_tag,
                null_tag: contract.value_layout.immediate_null_tag,
                false_tag: contract.value_layout.immediate_false_tag,
                true_tag: contract.value_layout.immediate_true_tag,
                int32_tag: contract.value_layout.immediate_int32_tag,
                cell_tag: contract.value_layout.cell_tag,
                unsupported_exit: side_exit_label(
                    P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
                    bytecode_offset,
                ),
                false_value: contract.value_layout.immediate_false_tag,
                true_value: contract.value_layout.immediate_true_tag,
            },
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: frame_memory_operand(destination),
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
        ]
    }

    fn expected_int32_relational_selection(
        contract: &P6X86_64BaselineBackendContractRecord,
        bytecode_offset: u32,
        operation: P6X86_64BaselineInt32RelationalOperation,
        destination: u32,
        left: u32,
        right: u32,
    ) -> Vec<P6X86_64BaselineMachineInstruction> {
        let non_int32_exit = side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            bytecode_offset,
        );
        vec![
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
            P6X86_64BaselineMachineInstruction::Int32RelationalToBoolean {
                operation,
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                left: P6X86_64BaselineSymbolicRegister::Scratch0,
                right: P6X86_64BaselineSymbolicRegister::Scratch1,
                false_value: contract.value_layout.immediate_false_tag,
                true_value: contract.value_layout.immediate_true_tag,
            },
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: frame_memory_operand(destination),
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
        ]
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
    fn p6_bitwise_lowering_selects_and_emits_right_shift_int32_fast_path() {
        let code_block = p6_bitwise_code_block(CoreOpcode::RightShiftInt32, -8, 33);
        let proof = p6_bitwise_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOr
        );
        assert_eq!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::RightShiftInt32 {
                destination: local(2),
                left: local(0),
                right: local(1),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let policy = contract.instructions[2]
            .bitwise_exit_policy
            .expect("right shift bitwise exit policy");
        assert_eq!(
            policy.operation,
            P6X86_64BaselineInt32BitwiseOperation::RightShift
        );
        assert_eq!(
            policy.operand_guard,
            P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
        );
        assert_exit_contract(
            policy.non_int32_exit,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
            BytecodeIndex::from_offset(2),
        );

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert!(selection.instructions[2]
            .machine_instructions
            .iter()
            .any(|instruction| matches!(
                instruction,
                P6X86_64BaselineMachineInstruction::Int32Bitwise {
                    operation: P6X86_64BaselineInt32BitwiseOperation::RightShift,
                    destination: P6X86_64BaselineSymbolicRegister::Scratch2,
                    left: P6X86_64BaselineSymbolicRegister::Scratch0,
                    right: P6X86_64BaselineSymbolicRegister::Scratch1,
                }
            )));

        let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrSubset
        );
        let bytes = &emission.instruction_bytes[2].bytes;
        assert!(
            bytes.windows(3).any(|window| window == [0x83, 0xe1, 0x1f]),
            "RightShiftInt32 must mask the shift count with 31 before sar"
        );
        assert!(
            bytes.windows(3).any(|window| window == [0x41, 0xd3, 0xfa]),
            "RightShiftInt32 must emit arithmetic shift-right on the int32 payload"
        );
        assert!(
            bytes.windows(3).any(|window| window == [0x44, 0x89, 0xd1]),
            "RightShiftInt32 must move the shifted payload into the retag carrier"
        );
    }

    #[test]
    fn p6_bitwise_lowering_selects_and_emits_bitxor_int32_fast_path() {
        let code_block = p6_bitwise_code_block(CoreOpcode::BitXorInt32, 7, 3);
        let proof = p6_bitwise_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOr
        );
        assert_eq!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::BitXorInt32 {
                destination: local(2),
                left: local(0),
                right: local(1),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let policy = contract.instructions[2]
            .bitwise_exit_policy
            .expect("bitxor bitwise exit policy");
        assert_eq!(
            policy.operation,
            P6X86_64BaselineInt32BitwiseOperation::BitXor
        );
        assert_eq!(
            policy.operand_guard,
            P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
        );
        assert_exit_contract(
            policy.non_int32_exit,
            P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
            BytecodeIndex::from_offset(2),
        );

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert!(selection.instructions[2]
            .machine_instructions
            .iter()
            .any(|instruction| matches!(
                instruction,
                P6X86_64BaselineMachineInstruction::Int32Bitwise {
                    operation: P6X86_64BaselineInt32BitwiseOperation::BitXor,
                    destination: P6X86_64BaselineSymbolicRegister::Scratch2,
                    left: P6X86_64BaselineSymbolicRegister::Scratch0,
                    right: P6X86_64BaselineSymbolicRegister::Scratch1,
                }
            )));

        let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrSubset
        );
        let bytes = &emission.instruction_bytes[2].bytes;
        assert!(
            bytes.windows(3).any(|window| window == [0x44, 0x31, 0xd9]),
            "BitXorInt32 must emit xor on int32 payloads"
        );
    }

    #[test]
    fn p6_equality_lowering_selects_and_emits_int32_boolean_fast_path() {
        for (opcode, operation, branch_opcode) in [
            (
                CoreOpcode::Equal,
                P6X86_64BaselineInt32EqualityOperation::Equal,
                0x85,
            ),
            (
                CoreOpcode::NotEqual,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
                0x84,
            ),
        ] {
            let code_block = p6_equality_code_block(opcode, 7, 7);
            let proof = p6_equality_lowering_proof(&code_block);
            let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            ))
            .unwrap();

            assert_eq!(
                lowering.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrEqualitySubset
            );
            assert_eq!(
                lowering.plan.opcode_subset,
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEquality
            );
            let expected_lowered = match operation {
                P6X86_64BaselineInt32EqualityOperation::Equal => {
                    P6X86_64BaselineLoweredOperation::EqualInt32 {
                        destination: local(2),
                        left: local(0),
                        right: local(1),
                    }
                }
                P6X86_64BaselineInt32EqualityOperation::NotEqual => {
                    P6X86_64BaselineLoweredOperation::NotEqualInt32 {
                        destination: local(2),
                        left: local(0),
                        right: local(1),
                    }
                }
            };
            assert_eq!(lowering.plan.operations[2].operation, expected_lowered);

            let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
            let policy = contract.instructions[2]
                .equality_exit_policy
                .expect("int32 equality exit policy");
            assert_eq!(policy.operation, operation);
            assert_eq!(
                policy.operand_guard,
                P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
            );
            assert_exit_contract(
                policy.non_int32_exit,
                P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                BytecodeIndex::from_offset(2),
            );

            let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
            assert_eq!(
                selection.instructions[2].machine_instructions,
                expected_int32_equality_selection(&contract, 2, operation, 2, 0, 1)
            );

            let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
            assert_eq!(
                emission.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrEqualitySubset
            );
            let bytes = &emission.instruction_bytes[2].bytes;
            assert!(
                bytes.windows(3).any(|window| window == [0x45, 0x39, 0xda]),
                "{opcode:?} must emit cmp r11d, r10d before boxing the boolean"
            );
            assert!(
                bytes
                    .windows(2)
                    .any(|window| window == [0x0f, branch_opcode]),
                "{opcode:?} must invert the internal branch around true materialization"
            );
            assert!(
                bytes.windows(2).any(|window| window == [0x49, 0xba]),
                "{opcode:?} must materialize a boolean JSValue with movabs"
            );
        }

        let code_block = p6_equality_code_block(CoreOpcode::Equal, 1, 1);
        let error = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner(),
            lowering_snapshot(),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .expect_err("legacy P6 arithmetic subset must not advertise loose equality");
        assert!(matches!(
            error,
            JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                opcode: CoreOpcode::Equal,
                ..
            }
        ));
    }

    #[test]
    fn p6_no_call_loose_equality_lowering_selects_generated_no_call_fast_path() {
        for (opcode, operation, expected_lowered) in [
            (
                CoreOpcode::Equal,
                P6X86_64BaselineInt32EqualityOperation::Equal,
                P6X86_64BaselineLoweredOperation::EqualNoCallLoose {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
            ),
            (
                CoreOpcode::NotEqual,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
                P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
            ),
        ] {
            let code_block = p6_equality_code_block(opcode, 7, 7);
            let proof = p6_no_call_loose_equality_lowering_proof(&code_block);
            let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            ))
            .unwrap();

            assert_eq!(
                lowering.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualitySubset
            );
            assert_eq!(
                lowering.plan.opcode_subset,
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEquality
            );
            assert_eq!(lowering.plan.operations[2].operation, expected_lowered);

            let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
            let policy = contract.instructions[2]
                .equality_exit_policy
                .expect("no-call loose equality exit policy");
            assert_eq!(policy.operation, operation);
            assert_eq!(
                policy.fast_path,
                P6X86_64BaselineEqualityFastPath::GeneratedNoCallLoose
            );
            assert_exit_contract(
                policy.non_int32_exit,
                P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                BytecodeIndex::from_offset(2),
            );

            let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
            assert_eq!(
                selection.instructions[2].machine_instructions,
                expected_no_call_loose_equality_selection(&contract, 2, operation, 2, 0, 1)
            );

            let cell_tag = p6_x86_64_semantic_u8_tag(
                BytecodeIndex::from_offset(2),
                contract.value_layout.cell_tag,
            )
            .unwrap();
            let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
            assert_eq!(
                emission.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualitySubset
            );
            let bytes = &emission.instruction_bytes[2].bytes;
            assert!(
                bytes.windows(3).any(|window| window == [0x4d, 0x39, 0xda]),
                "{opcode:?} must compare full JSValue bits before exact non-double identity"
            );
            assert!(
                bytes
                    .windows(4)
                    .any(|window| window == [0x41, 0x80, 0xfa, cell_tag]),
                "{opcode:?} must accept exact same-cell identity"
            );
            assert!(emission.side_exit_placeholders.iter().any(|placeholder| {
                placeholder.bytecode_index == BytecodeIndex::from_offset(2)
                    && placeholder.reason == P6X86_64BaselineSelectedSideExitReason::NonInt32Operand
            }));
        }
    }

    #[test]
    fn p6_strict_equality_lowering_selects_primitive_no_double_fast_path() {
        for (opcode, operation, expected_lowered, expected_result) in [
            (
                CoreOpcode::StrictEqual,
                P6X86_64BaselineInt32EqualityOperation::Equal,
                P6X86_64BaselineLoweredOperation::StrictEqualPrimitive {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
                true,
            ),
            (
                CoreOpcode::StrictNotEqual,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
                P6X86_64BaselineLoweredOperation::StrictNotEqualPrimitive {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
                false,
            ),
        ] {
            let code_block = p6_strict_equality_code_block(opcode, 7, 7);
            let proof = p6_strict_equality_lowering_proof(&code_block);
            let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            ))
            .unwrap();

            assert_eq!(
                lowering.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanSubset
            );
            assert_eq!(
                lowering.plan.opcode_subset,
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBoolean
            );
            assert_eq!(lowering.plan.operations[2].operation, expected_lowered);

            let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
            let policy = contract.instructions[2]
                .equality_exit_policy
                .expect("strict equality exit policy");
            assert_eq!(policy.operation, operation);
            assert_eq!(
                policy.fast_path,
                P6X86_64BaselineEqualityFastPath::PrimitiveStrictNoDouble
            );
            assert_exit_contract(
                policy.non_int32_exit,
                P6X86_64BaselineArithmeticSideExitReason::UnsupportedStrictEqualityOperand,
                BytecodeIndex::from_offset(2),
            );
            let value_layout = contract.value_layout;

            let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
            assert!(matches!(
                selection.instructions[2].machine_instructions.as_slice(),
                [
                    P6X86_64BaselineMachineInstruction::LoadQ { .. },
                    P6X86_64BaselineMachineInstruction::LoadQ { .. },
                    P6X86_64BaselineMachineInstruction::PrimitiveStrictEqualityToBoolean {
                        operation: selected_operation,
                        unsupported_exit,
                        ..
                    },
                    P6X86_64BaselineMachineInstruction::StoreQ { .. },
                ] if *selected_operation == operation
                    && unsupported_exit.reason == P6X86_64BaselineSelectedSideExitReason::UnsupportedStrictEqualityOperand
            ));

            let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
            assert_eq!(
                emission.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitwiseRelationalJumpsPrimitiveTruthinessPrimitiveBooleanSubset
            );
            let bytes = &emission.instruction_bytes[2].bytes;
            assert!(
                bytes.windows(3).any(|window| window == [0x4d, 0x39, 0xda]),
                "{opcode:?} must compare raw JSValue bits after excluding double and unknown tags"
            );
            assert!(bytes.windows(10).any(|window| window[0..2] == [0x49, 0xba]
                && u64::from_le_bytes(window[2..10].try_into().unwrap())
                    == if expected_result {
                        value_layout.immediate_true_tag
                    } else {
                        value_layout.immediate_false_tag
                    }));
            assert!(emission.side_exit_placeholders.iter().any(|placeholder| {
                placeholder.bytecode_index == BytecodeIndex::from_offset(2)
                    && placeholder.reason
                        == P6X86_64BaselineSelectedSideExitReason::UnsupportedStrictEqualityOperand
            }));
        }

        let code_block = p6_strict_equality_code_block(CoreOpcode::StrictEqual, 1, 1);
        let error = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner(),
            lowering_snapshot(),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .expect_err("legacy P6 arithmetic subset must not advertise strict equality");
        assert!(matches!(
            error,
            JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                opcode: CoreOpcode::StrictEqual,
                ..
            }
        ));
    }

    #[test]
    fn p6_primitive_to_number_lowering_selects_and_emits_guarded_conversion() {
        let code_block = p6_to_number_code_block();
        let proof = p6_primitive_to_number_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumber
        );
        assert_eq!(
            lowering.plan.operations[1].operation,
            P6X86_64BaselineLoweredOperation::ToNumberPrimitive {
                destination: local(1),
                source: local(0),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let value_layout = contract.value_layout;
        let policy = contract.instructions[1]
            .primitive_to_number_exit_policy
            .expect("primitive ToNumber exit policy");
        assert_eq!(
            policy.unsupported_operand_exit.reason,
            P6X86_64BaselinePrimitiveToNumberSideExitReason::UnsupportedOperand
        );
        assert_eq!(
            policy.unsupported_operand_exit.destination,
            P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged
        );
        assert_eq!(
            policy.unsupported_operand_exit.retained_bytecode_index,
            BytecodeIndex::from_offset(1)
        );
        assert!(!policy.unsupported_operand_exit.may_throw);
        assert!(!policy.unsupported_operand_exit.runtime_call);
        assert!(!policy.unsupported_operand_exit.heap_allocation);
        assert!(!policy.unsupported_operand_exit.touches_gc_roots);

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert!(matches!(
            selection.instructions[1].machine_instructions.as_slice(),
            [
                P6X86_64BaselineMachineInstruction::LoadQ { .. },
                P6X86_64BaselineMachineInstruction::PrimitiveToNumber {
                    value: P6X86_64BaselineSymbolicRegister::Scratch0,
                    unsupported_exit,
                    nan_value,
                    ..
                },
                P6X86_64BaselineMachineInstruction::StoreQ { .. },
            ] if unsupported_exit.reason == P6X86_64BaselineSelectedSideExitReason::UnsupportedToNumberOperand
                && *nan_value == p6_primitive_to_number_undefined_nan_value(value_layout)
        ));

        let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.emitter_kind,
            BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalPrimitiveToNumberSubset
        );
        let bytes = &emission.instruction_bytes[1].bytes;
        let int32_tag = p6_x86_64_semantic_u8_tag(
            BytecodeIndex::from_offset(1),
            value_layout.immediate_int32_tag,
        )
        .unwrap();
        let double_tag =
            p6_x86_64_semantic_u8_tag(BytecodeIndex::from_offset(1), value_layout.double_tag)
                .unwrap();
        assert!(
            bytes
                .windows(4)
                .any(|window| window == [0x41, 0x80, 0xfa, int32_tag]),
            "ToNumber must pass through int32 values by tag"
        );
        assert!(
            bytes
                .windows(4)
                .any(|window| window == [0x41, 0x80, 0xfa, double_tag]),
            "ToNumber must pass through double values by tag"
        );
        assert!(
            bytes.windows(10).any(|window| window[0..2] == [0x49, 0xba]
                && u64::from_le_bytes(window[2..10].try_into().unwrap())
                    == p6_primitive_to_number_undefined_nan_value(value_layout)),
            "undefined must materialize JSC pure NaN bits"
        );
        assert!(emission.side_exit_placeholders.iter().any(|placeholder| {
            placeholder.bytecode_index == BytecodeIndex::from_offset(1)
                && placeholder.reason
                    == P6X86_64BaselineSelectedSideExitReason::UnsupportedToNumberOperand
        }));
    }

    #[test]
    fn p6_relational_lowering_selects_and_emits_int32_boolean_fast_path() {
        for (opcode, operation, false_branch_opcode, expected_lowered) in [
            (
                CoreOpcode::LessThanInt32,
                P6X86_64BaselineInt32RelationalOperation::LessThan,
                0x8d,
                P6X86_64BaselineLoweredOperation::LessThanInt32 {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
            ),
            (
                CoreOpcode::LessEqualInt32,
                P6X86_64BaselineInt32RelationalOperation::LessEqual,
                0x8f,
                P6X86_64BaselineLoweredOperation::LessEqualInt32 {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
            ),
            (
                CoreOpcode::GreaterThanInt32,
                P6X86_64BaselineInt32RelationalOperation::GreaterThan,
                0x8e,
                P6X86_64BaselineLoweredOperation::GreaterThanInt32 {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
            ),
            (
                CoreOpcode::GreaterEqualInt32,
                P6X86_64BaselineInt32RelationalOperation::GreaterEqual,
                0x8c,
                P6X86_64BaselineLoweredOperation::GreaterEqualInt32 {
                    destination: local(2),
                    left: local(0),
                    right: local(1),
                },
            ),
        ] {
            let code_block = p6_relational_code_block(opcode, -3, 7);
            let proof = p6_relational_lowering_proof(&code_block);
            let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                proof,
            ))
            .unwrap();

            assert_eq!(
                lowering.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrEqualityRelationalSubset
            );
            assert_eq!(
                lowering.plan.opcode_subset,
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelational
            );
            assert_eq!(lowering.plan.operations[2].operation, expected_lowered);

            let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
            let policy = contract.instructions[2]
                .relational_exit_policy
                .expect("int32 relational exit policy");
            assert_eq!(policy.operation, operation);
            assert_eq!(
                policy.operand_guard,
                P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithInt32Tag
            );
            assert_exit_contract(
                policy.non_int32_exit,
                P6X86_64BaselineArithmeticSideExitReason::NonInt32Operand,
                BytecodeIndex::from_offset(2),
            );

            let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
            assert_eq!(
                selection.instructions[2].machine_instructions,
                expected_int32_relational_selection(&contract, 2, operation, 2, 0, 1)
            );

            let emission = emit_p6_x86_64_baseline_semantic_bytes(contract, selection).unwrap();
            assert_eq!(
                emission.emitter_kind,
                BaselineMachineCodeEmitterKind::P6X86_64NoCallNoHeapBitAndOrEqualityRelationalSubset
            );
            let bytes = &emission.instruction_bytes[2].bytes;
            assert!(
                bytes.windows(3).any(|window| window == [0x45, 0x39, 0xda]),
                "{opcode:?} must emit a signed int32 cmp before boxing the boolean"
            );
            assert!(
                bytes
                    .windows(2)
                    .any(|window| window == [0x0f, false_branch_opcode]),
                "{opcode:?} must invert the signed internal branch around true materialization"
            );
            assert!(
                bytes.windows(2).any(|window| window == [0x49, 0xba]),
                "{opcode:?} must materialize a boolean JSValue with movabs"
            );
        }

        let code_block = p6_relational_code_block(CoreOpcode::LessThanInt32, 1, 2);
        let error = BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot(
            &code_block,
            owner(),
            lowering_snapshot(),
            BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
            Vec::new(),
        )
        .expect_err("legacy P6 arithmetic subset must not advertise int32 relational ops");
        assert!(matches!(
            error,
            JitPlanValidationError::BaselineEligibilityUnsupportedOpcode {
                opcode: CoreOpcode::LessThanInt32,
                ..
            }
        ));
    }

    #[test]
    fn p8b_equality_branch_lowering_feeds_not_equal_result_into_jump_if_false() {
        let code_block = p8b_not_equal_jump_if_false_code_block();
        let proof = p8b_equality_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrEqualityBranchTruthinessSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityBranchNullishFalse
        );
        assert_eq!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::NotEqualInt32 {
                destination: local(2),
                left: local(0),
                right: local(1),
            }
        );
        assert_eq!(
            lowering.plan.operations[3].operation,
            P6X86_64BaselineLoweredOperation::JumpIfFalse {
                source: local(2),
                target: BytecodeIndex::from_offset(6),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        assert_eq!(
            contract.instructions[3].branch_target,
            Some(P6X86_64BaselineControlFlowBranchContract {
                kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                source_bytecode_index: BytecodeIndex::from_offset(3),
                target_bytecode_index: BytecodeIndex::from_offset(6),
            })
        );
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert_eq!(
            selection.instructions[2].machine_instructions,
            expected_int32_equality_selection(
                &contract,
                2,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
                2,
                0,
                1,
            )
        );
        let branch_instructions = &selection.instructions[3].machine_instructions;
        assert_eq!(branch_instructions.len(), 2);
        assert!(matches!(
            &branch_instructions[0],
            P6X86_64BaselineMachineInstruction::LoadQ { .. }
        ));
        assert!(matches!(
            &branch_instructions[1],
            P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive { .. }
        ));

        let emission =
            emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrEqualityBranchTruthinessSubset
        );
        assert_eq!(emission.bytecode_branches.len(), 1);
        assert_eq!(
            emission.bytecode_branches[0].kind,
            P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
        );
        assert!(emission.side_exit_return_stubs.iter().any(|stub| {
            stub.bytecode_index == BytecodeIndex::from_offset(2)
                && stub.reason == P6X86_64BaselineSelectedSideExitReason::NonInt32Operand
        }));
        assert!(emission.side_exit_return_stubs.iter().any(|stub| {
            stub.bytecode_index == BytecodeIndex::from_offset(3)
                && stub.reason
                    == P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        }));
    }

    #[test]
    fn p8b_no_call_loose_equality_branch_feeds_result_into_jump_if_false() {
        let code_block = p8b_not_equal_jump_if_false_code_block();
        let proof = p8b_no_call_loose_equality_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityBranchTruthinessSubset
        );
        assert_eq!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose {
                destination: local(2),
                left: local(0),
                right: local(1),
            }
        );
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert_eq!(
            selection.instructions[2].machine_instructions,
            expected_no_call_loose_equality_selection(
                &contract,
                2,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
                2,
                0,
                1,
            )
        );

        let emission =
            emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityBranchTruthinessSubset
        );
        assert!(emission.side_exit_return_stubs.iter().any(|stub| {
            stub.bytecode_index == BytecodeIndex::from_offset(2)
                && stub.reason == P6X86_64BaselineSelectedSideExitReason::NonInt32Operand
        }));
        assert!(emission.side_exit_return_stubs.iter().any(|stub| {
            stub.bytecode_index == BytecodeIndex::from_offset(3)
                && stub.reason
                    == P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        }));
    }

    #[test]
    fn p8b_relational_branch_lowering_feeds_less_than_result_into_jump_if_false() {
        let code_block = p8b_relational_jump_if_false_code_block();
        let proof = p8b_relational_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrEqualityRelationalBranchTruthinessSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrEqualityRelationalBranchNullishFalse
        );
        assert_eq!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::LessThanInt32 {
                destination: local(2),
                left: local(0),
                right: local(1),
            }
        );
        assert_eq!(
            lowering.plan.operations[3].operation,
            P6X86_64BaselineLoweredOperation::JumpIfFalse {
                source: local(2),
                target: BytecodeIndex::from_offset(6),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        assert_eq!(
            contract.instructions[3].branch_target,
            Some(P6X86_64BaselineControlFlowBranchContract {
                kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                source_bytecode_index: BytecodeIndex::from_offset(3),
                target_bytecode_index: BytecodeIndex::from_offset(6),
            })
        );
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert_eq!(
            selection.instructions[2].machine_instructions,
            expected_int32_relational_selection(
                &contract,
                2,
                P6X86_64BaselineInt32RelationalOperation::LessThan,
                2,
                0,
                1,
            )
        );
        let branch_instructions = &selection.instructions[3].machine_instructions;
        assert_eq!(branch_instructions.len(), 2);
        assert!(matches!(
            &branch_instructions[0],
            P6X86_64BaselineMachineInstruction::LoadQ { .. }
        ));
        assert!(matches!(
            &branch_instructions[1],
            P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive { .. }
        ));

        let emission =
            emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrEqualityRelationalBranchTruthinessSubset
        );
        assert_eq!(emission.bytecode_branches.len(), 1);
        assert_eq!(
            emission.bytecode_branches[0].kind,
            P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
        );
        assert!(emission.side_exit_return_stubs.iter().any(|stub| {
            stub.bytecode_index == BytecodeIndex::from_offset(2)
                && stub.reason == P6X86_64BaselineSelectedSideExitReason::NonInt32Operand
        }));
        assert!(emission.side_exit_return_stubs.iter().any(|stub| {
            stub.bytecode_index == BytecodeIndex::from_offset(3)
                && stub.reason
                    == P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        }));
    }

    #[test]
    fn p8b_mixed_relational_and_no_call_loose_equality_keeps_relational_subset() {
        let code_block = p8b_mixed_relational_no_call_loose_equality_jump_if_false_code_block();
        let proof = p8b_no_call_loose_relational_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityRelationalBranchTruthinessSubset
        );
        assert_eq!(
            lowering.plan.opcode_subset,
            BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityRelationalBranchNullishFalse
        );
        assert_eq!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::LessThanInt32 {
                destination: local(2),
                left: local(0),
                right: local(1),
            }
        );
        assert_eq!(
            lowering.plan.operations[3].operation,
            P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose {
                destination: local(3),
                left: local(0),
                right: local(1),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let equality_policy = contract.instructions[3]
            .equality_exit_policy
            .expect("mixed no-call equality policy");
        assert_eq!(
            equality_policy.fast_path,
            P6X86_64BaselineEqualityFastPath::GeneratedNoCallLoose
        );
        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert_eq!(
            selection.instructions[2].machine_instructions,
            expected_int32_relational_selection(
                &contract,
                2,
                P6X86_64BaselineInt32RelationalOperation::LessThan,
                2,
                0,
                1,
            )
        );
        assert_eq!(
            selection.instructions[3].machine_instructions,
            expected_no_call_loose_equality_selection(
                &contract,
                3,
                P6X86_64BaselineInt32EqualityOperation::NotEqual,
                3,
                0,
                1,
            )
        );
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
    fn p8a_lowering_allows_branch_target_to_runtime_helper_native_exit() {
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

        let lowering = plan_p6_x86_64_baseline_lowering_with_runtime_helper_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), &code_block, proof),
            &runtime_helper_plan,
        )
        .expect("branch target to helper opcode label is valid");
        assert_eq!(lowering.plan.operations.len(), 3);
        assert!(matches!(
            lowering.plan.operations[0].operation,
            P6X86_64BaselineLoweredOperation::Jump { target }
                if target == helper_index
        ));
        assert!(matches!(
            lowering.plan.operations[1].operation,
            P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit {
                opcode: CoreOpcode::NewObject,
                ..
            }
        ));
    }

    #[test]
    fn p8b_no_call_loose_equality_branch_accepts_unlinked_runtime_helper_root_maps() {
        let helper_index = BytecodeIndex::from_offset(6);
        let root_map = BytecodeRootMap {
            id: BytecodeRootMapId(78),
            owner: None,
            bytecode_range_start: helper_index,
            bytecode_range_end: helper_index,
            slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                helper_index,
                local(4),
                BytecodeRootSlotKind::VirtualRegister,
            )],
            complete: true,
        };
        let code_block = code_block_from_typed_instructions_with_side_tables(
            vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(0)), Operand::SignedImmediate(5)],
                ),
                typed_instruction(
                    1,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(1)), Operand::SignedImmediate(5)],
                ),
                typed_instruction(
                    2,
                    CoreOpcode::NotEqual,
                    vec![
                        Operand::Register(local(2)),
                        Operand::Register(local(0)),
                        Operand::Register(local(1)),
                    ],
                ),
                typed_instruction(
                    3,
                    CoreOpcode::JumpIfFalse,
                    vec![
                        Operand::Register(local(2)),
                        Operand::BytecodeIndex(helper_index),
                    ],
                ),
                typed_instruction(
                    4,
                    CoreOpcode::LoadInt32,
                    vec![Operand::Register(local(3)), Operand::SignedImmediate(11)],
                ),
                typed_instruction(5, CoreOpcode::Return, vec![Operand::Register(local(3))]),
                typed_instruction(6, CoreOpcode::NewObject, vec![Operand::Register(local(4))]),
                typed_instruction(7, CoreOpcode::Return, vec![Operand::Register(local(4))]),
            ],
            UnlinkedSideTables {
                root_maps: vec![root_map],
                ..UnlinkedSideTables::default()
            },
        )
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
                BaselineSupportedOpcodeSubset::P8bConstantsMovesReturnInt32ArithmeticBitAndOrNoCallLooseEqualityBranchNullishFalse,
                derivation.safepoints,
                &runtime_helper_plan,
            )
            .unwrap();
        let shape =
            P6X86_64BaselineLoweringValidationShape::from_code_block_and_proof(&code_block, &proof);
        assert_eq!(shape.proof_root_map_count, 1);
        assert_eq!(shape.code_block_linked_root_map_count, 1);
        assert_eq!(shape.code_block_unlinked_root_map_count, 1);

        let lowering = plan_p6_x86_64_baseline_lowering_with_runtime_helper_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), &code_block, proof),
            &runtime_helper_plan,
        )
        .expect("unlinked generator root maps mirror linked helper root maps");
        assert_eq!(
            lowering.emitter_kind,
            BaselineMachineCodeEmitterKind::P8bX86_64NoCallNoHeapBitAndOrNoCallLooseEqualityBranchTruthinessSubset
        );
        assert!(matches!(
            lowering.plan.operations[2].operation,
            P6X86_64BaselineLoweredOperation::NotEqualNoCallLoose { .. }
        ));
        assert!(matches!(
            lowering.plan.operations[3].operation,
            P6X86_64BaselineLoweredOperation::JumpIfFalse { target, .. }
                if target == helper_index
        ));
        assert!(matches!(
            lowering.plan.operations[6].operation,
            P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit {
                opcode: CoreOpcode::NewObject,
                ..
            }
        ));
    }

    #[test]
    fn throw_lowers_to_runtime_helper_native_exit_with_source_root_map() {
        let throw_index = BytecodeIndex::from_offset(1);
        let thrown_value = local(0);
        let root_map = BytecodeRootMap {
            id: BytecodeRootMapId(79),
            owner: None,
            bytecode_range_start: throw_index,
            bytecode_range_end: throw_index,
            slots: vec![BytecodeRootSlotDescriptor::virtual_register(
                throw_index,
                thrown_value,
                BytecodeRootSlotKind::VirtualRegister,
            )],
            complete: true,
        };
        let code_block = code_block_from_typed_instructions_with_side_tables(
            vec![
                typed_instruction(
                    0,
                    CoreOpcode::LoadInt32,
                    vec![
                        Operand::Register(thrown_value),
                        Operand::SignedImmediate(17),
                    ],
                ),
                typed_instruction(1, CoreOpcode::Throw, vec![Operand::Register(thrown_value)]),
                typed_instruction(2, CoreOpcode::Return, vec![Operand::Register(thrown_value)]),
            ],
            UnlinkedSideTables {
                root_maps: vec![root_map],
                ..UnlinkedSideTables::default()
            },
        )
        .with_root_map_owner(owner());
        let derivation =
            crate::jit::plan::derive_baseline_generated_runtime_helper_plan_from_code_block(
                &code_block,
                owner(),
            )
            .unwrap();
        let runtime_helper_plan = derivation.metadata.expect("throw helper metadata");
        let proof =
            BaselineBytecodeEligibilityRecord::proof_from_code_block_snapshot_with_runtime_helpers(
                &code_block,
                owner(),
                lowering_snapshot(),
                BaselineSupportedOpcodeSubset::P6ConstantsMovesReturnInt32Arithmetic,
                derivation.safepoints,
                &runtime_helper_plan,
            )
            .unwrap();

        let lowering = plan_p6_x86_64_baseline_lowering_with_runtime_helper_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), &code_block, proof),
            &runtime_helper_plan,
        )
        .expect("throw is retained as a VM-owned native exit");
        assert_eq!(lowering.plan.operations.len(), 3);
        assert!(matches!(
            lowering.plan.operations[1].operation,
            P6X86_64BaselineLoweredOperation::RuntimeHelperNativeExit {
                opcode: CoreOpcode::Throw,
                root_map: BytecodeRootMapId(79),
                root_count: 1,
                may_throw: true,
                ..
            }
        ));
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
                    byte_offset_from_frame_base: u64::from(contract.frame_local_count) * 8 + 8,
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
    fn p6_load_callee_lowers_selects_and_emits_from_rust_callee_carrier() {
        let code_block = p6_load_callee_code_block();
        let proof = p6_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering(P6X86_64BaselineLoweringRequest::new(
            owner(),
            &code_block,
            proof,
        ))
        .unwrap();

        assert_eq!(
            lowering.plan.operations[0].operation,
            P6X86_64BaselineLoweredOperation::LoadCallee {
                destination: local(0),
            }
        );

        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        assert_eq!(
            contract.instructions[0].operand_locations,
            vec![
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Destination,
                    location: frame_local_location(0),
                },
                P6X86_64BaselineOperandLocationRecord {
                    role: P6X86_64BaselineOperandRole::Source,
                    location: P6X86_64BaselineOperandLocation::CallFrameCalleeValue,
                },
            ]
        );

        let selection = select_p6_x86_64_baseline_instructions(&contract).unwrap();
        assert_eq!(
            selection.instructions[0].machine_instructions,
            vec![
                P6X86_64BaselineMachineInstruction::MoveQ {
                    destination: P6X86_64BaselineMachineOperand::Register(
                        P6X86_64BaselineSymbolicRegister::Scratch0,
                    ),
                    source: P6X86_64BaselineMachineOperand::Register(
                        P6X86_64BaselineSymbolicRegister::PinnedCalleeValue,
                    ),
                },
                P6X86_64BaselineMachineInstruction::StoreQ {
                    destination: frame_memory_operand(0),
                    source: P6X86_64BaselineSymbolicRegister::Scratch0,
                },
            ]
        );

        let emission =
            emit_p6_x86_64_baseline_callable_semantic_bytes(contract, selection).unwrap();
        assert_eq!(
            emission.callable_prologue.as_ref().unwrap().bytes,
            P6_X86_64_CALLABLE_PROLOGUE_BYTES
        );
        assert_eq!(
            emission.instruction_bytes[0].bytes,
            vec![0x4d, 0x8b, 0xd1, 0x4c, 0x89, 0x95, 0, 0, 0, 0]
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
            P6X86_64BaselineInt32OperandGuard::GuardBothOperandsWithNumberTagsAfterInt32FastPath
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
        let entrypoints_before = code_block.entrypoints().clone();
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
        assert_eq!(code_block.entrypoints(), &entrypoints_before);
        assert_eq!(code_block.lifecycle(), lifecycle_before);
        assert!(code_block.entrypoints().baseline_jit().is_none());
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
        let entrypoints_before = code_block.entrypoints().clone();
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
        assert_eq!(code_block.entrypoints(), &entrypoints_before);
        assert_eq!(code_block.lifecycle(), lifecycle_before);
        assert!(code_block.entrypoints().baseline_jit().is_none());
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
                    frame_argument_location(1, contract.frame_local_count),
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

        assert!(selection.instructions[9]
            .machine_instructions
            .iter()
            .any(|instruction| matches!(
                instruction,
                P6X86_64BaselineMachineInstruction::Int32OrNumberArithmetic {
                    operation: P6X86_64BaselineInt32ArithmeticOperation::Mul,
                    negative_zero_exit: Some(exit),
                    ..
                } if *exit == side_exit_label(
                    P6X86_64BaselineSelectedSideExitReason::NegativeZero,
                    9,
                )
            )));

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
                AssemblerArchitecture::X86_64,
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
    fn p6_semantic_emitter_accepts_frame_argument_sources_and_rejects_constant_memory_sources() {
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
        p6_semantic_emission_for_code_block(&frame_argument_source).unwrap();

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
    fn p6_callable_prologue_epilogue_seed_r13_jit_data_register_and_stay_balanced() {
        // Pin the exact prologue/epilogue byte shape after adding the r13
        // (GPRInfo::jitDataRegister) seed. The prologue pushes rbp, r15, r12,
        // r13 in that order and seeds r13 from rcx (the 4th C-ABI arg = IC store
        // base); the epilogue pops in reverse (r13, r12, r15, rbp) then rets, so
        // every callee-saved register pushed is popped exactly once and the
        // frame stays balanced.
        assert_eq!(
            P6_X86_64_CALLABLE_PROLOGUE_BYTES,
            &[
                0x55, // push rbp
                0x41, 0x57, // push r15
                0x41, 0x54, // push r12
                0x41, 0x55, // push r13
                0x48, 0x89, 0xf5, // mov rbp, rsi
                0x49, 0x89, 0xff, // mov r15, rdi
                0x49, 0x89, 0xd1, // mov r9, rdx
                0x49, 0x89, 0xcd, // mov r13, rcx (jitDataRegister = IC store base)
            ]
        );
        assert_eq!(
            P6_X86_64_CALLABLE_EPILOGUE_BYTES,
            &[
                0x41, 0x5d, // pop r13
                0x41, 0x5c, // pop r12
                0x41, 0x5f, // pop r15
                0x5d, // pop rbp
                0xc3, // ret
            ]
        );

        // Frame balance: count push (0x50..=0x57, with optional 0x41 REX.B
        // prefix for r8..r15) and pop (0x58..=0x5f) opcodes across the
        // prologue and epilogue and require them to match.
        fn count_push_pop(bytes: &[u8]) -> (usize, usize) {
            let (mut pushes, mut pops) = (0usize, 0usize);
            let mut i = 0;
            while i < bytes.len() {
                let mut op = bytes[i];
                if op == 0x41 && i + 1 < bytes.len() {
                    // REX.B prefix for a one-byte push/pop of r8..r15.
                    let next = bytes[i + 1];
                    if (0x50..=0x5f).contains(&next) {
                        op = next;
                        i += 1;
                    }
                }
                if (0x50..=0x57).contains(&op) {
                    pushes += 1;
                } else if (0x58..=0x5f).contains(&op) {
                    pops += 1;
                }
                i += 1;
            }
            (pushes, pops)
        }
        let (prologue_pushes, prologue_pops) = count_push_pop(P6_X86_64_CALLABLE_PROLOGUE_BYTES);
        let (epilogue_pushes, epilogue_pops) = count_push_pop(P6_X86_64_CALLABLE_EPILOGUE_BYTES);
        assert_eq!(prologue_pushes, 4, "prologue pushes rbp, r15, r12, r13");
        assert_eq!(prologue_pops, 0);
        assert_eq!(epilogue_pushes, 0);
        assert_eq!(
            epilogue_pops, prologue_pushes,
            "epilogue must pop exactly what the prologue pushed"
        );

        // The P9 owner post-call reentry prologue rejoins the shared epilogue,
        // so it must also push r13 to balance the epilogue's pop r13.
        let (reentry_pushes, reentry_pops) =
            count_push_pop(P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES);
        assert_eq!(
            reentry_pushes, prologue_pushes,
            "P9 reentry prologue must push the same callee-saved set as the shared epilogue pops"
        );
        assert_eq!(reentry_pops, 0);
        assert!(
            P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES
                .windows(2)
                .any(|w| w == [0x41, 0x55]),
            "P9 reentry prologue must contain push r13 (0x41 0x55)"
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
    fn p9_callable_emission_retains_exact_call_call_with_this_and_construct_native_exit_metadata() {
        let code_block = p9_call_native_exit_code_block();
        let result = p9_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert_eq!(result.js_call_native_exit_stubs.len(), 3);
        assert_eq!(result.js_call_owner_post_call_stubs.len(), 2);
        assert_eq!(result.js_call_owner_post_call_reentry_stubs.len(), 2);

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
        let call_post_call = &result.js_call_owner_post_call_stubs[0];
        assert_eq!(call_post_call.bytecode_index, call.bytecode_index);
        assert_eq!(call_post_call.opcode, CoreOpcode::Call);
        assert_eq!(call_post_call.destination, local(1));
        assert_eq!(
            call_post_call.resume_bytecode_index,
            BytecodeIndex::from_offset(2)
        );
        assert_eq!(
            call_post_call.resume_instruction_start_offset,
            result.instruction_bytes[2].start_offset
        );
        assert!(call_post_call.start_offset >= call.end_offset);
        assert_eq!(
            call_post_call.reset_sp_noop_offset,
            call_post_call.start_offset
        );
        assert_eq!(
            call_post_call.reset_sp_noop_end_offset,
            call_post_call.start_offset + 1
        );
        assert_eq!(
            call_post_call.result_profile_placeholder_offset,
            call_post_call.start_offset + 1
        );
        assert_eq!(
            call_post_call.result_profile_placeholder_end_offset,
            call_post_call.start_offset + 2
        );
        assert_eq!(
            &call_post_call.bytes[..9],
            &[0x90, 0x90, 0x48, 0x89, 0x85, 0x08, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            p9_owner_post_call_stub_resume_target_for_test(call_post_call),
            call_post_call.resume_instruction_start_offset
        );
        let call_reentry = &result.js_call_owner_post_call_reentry_stubs[0];
        assert_eq!(call_reentry.bytecode_index, call.bytecode_index);
        assert_eq!(call_reentry.opcode, CoreOpcode::Call);
        assert_eq!(call_reentry.destination, local(1));
        assert_eq!(
            call_reentry.resume_bytecode_index,
            BytecodeIndex::from_offset(2)
        );
        assert_eq!(
            call_reentry.post_call_target_start_offset,
            call_post_call.start_offset
        );
        assert_eq!(
            call_reentry.callable_prologue_offset,
            call_reentry.start_offset
        );
        assert_eq!(
            call_reentry.callable_prologue_end_offset,
            call_reentry.start_offset
                + P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES.len() as u32
        );
        assert_eq!(
            call_reentry.result_seed,
            P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRax
        );
        assert_eq!(
            &call_reentry.bytes[..P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES.len()],
            P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES
        );
        assert_eq!(
            &call_reentry.bytes[P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES.len()
                ..P9_X86_64_BASELINE_OWNER_CALL_REENTRY_PROLOGUE_BYTES.len()
                    + P9_X86_64_BASELINE_OWNER_CALL_REENTRY_RESULT_SEED_BYTES.len()],
            P9_X86_64_BASELINE_OWNER_CALL_REENTRY_RESULT_SEED_BYTES
        );
        assert_eq!(
            p9_owner_post_call_reentry_stub_target_for_test(call_reentry),
            call_post_call.start_offset
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
        let call_with_this_post_call = &result.js_call_owner_post_call_stubs[1];
        assert_eq!(
            call_with_this_post_call.bytecode_index,
            call_with_this.bytecode_index
        );
        assert_eq!(call_with_this_post_call.opcode, CoreOpcode::CallWithThis);
        assert_eq!(call_with_this_post_call.destination, local(2));
        assert_eq!(
            call_with_this_post_call.resume_bytecode_index,
            BytecodeIndex::from_offset(3)
        );
        assert_eq!(
            call_with_this_post_call.resume_instruction_start_offset,
            result.instruction_bytes[3].start_offset
        );
        assert!(call_with_this_post_call.start_offset >= call_with_this.end_offset);
        assert_eq!(
            &call_with_this_post_call.bytes[..9],
            &[0x90, 0x90, 0x48, 0x89, 0x85, 0x10, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            p9_owner_post_call_stub_resume_target_for_test(call_with_this_post_call),
            call_with_this_post_call.resume_instruction_start_offset
        );
        let call_with_this_reentry = &result.js_call_owner_post_call_reentry_stubs[1];
        assert_eq!(
            call_with_this_reentry.bytecode_index,
            call_with_this.bytecode_index
        );
        assert_eq!(call_with_this_reentry.opcode, CoreOpcode::CallWithThis);
        assert_eq!(
            call_with_this_reentry.post_call_target_start_offset,
            call_with_this_post_call.start_offset
        );
        assert_eq!(
            p9_owner_post_call_reentry_stub_target_for_test(call_with_this_reentry),
            call_with_this_post_call.start_offset
        );

        let construct = &result.js_call_native_exit_stubs[2];
        assert_eq!(construct.bytecode_index, BytecodeIndex::from_offset(3));
        assert_eq!(construct.opcode, CoreOpcode::Construct);
        assert_eq!(construct.destination, local(3));
        assert_eq!(construct.callee, argument_including_this(1));
        assert_eq!(construct.this_register, None);
        assert_eq!(construct.provided_argument_count, 1);
        assert_eq!(
            construct.argument_registers,
            vec![argument_including_this(3)]
        );
        assert_eq!(
            construct.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(4))
        );
        assert!(construct.requires_no_gc_exit_reentry);
        assert!(construct.may_throw);
        assert_eq!(construct.encoded_payload.call_exit_index(), 2);
        assert_eq!(
            construct.encoded_payload.low_tag(),
            P9_X86_64_BASELINE_JS_CALL_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
        assert_ne!(
            call.encoded_payload.raw_bits(),
            call_with_this.encoded_payload.raw_bits()
        );
        assert_ne!(
            call.encoded_payload.raw_bits(),
            construct.encoded_payload.raw_bits()
        );
        assert_ne!(
            call_with_this.encoded_payload.raw_bits(),
            construct.encoded_payload.raw_bits()
        );
        assert!(result
            .js_call_owner_post_call_stubs
            .iter()
            .all(|stub| stub.bytecode_index != construct.bytecode_index));
        assert!(result
            .js_call_owner_post_call_reentry_stubs
            .iter()
            .all(|stub| stub.bytecode_index != construct.bytecode_index));
    }

    // Regression for the call-path residency gate analogous to the get_by_id
    // FrameLocal->FrameLocal+FrameArgument widening in e5ceee2: a CallWithThis
    // whose RESULT destination is an argument-including-this slot must still emit
    // the owner-post-call and owner-post-call-reentry stubs, because C++ JSC
    // emitPutCallResult (JITCall.cpp:58-61) stores to the dst VirtualRegister's
    // slot regardless of class. Before the fix, p9_x86_64_owner_call_destination_disp32
    // returned None for argument destinations and the stub loop `continue`d, so the
    // P9 exit handler could never find a return-target proof and the site fell to
    // the single-dispatch exit (never warming its call-link, never going resident).
    #[test]
    fn p9_argument_destination_call_with_this_emits_owner_post_call_reentry_stub() {
        let code_block = p9_call_with_this_argument_destination_code_block();
        let result = p9_callable_semantic_emission_for_code_block(&code_block).unwrap();

        let call_with_this = result
            .js_call_native_exit_stubs
            .iter()
            .find(|stub| stub.opcode == CoreOpcode::CallWithThis)
            .expect("CallWithThis native-exit stub");
        // The result destination is an argument-classified register, exactly the
        // shape that was skipped before the widening.
        assert_eq!(
            call_with_this.destination.classify(ThisArgumentOffset(5)),
            RegisterClass::ArgumentIncludingThis(2)
        );

        // The owner-post-call stub is now emitted for this argument destination
        // (previously the loop `continue`d and produced nothing for this site).
        let post_call = result
            .js_call_owner_post_call_stubs
            .iter()
            .find(|stub| stub.bytecode_index == call_with_this.bytecode_index)
            .expect("owner-post-call stub for argument-destination CallWithThis");
        assert_eq!(post_call.opcode, CoreOpcode::CallWithThis);
        assert_eq!(post_call.destination, call_with_this.destination);

        // The matching owner-post-call-reentry stub exists, so at the P9 exit the
        // return-target proof lookup will succeed and control returns into the
        // owner region (no cross-compartment near-call).
        let reentry = result
            .js_call_owner_post_call_reentry_stubs
            .iter()
            .find(|stub| stub.bytecode_index == call_with_this.bytecode_index)
            .expect("owner-post-call-reentry stub for argument-destination CallWithThis");
        assert_eq!(reentry.opcode, CoreOpcode::CallWithThis);
        assert_eq!(
            reentry.post_call_target_start_offset,
            post_call.start_offset
        );
        assert_eq!(
            p9_owner_post_call_reentry_stub_target_for_test(reentry),
            post_call.start_offset
        );

        // The result store writes rax (call result) to [rbp+disp32] where disp32 is
        // the destination's frame-base offset, matching the formula every other
        // argument read/write uses (frame_local_count*stride + index*width). Recompute
        // it from the contract and assert the emitted store carries that exact disp32,
        // so a wrong-slot store (which would corrupt the call result) is caught.
        let js_call_plan =
            crate::jit::plan::derive_baseline_generated_js_call_native_exit_plan_from_code_block(
                &code_block,
                owner(),
            )
            .unwrap()
            .metadata
            .expect("P9 call native-exit metadata");
        let lowering = plan_p6_x86_64_baseline_lowering_with_native_exits(
            P6X86_64BaselineLoweringRequest::new(
                owner(),
                &code_block,
                p9_mixed_lowering_proof(&code_block),
            ),
            None,
            Some(&js_call_plan),
            None,
        )
        .unwrap();
        let contract = record_p6_x86_64_baseline_backend_contract(&lowering).unwrap();
        let expected_disp32 = p9_x86_64_owner_call_destination_disp32(
            &contract,
            call_with_this.bytecode_index,
            call_with_this.destination,
        )
        .unwrap()
        .expect("argument destination now resolves to a disp32");
        let store_local_offset = (post_call.result_store_offset - post_call.start_offset) as usize;
        // `48 89 85 <disp32>` = mov [rbp+disp32], rax
        assert_eq!(
            &post_call.bytes[store_local_offset..store_local_offset + 3],
            &[0x48, 0x89, 0x85]
        );
        let mut emitted_disp = [0u8; 4];
        emitted_disp
            .copy_from_slice(&post_call.bytes[store_local_offset + 3..store_local_offset + 7]);
        assert_eq!(i32::from_le_bytes(emitted_disp), expected_disp32);
    }

    #[test]
    fn p9_callable_emission_with_owner_map_uses_metadata_table_relative_profile_store() {
        let code_block = p9_call_native_exit_code_block();
        let result =
            p9_callable_semantic_emission_with_owner_map_for_code_block(&code_block).unwrap();

        let call_post_call = &result.js_call_owner_post_call_stubs[0];
        let mut expected_call_profile_store = vec![0x90, 0x49, 0x89, 0x84, 0x24];
        expected_call_profile_store.extend_from_slice(&(-32i32).to_le_bytes());
        expected_call_profile_store.extend_from_slice(&[0x48, 0x89, 0x85, 0x08, 0, 0, 0]);
        assert_eq!(
            &call_post_call.bytes[..expected_call_profile_store.len()],
            expected_call_profile_store.as_slice()
        );
        assert_eq!(
            call_post_call.result_profile_placeholder_end_offset
                - call_post_call.result_profile_placeholder_offset,
            8
        );

        let call_with_this_post_call = &result.js_call_owner_post_call_stubs[1];
        let mut expected_call_with_this_profile_store = vec![0x90, 0x49, 0x89, 0x84, 0x24];
        expected_call_with_this_profile_store.extend_from_slice(&(-48i32).to_le_bytes());
        expected_call_with_this_profile_store.extend_from_slice(&[0x48, 0x89, 0x85, 0x10, 0, 0, 0]);
        assert_eq!(
            &call_with_this_post_call.bytes[..expected_call_with_this_profile_store.len()],
            expected_call_with_this_profile_store.as_slice()
        );

        let call_reentry = &result.js_call_owner_post_call_reentry_stubs[0];
        assert_eq!(
            call_reentry.result_seed,
            P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRaxFourthArgumentRcxToR12
        );
        assert_eq!(
            call_reentry.metadata_table_seed_offset,
            Some(call_reentry.result_seed_end_offset)
        );
        assert_eq!(
            call_reentry.metadata_table_seed_end_offset,
            Some(call_reentry.post_call_jump_offset)
        );
        let metadata_seed_start =
            (call_reentry.metadata_table_seed_offset.unwrap() - call_reentry.start_offset) as usize;
        let metadata_seed_end = (call_reentry.metadata_table_seed_end_offset.unwrap()
            - call_reentry.start_offset) as usize;
        assert_eq!(
            &call_reentry.bytes[metadata_seed_start..metadata_seed_end],
            P9_X86_64_BASELINE_OWNER_CALL_REENTRY_METADATA_TABLE_SEED_BYTES
        );
        assert!(call_reentry.metadata_table_base_address.is_some());
    }

    #[test]
    fn p9_owner_continuation_native_binding_maps_call_done_offsets_separately_from_resume_labels() {
        let code_block = p9_call_native_exit_code_block();
        let owner_map =
            crate::jit::plan::derive_baseline_generated_owner_continuation_map_from_code_block(
                &code_block,
                owner(),
            )
            .unwrap()
            .metadata
            .expect("owner continuation map");
        let result = p9_callable_semantic_emission_for_code_block(&code_block).unwrap();
        let binding =
            bind_p6_x86_64_owner_continuation_map_to_semantic_emission(&owner_map, &result)
                .expect("owner continuation native binding");

        assert_eq!(binding.owner, owner());
        assert_eq!(binding.bytecode_snapshot, result.bytecode_snapshot);
        assert_eq!(binding.entry_offset, result.entry_offset);
        assert_eq!(
            binding.linked_size_bytes,
            result.linked_image.output_size_bytes
        );
        assert_eq!(binding.linked_digest, result.linked_image.output_digest);
        assert_eq!(binding.label_count(), result.instruction_bytes.len());
        assert_eq!(binding.call_continuation_count(), 3);

        let call = binding
            .call_continuation_for_bytecode_index(BytecodeIndex::from_offset(1))
            .expect("call binding");
        let call_stub = &result.js_call_native_exit_stubs[0];
        let resume_label = binding
            .label_for_bytecode_index(BytecodeIndex::from_offset(2))
            .expect("resume label");
        assert_eq!(call.opcode, CoreOpcode::Call);
        assert_eq!(call.kind, BaselineGeneratedOwnerContinuationKind::Call);
        assert_eq!(call.destination, local(1));
        assert_eq!(call.argument_count_including_this, 2);
        assert_eq!(
            call.resume_bytecode_index,
            Some(BytecodeIndex::from_offset(2))
        );
        assert_eq!(call.native_exit_stub_start_offset, call_stub.start_offset);
        assert_eq!(call.native_exit_stub_end_offset, call_stub.end_offset);
        assert_eq!(call.native_exit_stub_byte_len, call_stub.byte_len);
        assert_eq!(call.done_label_offset, call_stub.start_offset);
        assert_eq!(
            call.resume_instruction_start_offset,
            Some(resume_label.instruction_start_offset)
        );
        let call_profile_binding = call
            .post_call_native_block
            .as_ref()
            .and_then(|block| block.result_profile_binding)
            .expect("call result profile binding");
        assert_eq!(
            call.post_call_obligations,
            vec![
                P9X86_64BaselineOwnerCallPostCallObligation::ResetCallerStackPointer {
                    status: P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister,
                },
                P9X86_64BaselineOwnerCallPostCallObligation::ProfileCallResult {
                    status: P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending,
                    binding: Some(call_profile_binding),
                },
                P9X86_64BaselineOwnerCallPostCallObligation::WriteCallResult {
                    destination: local(1),
                },
                P9X86_64BaselineOwnerCallPostCallObligation::ResumeAtBytecodeLabel {
                    resume_bytecode_index: Some(BytecodeIndex::from_offset(2)),
                    resume_instruction_start_offset: Some(resume_label.instruction_start_offset),
                },
            ]
        );
        let call_post_call = call
            .post_call_native_block
            .as_ref()
            .expect("call post-call native block");
        assert_eq!(
            call_post_call.resume_instruction_start_offset,
            resume_label.instruction_start_offset
        );
        assert!(call_post_call.start_offset > call.native_exit_stub_end_offset);
        assert_eq!(
            call_post_call.reset_sp_status,
            P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister
        );
        assert_eq!(call_post_call.reset_sp_offset, call_post_call.start_offset);
        assert_eq!(
            call_post_call.reset_sp_end_offset,
            call_post_call.result_profile_offset
        );
        assert_eq!(
            call_post_call.result_profile_status,
            P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending
        );
        assert_eq!(
            call_post_call.result_profile_binding,
            Some(call_profile_binding)
        );
        assert_eq!(
            call_post_call.result_profile_end_offset,
            call_post_call.result_store_offset
        );
        assert!(call_post_call.result_store_offset < call_post_call.resume_jump_offset);
        assert!(call_post_call.resume_jump_end_offset <= call_post_call.end_offset);
        let call_reentry = call
            .post_call_reentry_stub
            .as_ref()
            .expect("call post-call reentry stub");
        assert!(call_reentry.start_offset >= call_post_call.end_offset);
        assert_eq!(
            call_reentry.callable_prologue_offset,
            call_reentry.start_offset
        );
        assert_eq!(
            call_reentry.result_seed,
            P9X86_64BaselineOwnerPostCallReentryResultSeed::X86_64CAbiThirdArgumentRdxToRax
        );
        assert_eq!(
            call_reentry.result_seed_offset,
            call_reentry.callable_prologue_end_offset
        );
        assert_eq!(
            call_reentry.result_seed_end_offset,
            call_reentry.post_call_jump_offset
        );
        assert_eq!(
            call_reentry.post_call_jump_target_start_offset,
            call_post_call.start_offset
        );
        let call_return_target = call
            .post_call_return_target_proof
            .as_ref()
            .expect("call post-call return target proof");
        assert_eq!(call_return_target.encoded_payload, call.encoded_payload);
        assert_eq!(
            call_return_target.reset_sp_status,
            call_post_call.reset_sp_status
        );
        assert_eq!(
            call_return_target.reset_sp_offset,
            call_post_call.reset_sp_offset
        );
        assert_eq!(
            call_return_target.result_profile_status,
            call_post_call.result_profile_status
        );
        assert_eq!(
            call_return_target.result_profile_binding,
            Some(call_profile_binding)
        );
        assert_eq!(
            call_return_target.result_profile_end_offset,
            call_post_call.result_store_offset
        );
        assert_eq!(
            call_return_target.post_call_target_start_offset,
            call_post_call.start_offset
        );
        assert_eq!(
            call_return_target.resume_instruction_start_offset,
            resume_label.instruction_start_offset
        );
        assert_eq!(
            call_return_target.post_call_reentry_stub_start_offset,
            call_reentry.start_offset
        );
        assert_eq!(
            call_return_target.post_call_reentry_result_seed,
            call_reentry.result_seed
        );
        assert_eq!(
            call_return_target.post_call_reentry_jump_target_start_offset,
            call_post_call.start_offset
        );
        assert_eq!(
            call_return_target.linked_size_bytes,
            result.linked_image.output_size_bytes
        );
        assert_eq!(
            call_return_target.linked_digest,
            result.linked_image.output_digest
        );
        assert_ne!(
            call_return_target.post_call_target_start_offset,
            call.native_exit_stub_start_offset
        );
        assert_ne!(
            call_return_target.post_call_target_start_offset,
            resume_label.instruction_start_offset
        );
        assert_ne!(
            call.done_label_offset,
            resume_label.instruction_start_offset
        );

        let call_with_this = binding
            .call_continuation_for_bytecode_index(BytecodeIndex::from_offset(2))
            .expect("call-with-this binding");
        let call_with_this_resume_label = binding
            .label_for_bytecode_index(BytecodeIndex::from_offset(3))
            .expect("call-with-this resume label");
        assert_eq!(call_with_this.opcode, CoreOpcode::CallWithThis);
        assert_eq!(
            call_with_this.kind,
            BaselineGeneratedOwnerContinuationKind::Call
        );
        let call_with_this_profile_binding = call_with_this
            .post_call_native_block
            .as_ref()
            .and_then(|block| block.result_profile_binding)
            .expect("call-with-this result profile binding");
        assert_eq!(
            call_with_this.post_call_obligations,
            vec![
                P9X86_64BaselineOwnerCallPostCallObligation::ResetCallerStackPointer {
                    status: P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister,
                },
                P9X86_64BaselineOwnerCallPostCallObligation::ProfileCallResult {
                    status: P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending,
                    binding: Some(call_with_this_profile_binding),
                },
                P9X86_64BaselineOwnerCallPostCallObligation::WriteCallResult {
                    destination: local(2),
                },
                P9X86_64BaselineOwnerCallPostCallObligation::ResumeAtBytecodeLabel {
                    resume_bytecode_index: Some(BytecodeIndex::from_offset(3)),
                    resume_instruction_start_offset: Some(
                        call_with_this_resume_label.instruction_start_offset,
                    ),
                },
            ]
        );
        let call_with_this_post_call = call_with_this
            .post_call_native_block
            .as_ref()
            .expect("call-with-this post-call native block");
        assert_eq!(
            call_with_this_post_call.resume_instruction_start_offset,
            call_with_this_resume_label.instruction_start_offset
        );
        assert!(call_with_this_post_call.start_offset > call_with_this.native_exit_stub_end_offset);
        assert_eq!(
            call_with_this_post_call.reset_sp_status,
            P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister
        );
        assert_eq!(
            call_with_this_post_call.result_profile_status,
            P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending
        );
        assert_eq!(
            call_with_this_post_call.result_profile_binding,
            Some(call_with_this_profile_binding)
        );
        let call_with_this_return_target = call_with_this
            .post_call_return_target_proof
            .as_ref()
            .expect("call-with-this post-call return target proof");
        let call_with_this_reentry = call_with_this
            .post_call_reentry_stub
            .as_ref()
            .expect("call-with-this post-call reentry stub");
        assert_eq!(
            call_with_this_return_target.reset_sp_offset,
            call_with_this_post_call.reset_sp_offset
        );
        assert_eq!(
            call_with_this_return_target.result_profile_end_offset,
            call_with_this_post_call.result_store_offset
        );
        assert_eq!(
            call_with_this_return_target.result_profile_binding,
            Some(call_with_this_profile_binding)
        );
        assert_eq!(
            call_with_this_return_target.post_call_target_start_offset,
            call_with_this_post_call.start_offset
        );
        assert_eq!(
            call_with_this_return_target.resume_instruction_start_offset,
            call_with_this_resume_label.instruction_start_offset
        );
        assert_eq!(
            call_with_this_return_target.post_call_reentry_stub_start_offset,
            call_with_this_reentry.start_offset
        );
        assert_eq!(
            call_with_this_return_target.post_call_reentry_jump_target_start_offset,
            call_with_this_post_call.start_offset
        );

        let construct = binding
            .call_continuation_for_bytecode_index(BytecodeIndex::from_offset(3))
            .expect("construct binding");
        let construct_resume_label = binding
            .label_for_bytecode_index(BytecodeIndex::from_offset(4))
            .expect("construct resume label");
        assert_eq!(construct.opcode, CoreOpcode::Construct);
        assert_eq!(
            construct.kind,
            BaselineGeneratedOwnerContinuationKind::Construct
        );
        assert_eq!(construct.destination, local(3));
        assert_eq!(construct.argument_count_including_this, 2);
        assert_eq!(
            construct.post_call_obligations,
            vec![
                P9X86_64BaselineOwnerCallPostCallObligation::ResetCallerStackPointer {
                    status: P9X86_64BaselineOwnerCallResetSpStatus::P6CallableAbiNoJsStackPointerRegister,
                },
                P9X86_64BaselineOwnerCallPostCallObligation::ProfileCallResult {
                    status: P9X86_64BaselineOwnerCallResultProfileStatus::MetadataPending,
                    binding: None,
                },
                P9X86_64BaselineOwnerCallPostCallObligation::NormalizeConstructResultThenWrite {
                    destination: local(3),
                },
                P9X86_64BaselineOwnerCallPostCallObligation::ResumeAtBytecodeLabel {
                    resume_bytecode_index: Some(BytecodeIndex::from_offset(4)),
                    resume_instruction_start_offset: Some(
                        construct_resume_label.instruction_start_offset,
                    ),
                },
            ]
        );
        assert_eq!(construct.post_call_native_block, None);
        assert_eq!(construct.post_call_reentry_stub, None);
        assert_eq!(construct.post_call_return_target_proof, None);
    }

    #[test]
    fn p9_owner_continuation_native_binding_rejects_stale_owner_map_snapshot() {
        let result =
            p9_callable_semantic_emission_for_code_block(&p9_call_native_exit_code_block())
                .unwrap();
        let stale_map =
            crate::jit::plan::derive_baseline_generated_owner_continuation_map_from_code_block(
                &p6_lowering_code_block(),
                owner(),
            )
            .unwrap()
            .metadata
            .expect("stale owner continuation map");

        assert_eq!(
            bind_p6_x86_64_owner_continuation_map_to_semantic_emission(&stale_map, &result),
            Err(
                P6X86_64BaselineOwnerContinuationNativeBindingError::SnapshotMismatch {
                    owner_map: stale_map.bytecode_snapshot(),
                    emission: result.bytecode_snapshot,
                }
            )
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
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(11)
            )))
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

    // STEP B: a GetByName with a plain frame-local base emits the resident self-load
    // DataIC fast path INLINE before the slow-path return stub. Pin the exact fast-path
    // prologue bytes (rel32-independent) and confirm the slow-path stub (returning the
    // P10 native-exit payload) is still present so a miss exits to the interpreter.
    #[test]
    fn p10_callable_emission_emits_inline_self_load_data_ic_for_frame_local_get_by_name() {
        let code_block = p10_get_by_name_self_load_frame_local_code_block();
        let result = p10_callable_semantic_emission_for_code_block(&code_block).unwrap();
        assert_eq!(result.property_native_exit_stubs.len(), 1);
        let property = &result.property_native_exit_stubs[0];
        assert_eq!(property.site.opcode, CoreOpcode::GetByName);
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::GetByName {
                destination: local(2),
                base: local(1),
            }
        );

        // base local(1) -> [rbp + 8]; dest local(2) -> [rbp + 16]; record_index 0
        // (record stride is 16: structure_id@+0, offset@+4, holder_ptr@+8).
        let bytes = &property.bytes;
        // mov rax, [rbp+8]
        assert_eq!(&bytes[0..7], &[0x48, 0x8b, 0x85, 0x08, 0x00, 0x00, 0x00]);
        // cmp al, TAG_CELL(0x20)
        assert_eq!(&bytes[7..9], &[0x3c, 0x20]);
        // jne rel32 (slow path / not a cell)
        assert_eq!(&bytes[9..11], &[0x0f, 0x85]);
        // shr rax, 8  (unbox)
        assert_eq!(&bytes[15..19], &[0x48, 0xc1, 0xe8, 0x08]);
        // mov r10d, [rax+0]  (receiver structure id)
        assert_eq!(&bytes[19..26], &[0x44, 0x8b, 0x90, 0x00, 0x00, 0x00, 0x00]);
        // cmp r10d, [r13+0]  (cached id)
        assert_eq!(&bytes[26..33], &[0x45, 0x3b, 0x95, 0x00, 0x00, 0x00, 0x00]);
        // jne rel32 (structure miss)
        assert_eq!(&bytes[33..35], &[0x0f, 0x85]);
        // mov r11d, [r13+4]  (cached offset)
        assert_eq!(&bytes[39..46], &[0x45, 0x8b, 0x9d, 0x04, 0x00, 0x00, 0x00]);
        // mov r10, [r13+8]  (baked holder ptr or 0; offsetOfInlineHolder analog)
        assert_eq!(&bytes[46..53], &[0x4d, 0x8b, 0x95, 0x08, 0x00, 0x00, 0x00]);
        // test r10, r10
        assert_eq!(&bytes[53..56], &[0x4d, 0x85, 0xd2]);
        // jnz rel32 (holder_ptr != 0 -> prototype load uses the holder)
        assert_eq!(&bytes[56..58], &[0x0f, 0x85]);
        // SELF case: mov r10, [rax+8]  (receiver storage ptr)
        assert_eq!(&bytes[62..69], &[0x4c, 0x8b, 0x90, 0x08, 0x00, 0x00, 0x00]);
        // jmp rel32 (storage base ready)
        assert_eq!(bytes[69], 0xe9);
        // PROTOTYPE case: mov r10, [r10+8]  (holder storage ptr; no unbox)
        assert_eq!(&bytes[74..81], &[0x4d, 0x8b, 0x92, 0x08, 0x00, 0x00, 0x00]);
        // mov rax, [r10+r11*8]  (value)
        assert_eq!(&bytes[81..85], &[0x4b, 0x8b, 0x04, 0xda]);
        // mov [rbp+16], rax  (store to dest)
        assert_eq!(&bytes[85..92], &[0x48, 0x89, 0x85, 0x10, 0x00, 0x00, 0x00]);
        // jmp rel32 (resident, over the slow-path stub)
        assert_eq!(bytes[92], 0xe9);

        // The slow-path return stub bytes must still appear (mov eax, payload + epilogue
        // ending in ret) so a guard miss exits the same way a standalone P10 exit would.
        let standalone_stub = p10_x86_64_callable_property_native_exit_return_stub_bytes(
            property.encoded_payload.raw_bits(),
        );
        assert!(
            bytes
                .windows(standalone_stub.len())
                .any(|window| window == standalone_stub.as_slice()),
            "DataIC must embed the slow-path native-exit return stub"
        );
        assert_eq!(*bytes.last().unwrap(), 0xc3, "slow path ends in ret");
    }

    // FIX 2 reconciliation: the emit-time `property_exit_index()` baked into each
    // property-handoff lowered op MUST equal the plan's positional
    // `property_site_index_for_bytecode_index`. Both now filter decoded instructions in
    // bytecode order by THE canonical `baseline_opcode_is_generated_property_handoff`
    // predicate, so they are the identical enumeration. This block interleaves a
    // GetLength (the opcode the emitter used to OMIT from its counter while the plan
    // counted it, the exact off-by-one source) between two admitted GetByName self-load
    // sites, so a regression desyncs the second GetByName's record index here. The
    // lowering loop also debug_asserts this equality; this test pins it explicitly.
    #[test]
    fn p10_property_handoff_emit_index_equals_writeback_index_with_interleaved_get_length() {
        let code_block = code_block_from_typed_instructions(vec![
            typed_instruction(
                0,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(2)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(11),
                ],
            ),
            typed_instruction(
                1,
                CoreOpcode::GetLength,
                vec![
                    Operand::Register(local(3)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(12),
                ],
            ),
            typed_instruction(
                2,
                CoreOpcode::GetByName,
                vec![
                    Operand::Register(local(4)),
                    Operand::Register(local(1)),
                    Operand::IdentifierIndex(13),
                ],
            ),
            typed_instruction(3, CoreOpcode::Return, vec![Operand::Register(local(4))]),
        ]);

        let property_plan =
            crate::jit::plan::derive_baseline_generated_property_handoff_plan_from_code_block(
                &code_block,
                owner(),
            )
            .unwrap()
            .metadata
            .expect("property native-exit metadata");
        // All three property-handoff opcodes count (GetByName, GetLength, GetByName).
        assert_eq!(property_plan.site_count(), 3);

        let proof = p9_mixed_lowering_proof(&code_block);
        let lowering = plan_p6_x86_64_baseline_lowering_with_native_exits(
            P6X86_64BaselineLoweringRequest::new(owner(), &code_block, proof),
            None,
            None,
            Some(&property_plan),
        )
        .unwrap();

        let plan = property_plan.borrowed_plan();
        let mut admitted_sites = 0;
        for op in &lowering.plan.operations {
            if let P6X86_64BaselineLoweredOperation::PropertyNativeExit {
                encoded_payload, ..
            } = op.operation
            {
                let emit_index = encoded_payload.property_exit_index() as usize;
                let writeback_index = plan
                    .property_site_index_for_bytecode_index(op.bytecode_index)
                    .expect("every property-handoff site has a plan index");
                assert_eq!(
                    emit_index, writeback_index,
                    "emit-index != writeback-index at {:?}",
                    op.bytecode_index
                );
                admitted_sites += 1;
            }
        }
        // All three property-handoff sites lowered to PropertyNativeExit ops with
        // matching indices (the GetLength in the middle does not skew the second
        // GetByName's index).
        assert_eq!(admitted_sites, 3);
    }

    #[test]
    fn p10_callable_emission_retains_exact_get_global_object_property_native_exit_metadata() {
        let code_block = p10_get_global_object_property_native_exit_code_block();
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
        assert_eq!(property.site.opcode, CoreOpcode::GetGlobalObjectProperty);
        assert_eq!(
            property.site.cache_kind,
            crate::jit::InlineCacheKind::PropertyLoad
        );
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::GetGlobalObjectProperty {
                destination: local(1),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(11)
            )))
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
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(11)
            )))
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
    fn p10_callable_emission_retains_exact_get_by_value_property_native_exit_metadata() {
        let code_block = p10_get_by_value_native_exit_code_block();
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
        assert_eq!(property.site.opcode, CoreOpcode::GetByValue);
        assert_eq!(
            property.site.cache_kind,
            crate::jit::InlineCacheKind::ElementLoad
        );
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::GetByValue {
                destination: local(1),
                base: argument_including_this(1),
                property: local(3),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyCacheKey::RuntimeValue(local(3))
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
    fn p10_callable_emission_retains_exact_put_by_value_property_native_exit_metadata() {
        let code_block = p10_put_by_value_native_exit_code_block();
        let result = p10_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert!(result.js_call_native_exit_stubs.is_empty());
        assert_eq!(result.property_native_exit_stubs.len(), 1);

        let property = &result.property_native_exit_stubs[0];
        assert_eq!(property.bytecode_index, BytecodeIndex::from_offset(2));
        assert_eq!(property.site.owner, owner());
        assert_eq!(property.site.slot, crate::jit::InlineCacheSlotId(0));
        assert_eq!(property.site.bytecode_index, BytecodeIndex::from_offset(2));
        assert_eq!(property.site.opcode, CoreOpcode::PutByValue);
        assert_eq!(
            property.site.cache_kind,
            crate::jit::InlineCacheKind::ElementStore
        );
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::PutByValue {
                base: argument_including_this(1),
                property: local(3),
                value: local(0),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyCacheKey::RuntimeValue(local(3))
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
    fn p10_callable_emission_retains_exact_in_by_id_property_native_exit_metadata() {
        let code_block = p10_in_by_id_native_exit_code_block();
        let result = p10_callable_semantic_emission_for_code_block(&code_block).unwrap();

        assert!(result.runtime_helper_native_exit_stubs.is_empty());
        assert!(result.side_exit_return_stubs.is_empty());
        assert!(result.js_call_native_exit_stubs.is_empty());
        assert_eq!(result.property_native_exit_stubs.len(), 1);

        let property = &result.property_native_exit_stubs[0];
        assert_eq!(property.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(property.site.owner, owner());
        assert_eq!(property.site.slot, crate::jit::InlineCacheSlotId(0));
        assert_eq!(property.site.bytecode_index, BytecodeIndex::from_offset(0));
        assert_eq!(property.site.opcode, CoreOpcode::InById);
        assert_eq!(
            property.site.cache_kind,
            crate::jit::InlineCacheKind::HasProperty
        );
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::InById {
                destination: local(1),
                base: argument_including_this(1),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyCacheKey::Key(PropertyKey::from_identifier(Identifier::from_atom(
                AtomId::from_table_slot(11)
            )))
        );
        assert!(property.site.requires_no_gc_exit_reentry);
        assert!(property.site.may_throw);
        assert_eq!(property.encoded_payload.property_exit_index(), 0);
        assert_eq!(
            property.encoded_payload.low_tag(),
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
        );
    }

    #[test]
    fn p10_callable_emission_retains_exact_in_by_val_property_native_exit_metadata() {
        let code_block = p10_in_by_val_native_exit_code_block();
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
        assert_eq!(property.site.opcode, CoreOpcode::InByVal);
        assert_eq!(
            property.site.cache_kind,
            crate::jit::InlineCacheKind::HasProperty
        );
        assert_eq!(
            property.operands,
            P10X86_64BaselinePropertyNativeExitOperands::InByVal {
                destination: local(1),
                base: argument_including_this(1),
                property: local(3),
            }
        );
        assert_eq!(
            property.site.property_key,
            PropertyCacheKey::RuntimeValue(local(3))
        );
        assert!(property.site.requires_no_gc_exit_reentry);
        assert!(property.site.may_throw);
        assert_eq!(property.encoded_payload.property_exit_index(), 0);
        assert_eq!(
            property.encoded_payload.low_tag(),
            P10_X86_64_BASELINE_PROPERTY_NATIVE_EXIT_RETURN_PAYLOAD_LOW_TAG
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
            let expected_resume_bytecode = match stub.bytecode_index.offset() {
                7 => BytecodeIndex::from_offset(8),
                8 => BytecodeIndex::from_offset(9),
                9 => BytecodeIndex::from_offset(10),
                offset => panic!("unexpected P6 arithmetic side-exit bytecode {offset}"),
            };
            assert_eq!(stub.resume_bytecode_index, Some(expected_resume_bytecode));
            let resume_instruction = result
                .instruction_bytes
                .iter()
                .find(|record| record.bytecode_index == expected_resume_bytecode)
                .expect("resume instruction bytes");
            let resume_entry_offset = stub
                .resume_entry_offset
                .expect("side exit resume entry offset");
            assert_eq!(
                stub.native_reentry_targets,
                vec![P6BaselineNativeReentryTargetRecord {
                    resume_bytecode_index: expected_resume_bytecode,
                    resume_entry_offset,
                }]
            );
            assert!(resume_entry_offset > resume_instruction.start_offset);
            assert!(resume_entry_offset < resume_instruction.end_offset);
            let resume_entry_end_offset =
                resume_entry_offset + P6_X86_64_CALLABLE_PROLOGUE_BYTES.len() as u32;
            assert_eq!(
                &bytes[resume_entry_offset as usize..resume_entry_end_offset as usize],
                P6_X86_64_CALLABLE_PROLOGUE_BYTES
            );
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
                AssemblerArchitecture::X86_64,
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
        let entrypoints_before = code_block.entrypoints().clone();
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
        assert_eq!(code_block.entrypoints(), &entrypoints_before);
        assert_eq!(code_block.lifecycle(), lifecycle_before);
        assert!(code_block.entrypoints().baseline_jit().is_none());
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

    // ---- Batch 3a: assembler foundation primitives for inline machine-code
    // monomorphic GET_BY_ID (structure guard + offset-indexed property load).
    // Each test round-trips one new machine instruction through the full
    // freeze->layout->link->validate pipeline (finish_p6_x86_64_semantic_byte_emission)
    // and asserts the linked bytes equal the hand-encoded x86-64 mirroring
    // bytecode/InlineAccess.cpp:191-204. ----

    fn cell_relative(disp32: i32) -> P6X86_64BaselineMachineOperand {
        memory_operand(
            P6X86_64BaselineSymbolicRegister::PropertyBase,
            P6X86_64BaselineOperandLocation::CellRelative { disp32 },
        )
    }

    fn structure_mismatch_label(bytecode_offset: u32) -> P6X86_64BaselineSideExitLabel {
        side_exit_label(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            bytecode_offset,
        )
    }

    // Builds bytes for a single machine instruction, resolves its side-exit
    // placeholders, and drives them through the same freeze->layout->link->validate
    // pipeline production uses (finish_p6_x86_64_semantic_byte_emission). Returns
    // the validated linked image bytes, which validate proves identical to the
    // source bytes, so the returned bytes are exactly what the emitter produced.
    fn round_trip_single_instruction(
        instruction: P6X86_64BaselineMachineInstruction,
        bytecode_index: BytecodeIndex,
    ) -> Vec<u8> {
        let contract = p6_backend_contract();
        let mut builder = P6X86_64SemanticByteBuilder::default();
        emit_p6_x86_64_semantic_machine_instruction(
            &mut builder,
            &contract,
            bytecode_index,
            instruction,
            P6X86_64SemanticBranchEmissionMode::DirectBytecodeTargets,
        )
        .unwrap();
        let normal_path_end_offset = builder.offset().unwrap();
        let side_exit_placeholders = builder.finish_side_exit_placeholders().unwrap();
        let encoded = P6X86_64SemanticEncodedSelection {
            bytes: builder.bytes,
            terminal_policy: P6X86_64BaselineTerminalPolicyRecord {
                policy:
                    P6X86_64BaselineTerminalPolicy::SingleFinalNormalReturnRetThenInlineUd2SideExits,
                return_bytecode_index: bytecode_index,
                ret_offset: 0,
                normal_path_end_offset,
            },
            callable_prologue: None,
            callable_normal_epilogue: None,
            instruction_bytes: Vec::new(),
            bytecode_branches: Vec::new(),
            side_exit_placeholders,
            side_exit_return_stubs: Vec::new(),
            loop_backedge_safepoint_stubs: Vec::new(),
            runtime_helper_native_exit_stubs: Vec::new(),
            js_call_native_exit_stubs: Vec::new(),
            js_call_owner_post_call_stubs: Vec::new(),
            js_call_owner_post_call_reentry_stubs: Vec::new(),
            property_native_exit_stubs: Vec::new(),
        };
        let result = finish_p6_x86_64_semantic_byte_emission(
            &contract,
            encoded,
            P6X86_64BaselineSemanticByteEmissionShape::P2aSemanticX86_64FromAcceptedP6Selection,
            P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableNonCallableSemanticBytesOnly,
            p6_x86_64_semantic_source_buffer_id(&contract),
            p6_x86_64_semantic_source_image_id(&contract),
            0,
            AssemblerArchitecture::X86_64,
            p6_x86_64_semantic_physical_register_map(),
            0,
        )
        .unwrap();
        // validate proved linked == source; assert it to make that explicit.
        assert_eq!(result.linked_image.bytes(), result.source_image.bytes());
        result.linked_image.bytes().to_vec()
    }

    #[test]
    fn property_base_binds_to_rdi_in_register_map() {
        // C++ JSC GetById::baseJSR == argumentGPR0 == X86Registers::edi (rdi).
        let map = p6_x86_64_semantic_physical_register_map();
        let binding = map
            .bindings
            .iter()
            .find(|b| b.symbolic == P6X86_64BaselineSymbolicRegister::PropertyBase)
            .expect("PropertyBase must be bound");
        assert_eq!(binding.physical, "rdi");
    }

    #[test]
    fn loadq_cell_relative_into_return_gpr_encodes_rex_w_8b_modrm10_disp32() {
        // C++: loadValue(Address(base /* rdi */, offsetRelativeToBase(offset)), value)
        // mov rax, [rdi+0x10] = REX.W 8B, ModRM mod=10 reg=000(rax) rm=111(rdi)=0x87.
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::ReturnGpr,
                source: cell_relative(0x10),
            },
            BytecodeIndex::from_offset(0),
        );
        let mut expected = vec![0x48, 0x8b, 0x87];
        expected.extend_from_slice(&0x10i32.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn loadq_cell_relative_into_scratch0_encodes_rex_wr_8b_modrm10_disp32() {
        // mov r10, [rdi+0x10] = REX.WR 4C 8B, ModRM mod=10 reg=010(r10) rm=111=0x97.
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch0,
                source: cell_relative(0x18),
            },
            BytecodeIndex::from_offset(0),
        );
        let mut expected = vec![0x4c, 0x8b, 0x97];
        expected.extend_from_slice(&0x18i32.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn loadq_cell_relative_into_scratch1_encodes_rex_wr_8b_modrm10_disp32() {
        // mov r11, [rdi-0x8] = REX.WR 4C 8B, ModRM mod=10 reg=011(r11) rm=111=0x9F.
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::Scratch1,
                source: cell_relative(-8),
            },
            BytecodeIndex::from_offset(0),
        );
        let mut expected = vec![0x4c, 0x8b, 0x9f];
        expected.extend_from_slice(&(-8i32).to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn storeq_cell_relative_from_return_gpr_encodes_rex_w_89_modrm10_disp32() {
        // mov [rdi+0x10], rax = REX.W 48 89, ModRM mod=10 reg=000(rax) rm=111=0x87.
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: cell_relative(0x10),
                source: P6X86_64BaselineSymbolicRegister::ReturnGpr,
            },
            BytecodeIndex::from_offset(0),
        );
        let mut expected = vec![0x48, 0x89, 0x87];
        expected.extend_from_slice(&0x10i32.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn storeq_cell_relative_from_scratch0_encodes_rex_wr_89_modrm10_disp32() {
        // mov [rdi+0x20], r10 = REX.WR 4C 89, ModRM mod=10 reg=010(r10) rm=111=0x97.
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: cell_relative(0x20),
                source: P6X86_64BaselineSymbolicRegister::Scratch0,
            },
            BytecodeIndex::from_offset(0),
        );
        let mut expected = vec![0x4c, 0x89, 0x97];
        expected.extend_from_slice(&0x20i32.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn storeq_cell_relative_from_scratch1_encodes_rex_wr_89_modrm10_disp32() {
        // mov [rdi+0x28], r11 = REX.WR 4C 89, ModRM mod=10 reg=011(r11) rm=111=0x9F.
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::StoreQ {
                destination: cell_relative(0x28),
                source: P6X86_64BaselineSymbolicRegister::Scratch1,
            },
            BytecodeIndex::from_offset(0),
        );
        let mut expected = vec![0x4c, 0x89, 0x9f];
        expected.extend_from_slice(&0x28i32.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn guard_structure_id_scratch0_encodes_load32_cmp32_imm32_jne_rel32() {
        // C++: branch32(NotEqual, Address(base /* rdi */, structureIDOffset()),
        //               TrustedImm32(structure->id())) -> slow path.
        // Modeled: mov r10d, [rdi+0]; cmp r10d, 0xcafef00d; jne rel32 -> ud2 exit.
        //   load32 mov r10d,[rdi+0]: 44 8B 97 disp32 (REX.R, ModRM 10 010 111).
        //   cmp32  cmp r10d,imm32:   41 81 FA imm32  (REX.B, 81 /7, ModRM 11 111 010).
        //   jne rel32:               0F 85 rel32.
        // finish_side_exit_placeholders appends a 0x0F 0x0B (ud2) target and
        // patches the rel32. The jne target is the ud2 just past the branch.
        let bytecode_index = BytecodeIndex::from_offset(0);
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::GuardStructureId {
                base: P6X86_64BaselineSymbolicRegister::PropertyBase,
                scratch: P6X86_64BaselineSymbolicRegister::Scratch0,
                structure_id_offset: 0,
                cached_structure_id: 0xcafe_f00d,
                on_not_equal: structure_mismatch_label(0),
            },
            bytecode_index,
        );
        // load32 (7) + cmp32 (7) + jne rel32 (6) + ud2 (2) = 22 bytes.
        let mut expected = Vec::new();
        expected.extend_from_slice(&[0x44, 0x8b, 0x97]);
        expected.extend_from_slice(&0i32.to_le_bytes());
        expected.extend_from_slice(&[0x41, 0x81, 0xfa]);
        expected.extend_from_slice(&0xcafe_f00du32.to_le_bytes());
        // jne rel32 -> ud2 placeholder immediately after the 6-byte branch (rel=0).
        expected.extend_from_slice(&[0x0f, 0x85]);
        expected.extend_from_slice(&0i32.to_le_bytes());
        expected.extend_from_slice(&[0x0f, 0x0b]);
        assert_eq!(bytes, expected);
        // Confirm the jne actually targets the ud2 side-exit.
        let jne_end = 14 + 6;
        assert_eq!(rel32_branch_target(&bytes, jne_end as u32), jne_end as i64);
    }

    #[test]
    fn guard_structure_id_scratch1_encodes_load32_cmp32_imm32_jne_rel32() {
        // Same as above but scratch == r11:
        //   load32 mov r11d,[rdi+disp32]: 44 8B 9F disp32 (ModRM 10 011 111).
        //   cmp32  cmp r11d,imm32:        41 81 FB imm32  (ModRM 11 111 011).
        let bytecode_index = BytecodeIndex::from_offset(0);
        let bytes = round_trip_single_instruction(
            P6X86_64BaselineMachineInstruction::GuardStructureId {
                base: P6X86_64BaselineSymbolicRegister::PropertyBase,
                scratch: P6X86_64BaselineSymbolicRegister::Scratch1,
                structure_id_offset: 0,
                cached_structure_id: 0x0001_0002,
                on_not_equal: structure_mismatch_label(0),
            },
            bytecode_index,
        );
        let mut expected = Vec::new();
        expected.extend_from_slice(&[0x44, 0x8b, 0x9f]);
        expected.extend_from_slice(&0i32.to_le_bytes());
        expected.extend_from_slice(&[0x41, 0x81, 0xfb]);
        expected.extend_from_slice(&0x0001_0002u32.to_le_bytes());
        expected.extend_from_slice(&[0x0f, 0x85]);
        expected.extend_from_slice(&0i32.to_le_bytes());
        expected.extend_from_slice(&[0x0f, 0x0b]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn loadq_cell_relative_rejects_non_property_base() {
        // Only PropertyBase (rdi) is a valid cell base; PinnedCallFrameBase is
        // the frame-local path, so a cell-relative load from it is rejected.
        let contract = p6_backend_contract();
        let mut builder = P6X86_64SemanticByteBuilder::default();
        let result = emit_p6_x86_64_semantic_machine_instruction(
            &mut builder,
            &contract,
            BytecodeIndex::from_offset(0),
            P6X86_64BaselineMachineInstruction::LoadQ {
                destination: P6X86_64BaselineSymbolicRegister::ReturnGpr,
                source: memory_operand(
                    P6X86_64BaselineSymbolicRegister::Scratch2,
                    P6X86_64BaselineOperandLocation::CellRelative { disp32: 0 },
                ),
            },
            P6X86_64SemanticBranchEmissionMode::DirectBytecodeTargets,
        );
        // Scratch2 base is not the cell base / frame base, so the frame-local
        // fallback rejects it as an unsupported memory base.
        assert!(matches!(
            result,
            Err(P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation { .. })
                | Err(P6X86_64BaselineSemanticByteEmissionError::UnsupportedMemoryBase { .. })
        ));
    }

    #[test]
    fn guard_structure_id_rejects_non_property_base() {
        let contract = p6_backend_contract();
        let mut builder = P6X86_64SemanticByteBuilder::default();
        let result = emit_p6_x86_64_semantic_machine_instruction(
            &mut builder,
            &contract,
            BytecodeIndex::from_offset(0),
            P6X86_64BaselineMachineInstruction::GuardStructureId {
                base: P6X86_64BaselineSymbolicRegister::Scratch2,
                scratch: P6X86_64BaselineSymbolicRegister::Scratch0,
                structure_id_offset: 0,
                cached_structure_id: 1,
                on_not_equal: structure_mismatch_label(0),
            },
            P6X86_64SemanticBranchEmissionMode::DirectBytecodeTargets,
        );
        assert!(matches!(
            result,
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedMemoryBase {
                    base: P6X86_64BaselineSymbolicRegister::Scratch2,
                    ..
                }
            )
        ));
    }
}
