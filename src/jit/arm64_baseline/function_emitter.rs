//! ARM64 baseline FULL-FUNCTION emitter — `JIT::privateCompile`'s 3 passes
//! generalized over a whole `CodeBlock`'s bytecode into ONE callable ARM64 image
//! (emit -> relocate -> EXECUTE under W^X, including a native loop with a backward
//! back-edge). This is the dispatch Stage-1 core (docs/design/baseline-dispatch.md):
//! it lifts the standalone per-op `op_add`/`arith` execution proofs into a single
//! image driven by the CodeBlock instruction stream.
//!
//! C++ JSC map (source of truth):
//! - `jit/JIT.cpp` `JIT::privateCompile` (:813-815) runs THREE passes:
//!   `privateCompileMainPass` (the per-bytecode loop, `m_labels[m_bytecodeIndex] =
//!   label()` at :200 then `emit_op_X`), `privateCompileSlowCases` (:473-642, slow
//!   cases emitted AFTER the contiguous fast paths), and `privateCompileLinkPass`
//!   (:465, resolve every recorded jump through the label table). This module
//!   mirrors that exactly with three small bookkeeping vectors:
//!     * `labels: Vec<Option<Label>>` (one per bytecode index == JSC `m_labels`),
//!     * `jumps: Vec<(Jump, target_bci)>` (== `m_jmpTable`), resolved in LINK,
//!     * `slow: Vec<SlowCase>` (== `m_slowCases`), emitted after the epilogue (S6).
//! - Per-op lowerings:
//!   * op_enter (`jit/JITOpcodes.cpp` `emit_op_enter` :1475): zero-fill the callee
//!     locals with `jsUndefined()` at `addressFor(local)`.
//!   * op_mov (`emit_op_mov`): `load64(src) ; store64(dst)`.
//!   * the arith family (`jit/JITArithmetic.cpp` generators): reused from
//!     [`super::arith::emit_arith_fast_path`] (the SAME generator the standalone
//!     images use — int32 fast path plus the JSVALUE64 double fast path for
//!     add/sub/mul/div), with the slow guards deferred into a `SlowCase`.
//!   * the FUSED int32 compare-and-branch (`emit_op_jless`/`emitSlow_op_jless`,
//!     `JIT::emit_compareAndJump` JITArithmetic.cpp:215-227 / slow :127/:359): the
//!     genuinely-new control-flow piece — `branch32(cond, lhs, rhs)` straight to a
//!     BYTECODE-INDEX target, with a relational-operation slow path.
//!   * op_jmp (`emit_op_jmp`): an unconditional `b` to a bytecode-index target.
//!   * op_jfalse (`emit_op_jfalse`): boolean LSB test (`branchTest32`) with a
//!     `ToBoolean` slow path for non-boolean operands.
//!   * op_ret (`emit_op_ret`): `load64(value) -> returnValueGPR ; b done`.
//! - `jit/AssemblyHelpers.h` `addressFor(vreg) = Address(x29, vreg.offset()*8)`
//!   (:1290-1298, cfr == x29).
//!
//! BYTECODE-MODEL DIVERGENCES from JSC, all forced by this engine's existing
//! bytecode subsystem (`bytecode/opcode.rs` `CoreOpcode`, NOT JSC's opcode table)
//! and documented at their sites below:
//!   1. NO `op_enter` opcode exists in `CoreOpcode`; JSC emits op_enter as
//!      bytecode[0]. This emitter performs the equivalent callee-local zero-fill as
//!      a function-entry step keyed off the function's local count (scanned from
//!      the register operands).
//!   2. NO FUSED `op_jless`/`op_jnless`. The Rust bytecompiler emits a relational
//!      op (`LessThanInt32`/`LessEqualInt32`/`GreaterThanInt32`, == JSC `op_less`/
//!      `op_lesseq`/`op_greater`) followed by `JumpIfFalse`. This emitter re-fuses
//!      that canonical pair into JSC's `op_jnless`-style compare-and-branch — the
//!      identical machine code JSC's fused opcode emits.
//!   3. Branch targets are stored as ABSOLUTE `BytecodeIndex` (an ordinal into the
//!      linked instruction stream; the interpreter's `DispatchOutcome::Jump(target)`
//!      jumps to it directly), where JSC stores a signed branch OFFSET resolved as
//!      `bci + offset`. The LINK step is identical either way: resolve the target
//!      ordinal through the labels-by-bci table.
//!
//! Unsafe boundary: this module is entirely SAFE (`jit/mod.rs` is
//! `#![deny(unsafe_code)]`). It only COMPUTES instruction bytes; the execution
//! test composes the SAME three already-audited unsafe islands op_add/arith do
//! (the W^X executable call, the runtime-shim D1/D5 reborrows, the JSStack
//! provenance gate). It introduces NO new `unsafe`.
//!
//! SCOPE — INT32/PRIMITIVE only, the SAME forward prerequisites op_add/arith
//! document (object operands need the real active-frame `CodeBlock`; a real
//! `TypeError` cell; a pin-stable `Vm`). Property access and calls are out of scope
//! (the S4 allowlist gates them out for live dispatch; this standalone-image proof
//! only emits op_enter/mov/LoadInt32/int32-arith/fused-compare-branch/jmp/jfalse/
//! ret). This batch emits a STANDALONE image executed directly via the W^X path
//! (op_add's harness); the live tier-up wiring (U3 select_interpreter_entry_plan +
//! U4 B5-lite handoff) is the NEXT batch.

#![allow(dead_code)]

use crate::assembler::labels::{Jump, Label};
use crate::assembler::link_records::Arm64LinkRecord;
use crate::assembler::macro_assembler_arm64::{RelationalCondition, ResultCondition};
use crate::assembler::operands::{Address, TrustedImm32, TrustedImm64};
use crate::assembler::registers::RegisterID;
use crate::bytecode::BytecodeIndex;
use crate::bytecode::{CodeBlock, CoreOpcode, InstructionDecodeError, OperandAccessError};
use crate::jit::assembly_helpers::{AssemblyHelpers, TagRegistersMode};
use crate::jit::operations::{
    operation_call, operation_call_with_this, operation_compare_greater, operation_compare_less,
    operation_compare_lesseq, operation_get_by_id_optimize, operation_get_by_id_with_cached_offset,
    operation_get_by_val, operation_jfalse, operation_put_by_id_optimize,
    operation_put_by_id_with_cached_offset, operation_put_by_val,
    operation_resolve_baseline_native_entry, operation_throw_stack_overflow,
    MAX_REGISTER_CALL_ARGS, MAX_REGISTER_CALL_WITH_THIS_ARGS,
};
use crate::value::{JsValue, CELL_TAG, NUMBER_TAG, VALUE_TAG_MASK};
use crate::vm::jsstack::CallFrameSlot;

use super::arith::{emit_arith_fast_path, ArithFamilyOp, PINNED_VM_GPR};

/// `sizeof(Register)` (JSVALUE64); `addressFor` scales the VirtualRegister by it.
const REGISTER_SIZE_BYTES: i32 = 8;

/// `sizeof(CallerFrameAndPC)` (`CallFrame.h:109-112`; `sizeInRegisters == 2`): the
/// `[callerFrame, returnPC]` header pair. A call edge leaves `sp = calleeFrame +
/// sizeof(CallerFrameAndPC)` (JITCall.cpp:246) so the callee prologue's
/// `pushPair(fp,lr)` lands that pair at slots 0/1.
const CALLER_FRAME_AND_PC_SIZE_BYTES: i32 = 2 * REGISTER_SIZE_BYTES;

/// Bytes the prologue spills BELOW the reserved locals: two `pushPair`s — the
/// pinned-VM pair (x19/x20) and the tag-register pair (x27/x28) — at 16 bytes each
/// (`emit_prologue`). After the prologue `sp == fp - reservedLocalsBytes -
/// CALLEE_SAVE_SPILL_BYTES`; the native op_call edge restores exactly this so the
/// epilogue's `popPair`s (`emit_epilogue`) refill the right spill slots.
const CALLEE_SAVE_SPILL_BYTES: i32 = 32;

// --- Register identity (GPRInfo.h, ARM64 baseline) — the canonical conventions
//     op_add/arith established, kept identical so this emitter does not drift. ----

/// `x29 == cfr` (GPRInfo.h:582; AssemblyHelpers.h:1290-1298).
const CALL_FRAME_GPR: RegisterID = RegisterID::Fp;
/// `x30 == lr`. Saved/restored around the operation far-calls.
const LINK_GPR: RegisterID = RegisterID::Lr;
/// Raw C-ABI `x1`. Pre-A1 the prologue moved this (the scratch-arena base) into
/// `cfr`; the faithful Option-A prologue now does `mov fp,sp` instead, so the JIT
/// CallFrame is built on the native machine stack and `x1` is no longer the frame
/// source. Kept for the register-map record (`dead_code` at module level).
const RAW_FRAME_ARG_GPR: RegisterID = RegisterID::X1;
/// The `*mut Vm` arrives in raw C-ABI `x0` and is the operation shims' arg0.
const RAW_VM_ARG_GPR: RegisterID = RegisterID::X0;
/// `leftRegs == argumentGPR1` (x1): op1 / compare lhs, reused as a slow-call arg1.
const LEFT_GPR: RegisterID = RegisterID::X1;
/// `rightRegs == argumentGPR2` (x2): op2 / compare rhs, reused as a slow-call arg2.
const RIGHT_GPR: RegisterID = RegisterID::X2;
/// `resultRegs == returnValueGPR` (x0): the scratch/result; the slow path's arg0
/// (the Vm) and its return value; op_ret's return register.
const RESULT_GPR: RegisterID = RegisterID::X0;
/// Paired with `PINNED_VM_GPR` (x19) only to keep `stp`/`ldp` 16-byte aligned.
const PINNED_VM_PAIR_GPR: RegisterID = RegisterID::X20;
/// `numberTagRegister` (x27) / `notCellMaskRegister` (x28), the `HaveTagRegisters`
/// pair the int32 guards/box rely on. Spilled in the prologue, refilled in the
/// epilogue.
const NUMBER_TAG_GPR: RegisterID = AssemblyHelpers::NUMBER_TAG_REGISTER;
const NOT_CELL_MASK_GPR: RegisterID = AssemblyHelpers::NOT_CELL_MASK_REGISTER;

/// Caller-saved scratch registers for the per-op fast paths (the undefined/boxed-
/// immediate carrier for op_enter/op_mov/LoadInt32, the post-call exception-word
/// probe, and the `branchIfNotBoolean` temporaries). All caller-saved (AAPCS64
/// x1-x15), live only within a single op's emission, so they need no spill.
const SCRATCH_GPR: RegisterID = RegisterID::X3;
const SCRATCH_GPR_B: RegisterID = RegisterID::X4;
/// Holds the `TrustedImm32(1)` LSB mask for the op_jfalse boolean test.
const BOOL_MASK_GPR: RegisterID = RegisterID::X5;

/// A1.x broad native-call engagement: holds the runtime-RESOLVED callee entry between
/// the resolver far-call (which returns it in `x0`) and the native `blr` that jumps to
/// it, surviving the callee-frame setup (which clobbers `x0`/`x3`/`x4`/`sp`). `x6`
/// (`CALL_ARG_GPRS[3]`) is caller-saved and is used ONLY by the slow path's register
/// argument lane — never by the native fast path's frame setup — so it is free to
/// carry the entry across the native setup. The native setup contains NO call before
/// its terminal `blr`, so the entry is never clobbered by an intervening callee.
const NATIVE_ENTRY_GPR: RegisterID = RegisterID::X6;

/// Caller-saved scratch registers (AAPCS64 x9-x14, none of them the assembler's
/// internal `DATA_TEMP`/`MEMORY_TEMP` = x16/x17) for the `get_by_id`/`put_by_id`
/// DataIC structure-guard fast path: the boxed-base load, the is-cell guard, the
/// pointer unbox, the cell + cached structure-id loads, and the baked record
/// address. All are dead after the site (the slow/hit far-call clobbers them
/// anyway), so they need no spill; none aliases the live tag (`x27`) / pinned-VM
/// (`x19`) / cfr (`x29`) callee-saved registers.
const PROPERTY_BASE_GPR: RegisterID = RegisterID::X9;
const PROPERTY_TMP_GPR: RegisterID = RegisterID::X10;
const PROPERTY_CELL_GPR: RegisterID = RegisterID::X11;
const PROPERTY_STRUCTURE_ID_GPR: RegisterID = RegisterID::X12;
const PROPERTY_RECORD_GPR: RegisterID = RegisterID::X13;
const PROPERTY_CACHED_STRUCTURE_GPR: RegisterID = RegisterID::X14;

/// `StructureID::CELL_STRUCTURE_ID_OFFSET` (structure_cell.rs:105): `JSCell`'s
/// `m_structureID` is the first field, so the shape guard reads `[cell + 0]`.
const CELL_STRUCTURE_ID_OFFSET: i32 = 0;
/// `sizeof(HandlerPropertyInlineCacheRecord)` (`#[repr(C)]`
/// `{structure_id: u32@+0, offset: i32@+4, holder_ptr: u64@+8}`, ic.rs:1529): the
/// per-site record stride the generated guard indexes (`record_base +
/// record_index * 16`), the ARM64 analog of the x86-64 emitter's `record_index*16`
/// (jit/emitter.rs:6961).
const PROPERTY_IC_RECORD_STRIDE: usize = 16;

/// The AAPCS64 integer/pointer argument registers `x3..x7` the `op_call` lowering
/// loads the boxed call arguments into (`x0`=vm, `x1`=callee, `x2`=argc are the
/// preceding three; see [`crate::jit::operations::operation_call`]). Indexed by the
/// op_call argument position (0-based, EXCLUDING `this`); the emitter declines a call
/// site whose arity exceeds `MAX_REGISTER_CALL_ARGS == CALL_ARG_GPRS.len()`.
const CALL_ARG_GPRS: [RegisterID; MAX_REGISTER_CALL_ARGS] = [
    RegisterID::X3,
    RegisterID::X4,
    RegisterID::X5,
    RegisterID::X6,
    RegisterID::X7,
];

/// op_call_with_this slow-path register assignment (`operation_call_with_this`): the
/// receiver `this` consumes `x2`, so the layout is `x0`=vm, `x1`=callee, `x2`=this,
/// `x3`=argc, `x4..x7`=arg0..arg3 — ONE fewer arg register than plain op_call
/// (`MAX_REGISTER_CALL_WITH_THIS_ARGS == CALL_WITH_THIS_ARG_GPRS.len() == 4`). A
/// method-call site with more explicit arguments is declined by the S4 gate.
const CALL_WITH_THIS_THIS_GPR: RegisterID = RegisterID::X2;
const CALL_WITH_THIS_ARGC_GPR: RegisterID = RegisterID::X3;
const CALL_WITH_THIS_ARG_GPRS: [RegisterID; MAX_REGISTER_CALL_WITH_THIS_ARGS] = [
    RegisterID::X4,
    RegisterID::X5,
    RegisterID::X6,
    RegisterID::X7,
];

/// `AssemblyHelpers::addressFor(VirtualRegister) = Address(x29, vreg.offset()*8)`
/// (AssemblyHelpers.h:1290-1298). The operand IS the VirtualRegister's raw value.
fn address_for(operand: i32) -> Address {
    Address::new(CALL_FRAME_GPR, operand.wrapping_mul(REGISTER_SIZE_BYTES))
}

/// Bytes the prologue reserves below `fp` for the callee locals (`sub sp, fp,
/// #localsBytes`). The local count is rounded UP to an even number of registers
/// so the reservation is a multiple of 16 and keeps `sp` 16-aligned for the
/// slow-path far-calls (the AAPCS64 / JSC `stackAlignmentRegisters == 2` rule).
fn reserved_locals_bytes(num_locals: u32) -> i32 {
    let even = num_locals.checked_add(num_locals & 1).unwrap_or(num_locals);
    (even as i32).wrapping_mul(REGISTER_SIZE_BYTES)
}

/// Round a positive byte count UP to a 16-byte (2-register) multiple — JSC's
/// `stackAlignmentBytes()`/`stackAlignmentRegisters == 2` rule for a call edge.
fn round_up_to_16(bytes: i32) -> i32 {
    (bytes + 15) & !15
}

/// An emitted (not-yet-finalized) full-function image — the assembler bytes plus
/// the branch link records the LinkBuffer pass resolves. Mirrors op_add's
/// `OpAddImage` (the `(code, jumpsToLink)` a `JIT`/`LinkBuffer` carries before
/// `finalizeCodeWithoutDisassembly`).
pub(crate) struct FunctionImage {
    pub(crate) code: Vec<u8>,
    pub(crate) link_records: Vec<Arm64LinkRecord>,
    /// The native JIT->JIT call sites this image emitted — one per op_call that has a
    /// native fast path: the pre-seeded [`LinkedCallTarget`] proof AND, since broad
    /// engagement, every live op_call (`emit_op_call_dynamic` emits the native fast
    /// path inline, taken at runtime when the callee resolves to an installed image).
    /// Carries each call edge's `returnPC`/calleeFrame geometry for the R-lever proof;
    /// the install path does not consume it (it is verification metadata).
    pub(crate) linked_call_sites: Vec<LinkedCallSite>,
}

/// A1.3 (first cut): a resolved native call target for ONE op_call site — the
/// absolute entry address of the callee's installed baseline image. This is the
/// emit-time analog of a LINKED monomorphic `CallLinkInfo`'s
/// `m_monomorphicCallDestination` (`bytecode/CallLinkInfo.cpp:312,338`): JSC LOADS
/// that destination from the patchable `CallLinkInfo` data IC at runtime and
/// `jit.call`s it (`emitFastPathImpl` :338,363). This FIRST CUT instead BAKES the
/// absolute callee entry as a `blr` immediate, because a near `bl` reaches only
/// ±128 MB and two separately-`mmap`'d RX images can sit farther apart; the
/// repatchable load-from-`CallLinkInfo` (a real data IC) is the deferred follow-up.
/// Keyed by the op_call's bytecode index.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinkedCallTarget {
    pub(crate) bytecode_index: usize,
    pub(crate) entry: usize,
}

/// The geometry of one emitted native JIT->JIT call edge, recorded so the call's
/// CallFrame header (`CallFrame.h:109-112`) can be cross-checked against the live
/// native stack. `return_pc_offset` is the byte offset into [`FunctionImage::code`]
/// of the instruction AFTER the `blr` (== `returnPC`); after finalize it maps to
/// `image_base + return_pc_offset`. `callee_frame_offset_from_fp` is the signed
/// fp-relative byte offset of the callee `CallFrame*` (slot 0), so the callee frame
/// sits at `caller_fp + callee_frame_offset_from_fp`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinkedCallSite {
    pub(crate) bytecode_index: usize,
    pub(crate) return_pc_offset: usize,
    pub(crate) callee_frame_offset_from_fp: i32,
}

/// Where the native `op_call` fast path's terminal call instruction jumps.
#[derive(Clone, Copy)]
enum NativeCallTarget {
    /// `far_call(imm)`: an ABSOLUTE callee entry materialized into the data-temp and
    /// `blr`'d — the A1.3 pre-seeded [`LinkedCallTarget`] proof (the entry is known at
    /// emit time).
    Absolute(usize),
    /// `call_register(reg)`: the callee entry is already in `reg`, RESOLVED at runtime
    /// by the per-call resolver (A1.x broad engagement). A single `blr reg`.
    Register(RegisterID),
}

/// The source of the callee frame's `this` (receiver) slot — the ONLY delta between
/// plain `op_call` and the method-call `op_call_with_this` (JSC's `compileOpCall` is
/// templated over both; only the `thisValue` operand differs, JITCall.cpp:64-145).
#[derive(Clone, Copy)]
enum ThisSource {
    /// `op_call`: the implicit receiver is `jsUndefined()`
    /// (`CallObservationThisSource::ImplicitUndefined`).
    Undefined,
    /// `op_call_with_this`: the receiver is the value in this caller-frame slot (the
    /// explicit `this` operand), stored into the callee frame's `thisArgument` slot.
    Receiver(i32),
}

/// Failure modes of [`emit_baseline_function`]. A real CodeBlock that contains any
/// of these (an unsupported opcode, a constant-pool operand, a non-fused relational
/// op) is REJECTED — exactly the role of JSC's S4 baseline-allowlist gate: the
/// function stays in the interpreter rather than being mis-lowered.
#[derive(Clone, Debug)]
pub(crate) enum EmitFunctionError {
    /// The instruction stream could not be decoded at a bytecode index.
    Decode(InstructionDecodeError),
    /// An operand was missing or the wrong kind.
    Operand(OperandAccessError),
    /// The opcode is outside the Stage-1 int32/control-flow allowlist.
    UnsupportedOpcode(CoreOpcode),
    /// The decoded opcode is not a known `CoreOpcode`.
    UnknownOpcode,
    /// A register operand addressed the constant pool (not a frame slot); the
    /// constant-pool load lowering is deferred.
    ConstantOperand,
    /// A relational op was not immediately followed by a `JumpIfFalse` reading its
    /// result; the standalone boolean-producing relational lowering (a `cset`
    /// materialization) is deferred — only the FUSED compare-and-branch is emitted.
    StandaloneRelational(CoreOpcode),
    /// A branch operand named a bytecode-index target outside the instruction
    /// stream (or an unlabeled index). A malformed/unsupported stream is REJECTED
    /// (the S4 gate's safe-by-rejection posture), never linked against a panic.
    InvalidBranchTarget { target_bci: usize },
    /// An `op_call` site has more explicit arguments than the B5-first-cut
    /// register-passing ABI carries (`MAX_REGISTER_CALL_ARGS`). DECLINED so the
    /// CodeBlock stays in the interpreter (the S4 gate); the general arity arrives
    /// with the native direct-link (B5-full).
    UnsupportedCallArity { argc: u32 },
    /// A `get_by_id`/`put_by_id` DataIC site was reached but no baseline data-IC
    /// record store is installed on the CodeBlock (its stable base is what the
    /// structure guard bakes). DECLINED (the S4 gate's safe-by-rejection posture):
    /// the install path (`install_baseline_function`) must allocate the store
    /// sized to the property-site count BEFORE emitting; a caller that emits
    /// without installing (e.g. a non-property proof harness) cannot lower
    /// property access and the function stays in the interpreter.
    MissingPropertyRecordStore,
    /// The CodeBlock has no instructions.
    EmptyFunction,
}

impl From<InstructionDecodeError> for EmitFunctionError {
    fn from(error: InstructionDecodeError) -> Self {
        EmitFunctionError::Decode(error)
    }
}

impl From<OperandAccessError> for EmitFunctionError {
    fn from(error: OperandAccessError) -> Self {
        EmitFunctionError::Operand(error)
    }
}

/// A deferred slow case (== a JSC `SlowCaseEntry`): the fast-path guard `Jump`s that
/// branch into this slow block, the fall-through `resume_bci`, and the per-kind
/// payload. Collected during MAIN and emitted AFTER the epilogue (S6 / JSC
/// `privateCompileSlowCases`).
struct SlowCase {
    fast_jumps: Vec<Jump>,
    resume_bci: usize,
    kind: SlowKind,
}

/// The per-op slow-case shapes.
enum SlowKind {
    /// int32 arith family (`emitSlow_op_add`/`_sub`/...): far-call the operation,
    /// probe the exception word, store the result to `dst`, resume.
    BinaryOp { shim: usize, dst: i32 },
    /// fused int32 compare-and-branch (`emitSlow_op_jless`): far-call the relational
    /// operation -> boolean 0/1, probe the exception word, then `branchTest32(Zero)`
    /// to the branch target (JumpIfFalse takes the branch when the comparison is
    /// false), else resume.
    CompareBranch { shim: usize, target_bci: usize },
    /// op_jfalse on a non-boolean (`emitSlow_op_jfalse`): far-call `operation_jfalse`
    /// (`ToBoolean`, inverted) -> 0/1, then `branchTest32(NonZero)` to the target,
    /// else resume. Infallible (no exception probe).
    TruthyBranch { shim: usize, target_bci: usize },
}

/// The thin bci-bookkeeping struct OVER the assembler `Jump`/`Label` path (S5): it
/// owns the `AssemblyHelpers` and the three JSC pass-state vectors. This is the
/// "lift the control-flow builder's bci bookkeeping as a thin struct over the
/// assembler path" decision — there is no second abstract branch model.
struct FunctionEmitter {
    h: AssemblyHelpers,
    /// One label per bytecode index (== JSC `m_labels`); filled in MAIN.
    labels: Vec<Option<Label>>,
    /// Recorded branches to bytecode-index targets (== JSC `m_jmpTable`).
    jumps: Vec<(Jump, usize)>,
    /// Jumps to the shared epilogue (`done`): op_ret + the exception stub.
    done_jumps: Vec<Jump>,
    /// Jumps to the shared exception stub (the slow paths' pending-exception edge).
    exception_jumps: Vec<Jump>,
    /// Deferred slow cases (== JSC `m_slowCases`).
    slow: Vec<SlowCase>,
    /// Resolved link records for jumps that target a code LABEL directly (not a
    /// bytecode index) — the fast-path guards into their slow blocks.
    label_link_records: Vec<Arm64LinkRecord>,
    /// The baked `AbsoluteAddress` of `Vm::m_exception` (D3); see op_add prereq #3.
    jit_pending_address: usize,
    /// A1.4: the baked `AbsoluteAddress` of `Vm::jit_soft_stack_limit`
    /// (`VM::addressOfSoftStackLimit`, VM.h:919). The prologue stack-overflow check
    /// LOADs `*soft_stack_limit_address` and compares the new frame top against it
    /// (JIT.cpp:781). Like `jit_pending_address`, it must stay valid for the call's
    /// duration (a pin-stable `Vm`).
    soft_stack_limit_address: usize,
    /// A1.4: the prologue's stack-overflow guard jump(s) to the overflow stub
    /// (emitted after the exception stub, like `done_jumps`/`exception_jumps`).
    stack_overflow_jumps: Vec<Jump>,
    /// Bytes the prologue reserved below `fp` for this function's callee locals
    /// (`reserved_locals_bytes(num_locals)`), captured in [`Self::emit_prologue`].
    /// The native op_call edge (A1.2) needs it to place the callee frame BELOW the
    /// caller's whole used region and to restore `sp` after the call.
    reserved_locals_bytes: i32,
    /// A1.2/A1.3: native JIT->JIT call sites emitted in this image (one per op_call
    /// resolved to a [`LinkedCallTarget`]); surfaced on the [`FunctionImage`].
    linked_call_sites: Vec<LinkedCallSite>,
    /// The dense per-site `property_site_index` the MAIN pass assigns to each
    /// `get_by_id`/`put_by_id` DataIC site in bytecode order (the
    /// `HandlerPropertyInlineCacheRecord` store index the slow-path bridge fills).
    /// Bumped once per admitted GetByName/PutByName; the install path sizes the
    /// record store to the SAME bytecode-order count, so every emitted record
    /// index is in bounds.
    property_site_index: u32,
}

const MODE: TagRegistersMode = TagRegistersMode::HaveTagRegisters;

impl FunctionEmitter {
    fn new(
        instruction_count: usize,
        jit_pending_address: usize,
        soft_stack_limit_address: usize,
    ) -> Self {
        Self {
            h: AssemblyHelpers::new(),
            labels: vec![None; instruction_count],
            jumps: Vec::new(),
            done_jumps: Vec::new(),
            exception_jumps: Vec::new(),
            slow: Vec::new(),
            label_link_records: Vec::new(),
            jit_pending_address,
            soft_stack_limit_address,
            stack_overflow_jumps: Vec::new(),
            reserved_locals_bytes: 0,
            linked_call_sites: Vec::new(),
            property_site_index: 0,
        }
    }

    /// The faithful `AssemblyHelpers::emitFunctionPrologue` (AssemblyHelpers.h:
    /// 558-563): `pushPair(fp,lr); mov fp,sp`, building this function's CallFrame on
    /// the NATIVE machine stack (`fp/x29 == calleeFrame`). The caller positioned
    /// `sp = calleeFrame + sizeof(CallerFrameAndPC)` (`LowLevelInterpreter64.asm`
    /// `makeJavaScriptCall`), so `pushPair` lands the `[callerFrame,returnPC]` pair
    /// at slots 0/1 and `mov fp,sp` captures the frame base.
    ///
    /// DIVERGENCE-CORRECTION (A1, Option A — see docs/design/jsstack.md "B5/STACK
    /// MODEL"): the prior `mov fp,x1` made `fp` a per-function SCRATCH-ARENA base
    /// while `sp` stayed on the native stack — the latent Option-D header split
    /// (fp=arena / sp=native) that optimized AROUND the divergence. This is the
    /// faithful `mov fp,sp` that UNIFIES them onto the native stack, so `op_ret`'s
    /// epilogue, a future JIT->JIT `bl`, and a GC stack-walk all see ONE stack with
    /// `callerFrame`@0/`returnPC`@1 adjacent in the frame header.
    ///
    /// After capturing `fp`, reserve `num_locals` (rounded up to keep `sp`
    /// 16-aligned) slots BELOW `fp` — the prologue's `sub sp, fp, #localsBytes` —
    /// BEFORE spilling the callee-saved registers, so the callee-save spills land
    /// below the locals and `emit_op_enter`'s local zero-fill (which writes
    /// `[fp-8..]`) cannot clobber them. (Pre-flip this was unnecessary: `fp` was a
    /// separate arena, so the native-stack callee-save spills never overlapped the
    /// arena locals.)
    fn emit_prologue(&mut self, num_locals: u32) {
        self.h.masm_mut().push_pair(CALL_FRAME_GPR, LINK_GPR); // stp fp,lr,[sp,#-16]!
        self.h.masm_mut().move_rr(RegisterID::Sp, CALL_FRAME_GPR); // mov fp, sp (cfr)
        let reserved = reserved_locals_bytes(num_locals);
        // Captured for the native op_call edge (A1.2): the callee frame goes BELOW
        // `fp - reserved - CALLEE_SAVE_SPILL_BYTES` and `sp` is restored to it.
        self.reserved_locals_bytes = reserved;
        if reserved > 0 {
            // `sub sp, sp, #reserved` (== `sub sp, fp, #reserved`, fp==sp here):
            // a negative add immediate folds to a sub immediate.
            self.h.masm_mut().add64_imm(
                TrustedImm32::new(-reserved),
                RegisterID::Sp,
                RegisterID::Sp,
            );
        }
        // A1.4 STACK-OVERFLOW CHECK (JIT.cpp:773-783): after `emitFunctionPrologue`
        // + the locals reservation, JSC computes the frame top into `regT1` and
        // `branchPtr(GreaterThan, AbsoluteAddress(vm.addressOfSoftStackLimit()),
        // regT1)` to the stack-overflow thunk BEFORE `move(regT1, sp)` /
        // emitSaveCalleeSaves. Emitted HERE — before the callee-save spills descend
        // `sp` and WRITE — so an over-deep frame is caught with nothing written below
        // `fp` and `sp` still at `fp - localsBytes`; the overflow stub then needs no
        // callee-save refill. `x0` still holds the raw `*mut Vm`;
        // `SCRATCH_GPR`/`SCRATCH_GPR_B` (caller-saved x3/x4) are free here (the tag /
        // pinned-VM registers are materialized only after this point).
        //
        // The frame top is `fp - localsBytes - calleeSaveSpillBytes` (the lowest slot
        // the prologue+body will write). DIVERGENCE (first cut, documented): JSC's
        // `frameTopOffset` (`stackPointerOffsetFor`) ALSO reserves the maximal
        // OUTGOING-call argument area; this checks the locals + callee-save frame top
        // only. The bounded op_call outgoing area and the throw shim's own C frame are
        // absorbed by the generous soft-reserved zone above the guard page, and a
        // native JIT->JIT callee re-checks in its OWN prologue, so deep recursion is
        // still caught frame-by-frame.
        let frame_top_bytes = reserved + CALLEE_SAVE_SPILL_BYTES;
        // regT1 := fp - frameTopBytes (the new frame's lowest address). The negative
        // immediate folds to a `sub`; a large one sign-extends into the assembler's
        // own data temp, not SCRATCH_GPR/_B.
        self.h.masm_mut().add64_imm(
            TrustedImm32::new(-frame_top_bytes),
            CALL_FRAME_GPR,
            SCRATCH_GPR,
        );
        // SCRATCH_GPR_B := *softStackLimit (the `AbsoluteAddress` value LOAD).
        self.h.masm_mut().move_imm64(
            TrustedImm64::new(self.soft_stack_limit_address as i64),
            SCRATCH_GPR_B,
        );
        self.h
            .masm_mut()
            .load64(Address::new(SCRATCH_GPR_B, 0), SCRATCH_GPR_B);
        // branch if softStackLimit > frameTop -> overflow stub (signed GreaterThan ==
        // JSC's `branchPtr(GreaterThan, ...)`; stack addresses are positive in the
        // signed i64 range, so signed pointer comparison is faithful).
        let overflow = self.h.masm_mut().branch64(
            RelationalCondition::GreaterThan,
            SCRATCH_GPR_B,
            SCRATCH_GPR,
        );
        self.stack_overflow_jumps.push(overflow);
        self.h
            .masm_mut()
            .push_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // spill x19(+x20)
        self.h
            .masm_mut()
            .push_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // spill x27/x28
        self.h.masm_mut().move_rr(RAW_VM_ARG_GPR, PINNED_VM_GPR); // pinned-VM := *mut Vm
        self.h.emit_materialize_tag_check_registers(); // x27 = NumberTag, x28 = NotCellMask
    }

    /// op_enter (`emit_op_enter`, JITOpcodes.cpp:1475): zero-fill the callee locals
    /// with `jsUndefined()`. DIVERGENCE #1: the Rust `CoreOpcode` set has no Enter
    /// opcode, so the emitter performs this as the function-entry step over the
    /// `num_locals` negative var slots (positive argument slots, set up by the
    /// caller, are NOT touched — exactly like JSC, which fills only the vars).
    fn emit_op_enter(&mut self, num_locals: u32) {
        let undefined_bits = JsValue::undefined().encoded().0;
        self.h
            .masm_mut()
            .move_imm64(TrustedImm64::new(undefined_bits as i64), SCRATCH_GPR);
        for local_index in 0..num_locals {
            // VirtualRegister::local(i).raw() == -(i+1): the negative var slots.
            let operand = -((local_index as i32) + 1);
            self.h.masm_mut().store64(SCRATCH_GPR, address_for(operand));
        }
    }

    /// op_mov (`emit_op_mov`): `load64(src) ; store64(dst)`. Falls through.
    fn emit_op_mov(&mut self, dst: i32, src: i32) {
        self.h.masm_mut().load64(address_for(src), SCRATCH_GPR);
        self.h.masm_mut().store64(SCRATCH_GPR, address_for(dst));
    }

    /// LoadInt32 (`op_mov` of an int32 constant): materialize the BOXED int32
    /// immediate (`NumberTag | uint32(value)`, the exact inverse of `JsValue::from_i32`)
    /// and store it. DIVERGENCE (instruction selection): JSC's `op_mov` loads the
    /// constant from the constant pool; this materializes the identical boxed value
    /// directly (the constant-pool slot lowering is deferred). Falls through.
    fn emit_load_int32(&mut self, dst: i32, value: i32) {
        let boxed = NUMBER_TAG | (value as u32 as u64);
        self.h
            .masm_mut()
            .move_imm64(TrustedImm64::new(boxed as i64), SCRATCH_GPR);
        self.h.masm_mut().store64(SCRATCH_GPR, address_for(dst));
    }

    /// LoadDouble (`op_mov` of a double/number constant): materialize the BOXED
    /// double immediate and store it. JSC has NO separate LoadDouble opcode — a
    /// number literal lives in the constant pool and is loaded by `op_mov`, whose
    /// baseline lowering `emitGetVirtualRegister(constant, dst)` is
    /// `moveValue(getConstant(src), dst)` == `move(Imm64(JSValue::encode(value)),
    /// gpr)` then `emitPutVirtualRegister(dst)` == `storeValue` (JITInlines.h:367-374
    /// / AssemblyHelpers.h:342-346) — exactly the int32 shape above but with a
    /// double constant.
    ///
    /// This engine's bytecode carries the raw f64 bits in two 32-bit immediate
    /// operands (`CoreOpcode::LoadDouble`, interpreter/mod.rs:6412-6431). The BOXING
    /// is folded in at emit time via `JsValue::from_double` (value/repr.rs:593) so
    /// the materialized 64-bit immediate is BIT-IDENTICAL to the interpreter's
    /// `RuntimeValue::from_double(f64::from_bits(bits))` — INCLUDING the canonical
    /// representation choice: `from_double` canonicalizes an exactly-representable
    /// integral double (e.g. `2.0`) to a BOXED INT32, and otherwise offset-encodes
    /// the double (`bits + DoubleEncodeOffset`). JSC makes the same choice when it
    /// builds the constant pool (`jsNumber(d)` -> the int32 immediate when in range),
    /// so computing it here keeps the JIT and interpreter value reps in lockstep.
    /// DIVERGENCE (instruction selection, identical to `emit_load_int32`): the
    /// already-boxed immediate is materialized directly rather than loaded from a
    /// constant-pool slot. Falls through.
    fn emit_load_double(&mut self, dst: i32, bits: u64) {
        let boxed = JsValue::from_double(f64::from_bits(bits)).encoded().0;
        self.h
            .masm_mut()
            .move_imm64(TrustedImm64::new(boxed as i64), SCRATCH_GPR);
        self.h.masm_mut().store64(SCRATCH_GPR, address_for(dst));
    }

    /// The arith family fast path (`emit_op_add`/`_sub`/`_mul`/`_div`/...): load
    /// both operands into `argumentGPR1`/`argumentGPR2`, run the SHARED generator
    /// (which boxes + stores the result, emits the JSVALUE64 double fast path for
    /// add/sub/mul/div, and returns the slow guards), and DEFER the slow case after
    /// the epilogue (S6). The int32 path's skip-over jumps (`fast.end`, present for
    /// add/sub/mul) target the next bytecode; the path that falls through the
    /// generator continues into it. The double path's own control-flow branches
    /// (`fast.internal_links`) are already resolved in place.
    fn emit_op_arith(
        &mut self,
        op: ArithFamilyOp,
        dst: i32,
        lhs: i32,
        rhs: i32,
        resume_bci: usize,
    ) {
        // emitGetVirtualRegister(lhs, leftRegs) / (rhs, rightRegs).
        self.h.masm_mut().load64(address_for(lhs), LEFT_GPR);
        self.h.masm_mut().load64(address_for(rhs), RIGHT_GPR);
        let fast = emit_arith_fast_path(&mut self.h, op, dst, MODE);
        for end_jump in fast.end {
            self.jumps.push((end_jump, resume_bci));
        }
        self.label_link_records.extend(fast.internal_links);
        self.slow.push(SlowCase {
            fast_jumps: fast.slow,
            resume_bci,
            kind: SlowKind::BinaryOp {
                shim: op.operation_shim(),
                dst,
            },
        });
    }

    /// op_get_by_val (`emit_op_get_by_val`, JITPropertyAccess.cpp) — the asm.js HEAP
    /// element READ. SLOW-CALL lowering: load the boxed base/key into the C-ABI arg
    /// slots (`argumentGPR1`/`argumentGPR2`), set arg0 = the pinned `*mut Vm`, and
    /// far-call `operation_get_by_val` (the typed-array element shortcut + the
    /// general value-keyed funnel); probe the `m_exception` mirror (D3) for the throw
    /// edge, then store the boxed result to `dst`. Falls through to the next bytecode.
    ///
    /// DIVERGENCE from JSC (load-bearing, the inline fast-path follow-up): JSC emits
    /// an INLINE ArrayMode-dispatched typed-array IC stub that loads the view's raw
    /// `m_vector` (a `CagedPtr`) off the cell and indexes it. This engine's R4 cell
    /// carries an `AuxiliaryHandle` into a RELOCATABLE store-owned `Vec<Vec<u8>>`
    /// slab, NOT a raw backing pointer — so the element load cannot be emitted inline
    /// (it would have to bake `Vec` field offsets + the slab base, neither a stable
    /// ABI) until the typed-array cell carries a stable raw backing pointer (the
    /// inline fast path's prerequisite, a serial cell-representation decision). Until
    /// then the load is a leaf far-call, exactly the arith slow-call discipline.
    fn emit_get_by_val(&mut self, dst: i32, base: i32, key: i32) {
        self.h.masm_mut().load64(address_for(base), LEFT_GPR); // x1 = base (arg1)
        self.h.masm_mut().load64(address_for(key), RIGHT_GPR); // x2 = key (arg2)
        self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm (arg0)
        self.h
            .masm_mut()
            .far_call(TrustedImm64::new(operation_get_by_val as usize as i64));
        // D3 throw edge: a thrown get (e.g. a getter, a nullish base) stamps the
        // mirror; branch to the shared exception stub BEFORE storing the empty result.
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst)); // dst = x0 (boxed result)
    }

    /// op_put_by_val (`emit_op_put_by_val`, JITPropertyAccess.cpp) — the asm.js HEAP
    /// element STORE. SLOW-CALL lowering: load the boxed base/key/value into the
    /// C-ABI arg slots (`argumentGPR1`/`GPR2`/`GPR3`), set arg0 = the pinned
    /// `*mut Vm`, and far-call `operation_put_by_val`; probe the `m_exception` mirror
    /// (D3). put_by_val yields no observable value, so there is no `dst` store (the
    /// returned `undefined`/empty register is discarded). Falls through. Same inline
    /// fast-path follow-up divergence as [`Self::emit_get_by_val`].
    fn emit_put_by_val(&mut self, base: i32, key: i32, value: i32) {
        self.h.masm_mut().load64(address_for(base), LEFT_GPR); // x1 = base (arg1)
        self.h.masm_mut().load64(address_for(key), RIGHT_GPR); // x2 = key (arg2)
        self.h.masm_mut().load64(address_for(value), SCRATCH_GPR); // x3 = value (arg3)
        self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm (arg0)
        self.h
            .masm_mut()
            .far_call(TrustedImm64::new(operation_put_by_val as usize as i64));
        // D3 throw edge: a thrown store (e.g. a setter, a nullish base, a ToNumber
        // throw) stamps the mirror; branch to the shared exception stub. `SCRATCH_GPR`
        // (the value arg, now consumed) is reused by the probe.
        self.emit_exception_probe();
    }

    /// The baseline `get_by_id`/`put_by_id` DataIC structure guard, shared by both
    /// (`generateGetByIdInlineAccessBaselineDataIC`, JITInlineCacheGenerator.cpp:
    /// 140-183; the x86-64 analog jit/emitter.rs:6978-6994). Emits, into the guard
    /// scratch registers:
    /// 1. `load64 [cfr+base]` -> the boxed base value;
    /// 2. `branchIfNotCell` = `!isNumber() && (bits & VALUE_TAG_MASK) == CELL_TAG`
    ///    (the transitional cell test, value/repr.rs:684) -> two guard-miss jumps;
    /// 3. `lsr #8` UNBOX to the raw `CoreObjectCell*` (value/repr.rs:645 inverse);
    /// 4. `load32 [cell+0]` (receiver `StructureID`) vs `load32 [record+0]` (the
    ///    cached id, from the baked `record_addr`) -> one guard-miss jump on mismatch.
    /// Returns the guard-miss `Jump`s the caller links to its SLOW (optimize) block;
    /// on fall-through the receiver structure matched the cached record (a HIT). A
    /// SENTINEL record (`structure_id == 0`) never equals a real `StructureID`, so an
    /// unfilled site always misses until the slow-path bridge fills it.
    fn emit_property_ic_structure_guard(&mut self, base: i32, record_addr: usize) -> Vec<Jump> {
        let mut miss = Vec::with_capacity(3);
        // emitGetVirtualRegister(base): the boxed base value.
        self.h
            .masm_mut()
            .load64(address_for(base), PROPERTY_BASE_GPR);
        // branchIfNotCell part 1: a number (any NumberTag bit set) is not a cell.
        // x27 == numberTagRegister (materialized in the prologue).
        miss.push(self.h.masm_mut().branch_test64(
            ResultCondition::NonZero,
            PROPERTY_BASE_GPR,
            NUMBER_TAG_GPR,
        ));
        // branchIfNotCell part 2: the transitional cell tag is the low byte ==
        // CELL_TAG (0x20). Mask the low byte, compare. (After the number test, the
        // only non-cell low-byte-0x20 value would be a number — already excluded —
        // so this exactly classifies cells, unlike a low-byte-only test.)
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(VALUE_TAG_MASK as i32), PROPERTY_TMP_GPR);
        self.h
            .masm_mut()
            .and64(PROPERTY_BASE_GPR, PROPERTY_TMP_GPR, PROPERTY_TMP_GPR);
        miss.push(self.h.masm_mut().branch32_imm(
            RelationalCondition::NotEqual,
            PROPERTY_TMP_GPR,
            TrustedImm32::new(CELL_TAG as i32),
        ));
        // UNBOX: cellPtr = boxed >> 8 (the raw, pinned arena CoreObjectCell*).
        self.h
            .masm_mut()
            .urshift64_imm(PROPERTY_BASE_GPR, TrustedImm32::new(8), PROPERTY_CELL_GPR);
        // receiver structure id := [cell + 0] (load32; the StructureID is a u32).
        self.h.masm_mut().load32(
            Address::new(PROPERTY_CELL_GPR, CELL_STRUCTURE_ID_OFFSET),
            PROPERTY_STRUCTURE_ID_GPR,
        );
        // cached structure id := [record + 0]. The record store base is stable (the
        // `Box` is never reallocated), so its address + record_index*16 is baked as
        // an absolute immediate, the ARM64 analog of x86-64's `[r13 + disp]` (this
        // engine has no r13 jitDataRegister seeded; the install path pins the store).
        self.h
            .masm_mut()
            .move_imm64(TrustedImm64::new(record_addr as i64), PROPERTY_RECORD_GPR);
        self.h.masm_mut().load32(
            Address::new(PROPERTY_RECORD_GPR, 0),
            PROPERTY_CACHED_STRUCTURE_GPR,
        );
        // branch32(NotEqual, receiverStructID, cachedStructID) -> slow. Catches both
        // a real structure mismatch and the SENTINEL (cached == 0) unfilled record.
        miss.push(self.h.masm_mut().branch32(
            RelationalCondition::NotEqual,
            PROPERTY_STRUCTURE_ID_GPR,
            PROPERTY_CACHED_STRUCTURE_GPR,
        ));
        miss
    }

    /// op_get_by_id (`emit_op_get_by_id` + the baseline DataIC, JITPropertyAccess.cpp
    /// / JITInlineCacheGenerator.cpp) — the named-property READ. The structure guard
    /// (above) routes a HIT to a cheap cached-offset far-call
    /// (`operation_get_by_id_with_cached_offset`: the own-data load at the cached
    /// offset) and a MISS to the optimize far-call (`operation_get_by_id_optimize`:
    /// re-resolve + FILL the record). Both far-calls take `(vm, boxed base,
    /// key_index, record_index, bytecode_index)`, probe the `m_exception` mirror (D3),
    /// store the boxed result to `dst`, and converge at a shared tail.
    ///
    /// DIVERGENCE from JSC (Increment 1, the inline-load follow-up, mirrors the
    /// `emit_get_by_val` decision): JSC's DataIC HIT path emits the inline
    /// `loadProperty` machine code (offset<64 inline vs negative-butterfly OOL,
    /// AssemblyHelpers.cpp:442-465). This engine's R4 cell carries a butterfly
    /// HANDLE (a slab index, object_store.rs:507-511), not a machine-addressable
    /// storage pointer, so the cached-offset LOAD is a leaf far-call until the
    /// Batch-5 object-storage model lands a raw butterfly pointer (Increment 2). The
    /// generated structure guard + record fill are unchanged across the increment.
    #[allow(clippy::too_many_arguments)]
    fn emit_get_by_id(
        &mut self,
        dst: i32,
        base: i32,
        record_addr: usize,
        record_index: u32,
        key_index: u32,
        bytecode_index: u32,
    ) {
        let miss = self.emit_property_ic_structure_guard(base, record_addr);

        // === HIT: cheap cached-offset own-data load. ==========================
        self.emit_get_by_id_call_args(base, key_index, record_index, bytecode_index);
        self.h.masm_mut().far_call(TrustedImm64::new(
            operation_get_by_id_with_cached_offset as usize as i64,
        ));
        // An own-data load cannot throw, but the shim falls back to the optimize
        // path on a SENTINEL/drift, which can; probe the mirror for that edge.
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst));
        let to_tail = self.h.masm_mut().jump();

        // === SLOW: full optimize (re-resolve + fill the record). ==============
        let slow_label = self.h.masm().label();
        self.link_fast_jumps_to(&miss, slow_label);
        self.emit_get_by_id_call_args(base, key_index, record_index, bytecode_index);
        self.h.masm_mut().far_call(TrustedImm64::new(
            operation_get_by_id_optimize as usize as i64,
        ));
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst));

        // === TAIL: both paths converge here (the HIT jump over the slow block). =
        let tail_label = self.h.masm().label();
        self.link_fast_jumps_to(&[to_tail], tail_label);
    }

    /// Load the `get_by_id` DataIC far-call arguments: `x0`=vm, `x1`=boxed base
    /// (re-read from the frame so it does not depend on the guard register's
    /// liveness), `x2`=key_index, `x3`=record_index, `x4`=bytecode_index — matching
    /// `operation_get_by_id_optimize`/`_with_cached_offset`'s C-ABI.
    fn emit_get_by_id_call_args(
        &mut self,
        base: i32,
        key_index: u32,
        record_index: u32,
        bytecode_index: u32,
    ) {
        self.h.masm_mut().load64(address_for(base), LEFT_GPR); // x1 = boxed base
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(key_index as i32), RIGHT_GPR); // x2 = key_index
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(record_index as i32), CALL_ARG_GPRS[0]); // x3 = record_index
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(bytecode_index as i32), CALL_ARG_GPRS[1]); // x4 = bci
        self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm
    }

    /// op_put_by_id (`emit_op_put_by_id` + the baseline DataIC) — the named-property
    /// WRITE. Same structure guard: a HIT routes to the cheap in-place replace store
    /// (`operation_put_by_id_with_cached_offset`, which applies the faithful write
    /// barrier inside the store — the `emitWriteBarrier(base)` analog,
    /// JITPropertyAccess.cpp:771); a MISS routes to the optimize far-call
    /// (`operation_put_by_id_optimize`: the faithful put, which handles a property-add
    /// TRANSITION via the slow path and NEVER caches it). put yields no observable
    /// value, so there is no `dst` store. Far-calls take `(vm, boxed base, boxed
    /// value, key_index, record_index, bytecode_index)`.
    ///
    /// DIVERGENCE (Increment 1): the cached store + its write barrier are a far-call
    /// into the host (which applies `apply_value_store_write_barrier`), so NO separate
    /// generated-code barrier is emitted yet — the inline store + inline barrier are
    /// Increment 2 (gated on the Batch-5 object-storage model), exactly the
    /// `emit_put_by_val` deferral.
    #[allow(clippy::too_many_arguments)]
    fn emit_put_by_id(
        &mut self,
        base: i32,
        value: i32,
        record_addr: usize,
        record_index: u32,
        key_index: u32,
        bytecode_index: u32,
    ) {
        let miss = self.emit_property_ic_structure_guard(base, record_addr);

        // === HIT: cheap in-place replace store (barriered inside the host). =====
        self.emit_put_by_id_call_args(base, value, key_index, record_index, bytecode_index);
        self.h.masm_mut().far_call(TrustedImm64::new(
            operation_put_by_id_with_cached_offset as usize as i64,
        ));
        self.emit_exception_probe();
        let to_tail = self.h.masm_mut().jump();

        // === SLOW: full optimize (replace-fill / transition -> slow path). ======
        let slow_label = self.h.masm().label();
        self.link_fast_jumps_to(&miss, slow_label);
        self.emit_put_by_id_call_args(base, value, key_index, record_index, bytecode_index);
        self.h.masm_mut().far_call(TrustedImm64::new(
            operation_put_by_id_optimize as usize as i64,
        ));
        self.emit_exception_probe();

        // === TAIL. ============================================================
        let tail_label = self.h.masm().label();
        self.link_fast_jumps_to(&[to_tail], tail_label);
    }

    /// Load the `put_by_id` DataIC far-call arguments: `x0`=vm, `x1`=boxed base,
    /// `x2`=boxed value, `x3`=key_index, `x4`=record_index, `x5`=bytecode_index —
    /// matching `operation_put_by_id_optimize`/`_with_cached_offset`'s C-ABI.
    fn emit_put_by_id_call_args(
        &mut self,
        base: i32,
        value: i32,
        key_index: u32,
        record_index: u32,
        bytecode_index: u32,
    ) {
        self.h.masm_mut().load64(address_for(base), LEFT_GPR); // x1 = boxed base
        self.h.masm_mut().load64(address_for(value), RIGHT_GPR); // x2 = boxed value
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(key_index as i32), CALL_ARG_GPRS[0]); // x3 = key_index
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(record_index as i32), CALL_ARG_GPRS[1]); // x4 = record_index
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(bytecode_index as i32), CALL_ARG_GPRS[2]); // x5 = bci
        self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm
    }

    /// op_call (`JIT::compileOpCall` / `emit_op_call`, jit/JITCall.cpp) — the
    /// B5-FIRST-CUT UNLINKED VIRTUAL CALL. The boxed callee + each boxed argument live
    /// in the JIT caller's frame at the op_call operands' VirtualRegister slots; this
    /// reads them cfr-relative (in operand order) into the C-ABI argument registers
    /// and far-calls the `operation_call` slow path, which runs the callee through the
    /// faithful generic dispatch (`Vm::operation_call` -> `execute_function_value`).
    /// Probes the `m_exception` mirror (D3) for the throw edge, then stores the boxed
    /// result to `dst`. Falls through to the next bytecode.
    ///
    /// `args` are the explicit-argument slots in op_call operand order (operands
    /// `3..3+argc`, EXCLUDING `this` — op_call's implicit receiver is `undefined`,
    /// supplied by `operation_call`). The register ABI is `x0`=vm, `x1`=callee,
    /// `x2`=argc, `x3..x7`=arg0..arg4 (`CALL_ARG_GPRS`); the caller has already
    /// verified `args.len() <= MAX_REGISTER_CALL_ARGS`.
    ///
    /// DIVERGENCE from JSC (B5-first-cut, the native-call follow-up): JSC sets up the
    /// callee `CallFrame` on the stack (storing callee/argc into its header, the args
    /// already laid out by the caller) and emits a LINKED `bl` to the callee — every
    /// site starts as an unlinked `virtualThunk` far-call and is PATCHED to a direct
    /// `bl` on first execution (`linkFor`). This first cut emits only the unlinked
    /// far-call to `operation_call` (which runs the callee INTERPRETED), passing the
    /// already-boxed arguments by value in registers rather than building a stack
    /// callee frame; the native direct-link / arity stub / bl-chain is the deferred
    /// B5-full perf follow-up.
    fn emit_op_call(&mut self, dst: i32, callee: i32, args: &[i32]) {
        self.emit_slow_call_body(callee, args, ThisSource::Undefined);
        // D3 throw edge: the callee threw (or a non-callable callee's TypeError)
        // stamps the mirror; branch to the shared exception stub BEFORE storing the
        // empty result. The probe reuses SCRATCH_GPR; x0 (the boxed result) is intact.
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst)); // dst = x0 (boxed result)
    }

    /// The slow-call register setup + far-call, WITHOUT the trailing exception probe /
    /// result store (so [`Self::emit_op_call_dynamic`] can SHARE the probe+store with
    /// the native fast path after the two converge). Leaves the boxed result in `x0`
    /// (`RESULT_GPR`).
    ///
    /// `this_source` selects the slow shim and register layout:
    /// - `Undefined` (op_call): `operation_call`, `x0`=vm, `x1`=callee, `x2`=argc,
    ///   `x3..x7`=arg0..arg4 (`CALL_ARG_GPRS`); `this`=undefined is supplied by the shim.
    /// - `Receiver(slot)` (op_call_with_this): `operation_call_with_this`, `x0`=vm,
    ///   `x1`=callee, `x2`=this, `x3`=argc, `x4..x7`=arg0..arg3 (`CALL_WITH_THIS_ARG_GPRS`)
    ///   — one fewer arg register because `this` occupies `x2`.
    ///
    /// The caller has verified `args.len()` is within the active form's arity bound.
    fn emit_slow_call_body(&mut self, callee: i32, args: &[i32], this_source: ThisSource) {
        match this_source {
            ThisSource::Undefined => {
                debug_assert!(
                    args.len() <= MAX_REGISTER_CALL_ARGS,
                    "op_call arity must be gated to MAX_REGISTER_CALL_ARGS before lowering"
                );
                // Load each boxed argument cfr-relative into its argument register (x3..).
                // Done BEFORE materializing x0/x1/x2 so no argument source is clobbered (the
                // sources are all cfr-relative memory; the destinations x3..x7 are distinct
                // from the callee/argc/vm registers x1/x2/x0).
                for (arg_index, &arg_slot) in args.iter().enumerate() {
                    self.h
                        .masm_mut()
                        .load64(address_for(arg_slot), CALL_ARG_GPRS[arg_index]);
                }
                self.h.masm_mut().load64(address_for(callee), LEFT_GPR); // x1 = callee (boxed)
                self.h
                    .masm_mut()
                    .move_imm32(TrustedImm32::new(args.len() as i32), RIGHT_GPR); // x2 = argc
                self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm (arg0)
                self.h
                    .masm_mut()
                    .far_call(TrustedImm64::new(operation_call as usize as i64));
            }
            ThisSource::Receiver(this_slot) => {
                debug_assert!(
                    args.len() <= MAX_REGISTER_CALL_WITH_THIS_ARGS,
                    "op_call_with_this arity must be gated to MAX_REGISTER_CALL_WITH_THIS_ARGS \
                     before lowering"
                );
                // Args -> x4..x7, then callee/this/argc/vm. All sources are cfr-relative
                // memory (`this`, args, callee) or the pinned-VM register (x19); the
                // destinations x0..x7 are disjoint from every source, so the order is free.
                for (arg_index, &arg_slot) in args.iter().enumerate() {
                    self.h
                        .masm_mut()
                        .load64(address_for(arg_slot), CALL_WITH_THIS_ARG_GPRS[arg_index]);
                }
                self.h.masm_mut().load64(address_for(callee), LEFT_GPR); // x1 = callee (boxed)
                self.h
                    .masm_mut()
                    .load64(address_for(this_slot), CALL_WITH_THIS_THIS_GPR); // x2 = this (boxed)
                self.h.masm_mut().move_imm32(
                    TrustedImm32::new(args.len() as i32),
                    CALL_WITH_THIS_ARGC_GPR,
                ); // x3 = argc
                self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm (arg0)
                self.h
                    .masm_mut()
                    .far_call(TrustedImm64::new(operation_call_with_this as usize as i64));
            }
        }
    }

    /// op_call — the A1.2 native JIT->JIT FAST PATH (the R-lever existence proof):
    /// a LINKED call whose callee is another baseline-JIT'd image, reached by a
    /// native `bl`/`blr` on the UNIFIED native stack instead of the `operation_call`
    /// interpreter re-entry. This is JSC's faithful `compileOpCall`/`compileSetupFrame`
    /// + `CallLinkInfo::emitFastPathImpl` (`jit/JITCall.cpp:222-251,288-297`;
    /// `bytecode/CallLinkInfo.cpp:323-365`): the CALLER sets up the callee `CallFrame`
    /// on the stack (callee/argCount/this/args at the calleeFrame slots), leaves
    /// `sp = calleeFrame + sizeof(CallerFrameAndPC)`, then `call`s the resolved entry;
    /// the callee's own `emitFunctionPrologue` (`pushPair(fp,lr); mov fp,sp`) finishes
    /// the header (writing `callerFrame`@0 = the caller's `fp` and `returnPC`@1 = the
    /// return address) and `op_ret`'s epilogue restores `fp`/`sp` and returns the
    /// boxed value in `returnValueGPR`.
    ///
    /// Frame placement (faithful to `addPtr(registerOffset*8 + sizeof(CallerFrameAndPC),
    /// cfr, sp)`, JITCall.cpp:141): the callee frame is placed at
    /// `fp - reservedLocalsBytes - CALLEE_SAVE_SPILL_BYTES - aboveFpBytes`, i.e.
    /// CONTIGUOUSLY below the caller's whole used region (locals + callee-save
    /// spills), where `aboveFpBytes` covers the callee header + `this` + args
    /// (16-aligned for `stackAlignmentRegisters == 2`). The callee's own locals then
    /// grow further DOWN from `calleeFrame` (its prologue's `sub sp`), so the two
    /// frames share ONE descending span — the Option-A unified-stack model
    /// (docs/design/jsstack.md "B5/STACK MODEL").
    ///
    /// CELL-CARRYING CALLS ARE NOW ALLOWED (A1.5 landed). A JIT CallFrame on the native
    /// stack holding a CELL (object/string) across a collection IS rooted: the scoped
    /// conservative scan of the native-stack JIT-frame span (`active_jit_frame_spans`
    /// pushed around `run_installed_baseline_jit`, scanned in
    /// `poll_collection_at_safepoint`) admits the cell. So the native fast path is no
    /// longer gated to cell-free arith — [`Self::emit_op_call_dynamic`] routes REAL
    /// op_calls here once the callee is resolved to an installed baseline image.
    ///
    /// DIVERGENCE from JSC (first cut): the [`LinkedCallTarget`] pre-seed variant bakes
    /// an ABSOLUTE `blr` immediate at emit time (the A1.3 proof); the live
    /// [`Self::emit_op_call_dynamic`] variant RE-RESOLVES the callee entry into a
    /// register every call (the unlinked->linked `CallLinkInfo` analog) and `blr`s the
    /// register. Neither yet caches a patchable `CallLinkInfo::m_monomorphicCallDestination`
    /// (the data-IC link-on-first-exec + callee-identity guard + arity stub are the
    /// follow-up). The callee-frame `codeBlock`@2 slot is left unwritten (the callee's
    /// emitted image never consults it; it is needed only when a callee slow path
    /// re-enters the interpreter and reads the CURRENT frame's CodeBlock — a follow-up).
    fn emit_op_call_native_linked(
        &mut self,
        dst: i32,
        callee: i32,
        args: &[i32],
        entry: usize,
        bytecode_index: usize,
    ) {
        // The A1.3 pre-seeded LINKED proof is plain op_call only: `this`=undefined.
        self.emit_native_call_frame_setup_and_call(
            callee,
            args,
            bytecode_index,
            ThisSource::Undefined,
            NativeCallTarget::Absolute(entry),
        );
        // D3 throw edge (a callee slow path stamped the mirror), then store x0 -> dst.
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst));
    }

    /// op_call — the A1.x BROAD-ENGAGEMENT native fast path (the R-lever): RE-RESOLVE
    /// the callee's installed baseline entry per call and, if found, take the native
    /// `blr` fast path; else the `operation_call` slow path. This is JSC's
    /// unlinked->linked `CallLinkInfo` resolution (`jit/Repatch.cpp` `linkFor`)
    /// collapsed to a re-resolve (the monomorphic CACHE is the follow-up).
    ///
    /// Emitted shape (one op_call site):
    /// ```text
    ///   ; RESOLVE: operation_resolve_baseline_native_entry(vm, callee, argc) -> x0
    ///   load   x1, [cfr, callee]      ; boxed callee value
    ///   mov    x2, #argc              ; argument count (excl. this)
    ///   mov    x0, x19                ; pinned *mut Vm
    ///   blr    resolver               ; x0 = entry | 0
    ///   mov    x6, x0                 ; keep entry across frame setup (NATIVE_ENTRY_GPR)
    ///   cbz    x6, .Lslow             ; not native-callable -> slow path
    ///   ; NATIVE FAST PATH (build callee frame, blr x6) — result in x0
    ///   b      .Ltail
    /// .Lslow:
    ///   ; SLOW PATH (operation_call) — result in x0
    /// .Ltail:
    ///   <exception probe> ; store x0 -> dst   ; SHARED by both paths
    /// ```
    /// The resolver far-call clobbers only caller-saved registers; `cfr` (x29),
    /// the pinned-VM pair (x19/x20) and the tag pair (x27/x28) are callee-saved and
    /// survive it, so the native frame setup and the slow path both run unchanged after.
    fn emit_op_call_dynamic(
        &mut self,
        dst: i32,
        callee: i32,
        args: &[i32],
        this_source: ThisSource,
        bytecode_index: usize,
    ) {
        debug_assert!(
            args.len()
                <= match this_source {
                    ThisSource::Undefined => MAX_REGISTER_CALL_ARGS,
                    ThisSource::Receiver(_) => MAX_REGISTER_CALL_WITH_THIS_ARGS,
                },
            "call arity must be gated to the form's register bound before lowering"
        );
        // === RESOLVE: operation_resolve_baseline_native_entry(vm, callee, argc) ======
        // The resolver matches the callee's installed entry by callee VALUE + explicit
        // arg count (EXCLUDING `this`); it is `this`-agnostic, so both call forms share
        // this step unchanged.
        self.h.masm_mut().load64(address_for(callee), LEFT_GPR); // x1 = callee (boxed)
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(args.len() as i32), RIGHT_GPR); // x2 = argc
        self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // x0 = vm (arg0)
        self.h.masm_mut().far_call(TrustedImm64::new(
            operation_resolve_baseline_native_entry as usize as i64,
        ));
        // Preserve the resolved entry (x0) in NATIVE_ENTRY_GPR (x6) across the frame
        // setup, which clobbers x0. `cbz`-equivalent: branch to the slow path on a
        // zero (not-native-callable) result.
        self.h.masm_mut().move_rr(RESULT_GPR, NATIVE_ENTRY_GPR);
        let to_slow = self.h.masm_mut().branch_test64(
            ResultCondition::Zero,
            NATIVE_ENTRY_GPR,
            NATIVE_ENTRY_GPR,
        );

        // === NATIVE FAST PATH: build the callee frame and `blr` the resolved entry. ==
        self.emit_native_call_frame_setup_and_call(
            callee,
            args,
            bytecode_index,
            this_source,
            NativeCallTarget::Register(NATIVE_ENTRY_GPR),
        );
        // Jump OVER the slow path to the shared probe+store tail.
        let to_tail = self.h.masm_mut().jump();

        // === SLOW PATH: operation_call{,_with_this} (the callee runs interpreted /
        //     re-resolves its own sub-calls). Reached when the callee is not an
        //     installed baseline image at this arity. =============================
        let slow_label = self.h.masm().label();
        self.link_fast_jumps_to(&[to_slow], slow_label);
        self.emit_slow_call_body(callee, args, this_source);

        // === SHARED TAIL: D3 throw probe + store the boxed result. Both paths leave
        //     the result in x0 and converge here. =================================
        let tail_label = self.h.masm().label();
        self.link_fast_jumps_to(&[to_tail], tail_label);
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst));
    }

    /// The native op_call fast-path FRAME SETUP + terminal call (`compileSetupFrame` +
    /// `CallLinkInfo::emitFastPathImpl`), WITHOUT the trailing exception probe / result
    /// store (so the static [`Self::emit_op_call_native_linked`] and the dynamic
    /// [`Self::emit_op_call_dynamic`] share the probe+store). Leaves the boxed result
    /// in `x0` (`RESULT_GPR`). `target` selects the terminal call: an absolute baked
    /// entry (`far_call`) or a runtime-resolved register (`blr reg`). `this_source`
    /// selects what lands in the callee frame's `thisArgument` slot (the only delta
    /// between op_call and op_call_with_this).
    fn emit_native_call_frame_setup_and_call(
        &mut self,
        callee: i32,
        args: &[i32],
        bytecode_index: usize,
        this_source: ThisSource,
        target: NativeCallTarget,
    ) {
        let argc = args.len() as i32;
        // argumentCountIncludingThis (CallFrame.h:179): the explicit args plus `this`.
        let count_including_this = argc + 1;
        // The header + `this` + args sit AT/ABOVE the callee `fp`; the highest slot
        // is `FIRST_ARGUMENT + argc - 1` (== arg(argc-1)). Round the region to 16 so
        // `sp = calleeFrame + 16` is 16-aligned (AAPCS64 / stackAlignmentRegisters).
        let above_fp_bytes =
            round_up_to_16((CallFrameSlot::FIRST_ARGUMENT + argc) * REGISTER_SIZE_BYTES);
        // calleeFrame = fp + offset (offset < 0): below the caller's reserved locals
        // and callee-save spills, with room above for the callee header + args.
        let callee_frame_offset_from_fp =
            -(self.reserved_locals_bytes + CALLEE_SAVE_SPILL_BYTES + above_fp_bytes);

        // calleeFrame address -> SCRATCH_GPR_B (held until `sp` is lowered).
        self.h.masm_mut().add64_imm(
            TrustedImm32::new(callee_frame_offset_from_fp),
            CALL_FRAME_GPR,
            SCRATCH_GPR_B,
        );
        // Initialize Callee (CallFrame.h:178). The boxed callee value is laid into
        // the header for faithfulness/StackVisitor; dispatch uses the resolved entry.
        self.h.masm_mut().load64(address_for(callee), SCRATCH_GPR);
        self.store_callee_frame_slot(CallFrameSlot::CALLEE, SCRATCH_GPR);
        // Initialize ArgumentCount (CallFrame.h:179): payload == count incl. `this`.
        // (JSC also stamps the CallSiteIndex into the tag half, JITCall.cpp:247-248;
        // omitted in this first cut — the callee image does not read it.)
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(count_including_this), SCRATCH_GPR);
        self.store_callee_frame_slot(CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS, SCRATCH_GPR);
        // Initialize `this` (CallFrame.h:180 `thisArgument`; JSC `compileSetupFrame`
        // stores the bytecode `thisValue` here, JITCall.cpp:117-119). op_call's implicit
        // receiver is `undefined` (the same value `operation_call` supplies on the slow
        // path); op_call_with_this stores the explicit RECEIVER operand read cfr-relative
        // from the caller frame (`address_for(this_slot)`, which lies ABOVE the callee
        // frame being built, so the store never clobbers it).
        match this_source {
            ThisSource::Undefined => {
                let undefined_bits = JsValue::undefined().encoded().0;
                self.h
                    .masm_mut()
                    .move_imm64(TrustedImm64::new(undefined_bits as i64), SCRATCH_GPR);
            }
            ThisSource::Receiver(this_slot) => {
                self.h
                    .masm_mut()
                    .load64(address_for(this_slot), SCRATCH_GPR);
            }
        }
        self.store_callee_frame_slot(CallFrameSlot::THIS_ARGUMENT, SCRATCH_GPR);
        // Copy the explicit arguments into the callee frame's argument slots
        // (firstArgument + i), the analog of the caller's pre-laid-out arg area.
        for (arg_index, &arg_slot) in args.iter().enumerate() {
            self.h.masm_mut().load64(address_for(arg_slot), SCRATCH_GPR);
            self.store_callee_frame_slot(
                CallFrameSlot::FIRST_ARGUMENT + arg_index as i32,
                SCRATCH_GPR,
            );
        }

        // Hand the pinned `*mut Vm` to the callee in x0 (its prologue moves x0 into
        // the pinned-VM register x19, this engine's vm convention) BEFORE switching
        // `sp`. x0 is the result register, clobbered by the call anyway.
        self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR);
        // sp = calleeFrame + sizeof(CallerFrameAndPC) (JITCall.cpp:246). The callee
        // prologue's `pushPair(fp,lr)` lands [callerFrame, returnPC] at slots 0/1.
        self.h.masm_mut().add64_imm(
            TrustedImm32::new(CALLER_FRAME_AND_PC_SIZE_BYTES),
            SCRATCH_GPR_B,
            RegisterID::Sp,
        );
        // The linked near call (`jit.call(callTargetGPR, ...)`, CallLinkInfo.cpp:363):
        // an ABSOLUTE `blr` to a baked entry (the pre-seeded proof) or a `blr reg` to
        // the runtime-resolved entry (broad engagement).
        match target {
            NativeCallTarget::Absolute(entry) => {
                self.h.masm_mut().far_call(TrustedImm64::new(entry as i64));
            }
            NativeCallTarget::Register(reg) => {
                self.h.masm_mut().call_register(reg);
            }
        }
        // returnPC == the address of the instruction AFTER the blr.
        let return_pc_offset = self.h.masm().label().label().0 as usize;
        self.linked_call_sites.push(LinkedCallSite {
            bytecode_index,
            return_pc_offset,
            callee_frame_offset_from_fp,
        });

        // resetSP (JITCall.cpp:294): the callee restored `fp`; restore `sp` to the
        // caller's post-prologue position (`fp - reserved - spills`) so the epilogue's
        // `popPair`s refill the right callee-save spill slots.
        self.h.masm_mut().add64_imm(
            TrustedImm32::new(-(self.reserved_locals_bytes + CALLEE_SAVE_SPILL_BYTES)),
            CALL_FRAME_GPR,
            RegisterID::Sp,
        );
    }

    /// Store `src` into the in-construction callee frame's slot `slot_index`
    /// (`Address(calleeFrame, slot*sizeof(Register))`, calleeFrame held in
    /// SCRATCH_GPR_B). Faithful to JSC's `calleeFrameSlot(slot)` stores
    /// (JITCall.cpp:251).
    fn store_callee_frame_slot(&mut self, slot_index: i32, src: RegisterID) {
        self.h.masm_mut().store64(
            src,
            Address::new(SCRATCH_GPR_B, slot_index.wrapping_mul(REGISTER_SIZE_BYTES)),
        );
    }

    /// The FUSED int32 compare-and-branch (`emit_op_jless` via `emit_compareAndJump`,
    /// JITArithmetic.cpp:215-227): load lhs/rhs, guard both int32 -> slow, then
    /// `branch32(cond, lhs, rhs)` straight to the BYTECODE-INDEX target. `cond` is the
    /// INVERTED relational condition because the bytecode is `relational ; JumpIfFalse`
    /// (== JSC `op_jnless`): the branch is taken when the comparison is FALSE.
    fn emit_compare_and_jump(
        &mut self,
        op: CoreOpcode,
        lhs: i32,
        rhs: i32,
        target_bci: usize,
        resume_bci: usize,
    ) {
        self.h.masm_mut().load64(address_for(lhs), LEFT_GPR);
        self.h.masm_mut().load64(address_for(rhs), RIGHT_GPR);
        let mut fast_jumps = Vec::with_capacity(2);
        fast_jumps.push(self.h.branch_if_not_int32(LEFT_GPR, MODE));
        fast_jumps.push(self.h.branch_if_not_int32(RIGHT_GPR, MODE));
        // branch32(invert(cmpCond), lhs, rhs) -> target: taken iff the comparison is
        // false (JumpIfFalse). Compared 32-bit signed; the int32 guards above prove
        // the low 32 bits ARE the signed int32 values.
        let branch = self
            .h
            .masm_mut()
            .branch32(inverted_relational(op), LEFT_GPR, RIGHT_GPR);
        self.jumps.push((branch, target_bci));
        self.slow.push(SlowCase {
            fast_jumps,
            resume_bci,
            kind: SlowKind::CompareBranch {
                shim: compare_shim(op),
                target_bci,
            },
        });
    }

    /// op_jmp (`emit_op_jmp`): an unconditional `b` to a bytecode-index target.
    fn emit_op_jmp(&mut self, target_bci: usize) {
        let branch = self.h.masm_mut().jump();
        self.jumps.push((branch, target_bci));
    }

    /// op_jfalse (`emit_op_jfalse`): the standalone boolean-branch (NOT a fused
    /// relational). Guard the operand is a boolean -> slow (ToBoolean); then test
    /// bit 0 with the `Imm(1)` mask: a JS boolean is `ValueFalse`(0x6)/`ValueTrue`(0x7),
    /// so bit 0 is the truth value -> `branchTest32(Zero, value, 1)` jumps when FALSE.
    /// DEFERRED (documented): the inline int32 truthiness fast path (a non-boolean
    /// int32 operand takes the faithful `operation_jfalse` slow path instead).
    fn emit_op_jfalse(&mut self, cond: i32, target_bci: usize, resume_bci: usize) {
        self.h.masm_mut().load64(address_for(cond), LEFT_GPR);
        // addSlowCase(branchIfNotBoolean(value)).
        let not_boolean = self
            .h
            .branch_if_not_boolean(LEFT_GPR, SCRATCH_GPR, SCRATCH_GPR_B);
        // branchTest32(Zero, value, Imm(1)) -> target (false has bit 0 clear).
        self.h
            .masm_mut()
            .move_imm32(TrustedImm32::new(1), BOOL_MASK_GPR);
        let branch =
            self.h
                .masm_mut()
                .branch_test32(ResultCondition::Zero, LEFT_GPR, BOOL_MASK_GPR);
        self.jumps.push((branch, target_bci));
        self.slow.push(SlowCase {
            fast_jumps: vec![not_boolean],
            resume_bci,
            kind: SlowKind::TruthyBranch {
                shim: operation_jfalse as usize,
                target_bci,
            },
        });
    }

    /// op_ret (`emit_op_ret`): `load64(value) -> returnValueGPR ; b done`.
    fn emit_op_ret(&mut self, value: i32) {
        self.h.masm_mut().load64(address_for(value), RESULT_GPR);
        let to_done = self.h.masm_mut().jump();
        self.done_jumps.push(to_done);
    }

    /// The shared epilogue (`done`): restore the callee-saved spills, then the
    /// faithful `AssemblyHelpers::emitFunctionEpilogue` (`mov sp,fp; ldp fp,lr;
    /// ret`). x0 (the return value / op_ret result) is preserved across the pops.
    ///
    /// `mov sp,fp` discards the reserved-locals region in ONE step (it is now
    /// load-bearing, not a no-op: the callee-saves were spilled BELOW the locals,
    /// so after refilling them `sp` sits at `fp - localsBytes`, and `mov sp,fp`
    /// skips the locals so `ldp fp,lr` reads slots 0/1 of the frame header). This
    /// matches JSC's `restoreCalleeSaves` + `emitFunctionEpilogue` and leaves `sp`
    /// back at `calleeFrame + sizeof(CallerFrameAndPC)` for the trampoline/caller.
    fn emit_epilogue(&mut self) -> Label {
        let done = self.h.masm().label();
        self.h
            .masm_mut()
            .pop_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // refill x27/x28
        self.h
            .masm_mut()
            .pop_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // refill x19/x20
        self.h.masm_mut().move_rr(CALL_FRAME_GPR, RegisterID::Sp); // mov sp, fp
        self.h.masm_mut().pop_pair(CALL_FRAME_GPR, LINK_GPR); // ldp fp,lr,[sp],#16
        self.h.masm_mut().ret();
        done
    }

    /// A1.4 STACK-OVERFLOW STUB (== JSC's `ThrowStackOverflowAtPrologue` thunk
    /// target, JIT.cpp:864): the prologue's `softStackLimit` guard branches here. At
    /// entry the frame is HALF-built — `pushPair(fp,lr)` + `mov fp,sp` ran and
    /// `sp == fp - localsBytes`, but the callee-save spills did NOT (the check fired
    /// before them), so this CANNOT route through the normal epilogue (which would
    /// `popPair` never-pushed spills). `x0` still holds the raw `*mut Vm`. Far-call
    /// the throw shim — which materializes a faithful stack-overflow `RangeError` and
    /// STAMPS the `m_exception` mirror (D3) — then run the MINIMAL prologue teardown
    /// `mov sp,fp; ldp fp,lr; ret` (no callee-save refill) to return cleanly to the
    /// trampoline/caller, which branches on the stamped mirror (the throw edge).
    fn emit_stack_overflow_stub(&mut self) -> Label {
        let stub = self.h.masm().label();
        // operation_throw_stack_overflow(vm): x0 is already the raw `*mut Vm` (the
        // prologue has not yet moved it into the pinned-VM register). The far-call
        // clobbers lr; it is refilled below from the [fp,lr] header pushPair wrote.
        self.h.masm_mut().far_call(TrustedImm64::new(
            operation_throw_stack_overflow as usize as i64,
        ));
        // Minimal teardown (callee-saves were NOT spilled): `mov sp,fp` discards the
        // locals reservation, `ldp fp,lr,[sp],#16` refills the frame-header pair,
        // `ret` returns. x0 holds the shim's `JSValue::empty()` bits (the throw edge's
        // discarded result; the caller reads the stamped mirror instead).
        self.h.masm_mut().move_rr(CALL_FRAME_GPR, RegisterID::Sp); // mov sp, fp
        self.h.masm_mut().pop_pair(CALL_FRAME_GPR, LINK_GPR); // ldp fp,lr,[sp],#16
        self.h.masm_mut().ret();
        stub
    }

    /// The post-far-call pending-exception probe (D3), shared by the throwing slow
    /// kinds: `branchTestPtr(NonZero, AbsoluteAddress(&vm.m_exception))` -> the shared
    /// exception stub. Materialize the baked address, load the word, test it.
    fn emit_exception_probe(&mut self) {
        self.h.masm_mut().move_imm64(
            TrustedImm64::new(self.jit_pending_address as i64),
            SCRATCH_GPR,
        );
        self.h
            .masm_mut()
            .load64(Address::new(SCRATCH_GPR, 0), SCRATCH_GPR);
        let to_exception =
            self.h
                .masm_mut()
                .branch_test64(ResultCondition::NonZero, SCRATCH_GPR, SCRATCH_GPR);
        self.exception_jumps.push(to_exception);
    }

    /// SLOW pass (`privateCompileSlowCases`): emit every deferred slow block after
    /// the epilogue. Each links its fast guards to its slow label, emits the bridge
    /// call, and resolves back to the bytecode-index resume/target labels.
    fn emit_slow_cases(&mut self) {
        // Drain so the borrow of `self.slow` does not conflict with `&mut self`.
        let slow_cases = std::mem::take(&mut self.slow);
        for case in slow_cases {
            let slow_label = self.h.masm().label();
            // The fast guards target the slow LABEL (a code offset), not a bytecode
            // index, so record them in the dedicated label-link list.
            self.link_fast_jumps_to(&case.fast_jumps, slow_label);

            // arg0 := the pinned `*mut Vm` (jit-runtime-bridge.md D2c).
            self.h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // mov x0, x19
            match case.kind {
                SlowKind::BinaryOp { shim, dst } => {
                    // left (x1)/right (x2) are STILL the original operands.
                    self.h.masm_mut().far_call(TrustedImm64::new(shim as i64));
                    self.emit_exception_probe();
                    // emitPutVirtualRegister(result) then resume at the next bytecode.
                    self.h.masm_mut().store64(RESULT_GPR, address_for(dst));
                    let to_resume = self.h.masm_mut().jump();
                    self.jumps.push((to_resume, case.resume_bci));
                }
                SlowKind::CompareBranch { shim, target_bci } => {
                    self.h.masm_mut().far_call(TrustedImm64::new(shim as i64));
                    self.emit_exception_probe();
                    // x0 = comparison boolean (0/1). JumpIfFalse: branch to target
                    // when the comparison is FALSE (x0 == 0).
                    let to_target = self.h.masm_mut().branch_test32(
                        ResultCondition::Zero,
                        RESULT_GPR,
                        RESULT_GPR,
                    );
                    self.jumps.push((to_target, target_bci));
                    let to_resume = self.h.masm_mut().jump();
                    self.jumps.push((to_resume, case.resume_bci));
                }
                SlowKind::TruthyBranch { shim, target_bci } => {
                    // value still in x1 (arg1). operation_jfalse is INFALLIBLE -> no
                    // exception probe. x0 = !truthy as 0/1 (1 = take the branch).
                    self.h.masm_mut().far_call(TrustedImm64::new(shim as i64));
                    let to_target = self.h.masm_mut().branch_test32(
                        ResultCondition::NonZero,
                        RESULT_GPR,
                        RESULT_GPR,
                    );
                    self.jumps.push((to_target, target_bci));
                    let to_resume = self.h.masm_mut().jump();
                    self.jumps.push((to_resume, case.resume_bci));
                }
            }
        }
    }

    /// Link a slow block's fast-path guard jumps to its slow label, appending the
    /// resolved records directly (these target a code LABEL, not a bytecode index).
    fn link_fast_jumps_to(&mut self, guards: &[Jump], slow_label: Label) {
        for guard in guards {
            self.label_link_records
                .push(guard.to_link_record(slow_label));
        }
    }
}

/// Map a relational `CoreOpcode` to the INVERTED branch condition for the fused
/// `relational ; JumpIfFalse` pair (the branch is taken when the comparison is
/// false): `<` -> `>=`, `<=` -> `>`, `>` -> `<=`.
fn inverted_relational(op: CoreOpcode) -> RelationalCondition {
    match op {
        CoreOpcode::LessThanInt32 => RelationalCondition::GreaterThanOrEqual,
        CoreOpcode::LessEqualInt32 => RelationalCondition::GreaterThan,
        CoreOpcode::GreaterThanInt32 => RelationalCondition::LessThanOrEqual,
        _ => unreachable!("inverted_relational only handles the fused relational set"),
    }
}

/// The `operationCompare*` shim address a fused compare-and-branch slow path
/// far-calls (the NON-inverted relational evaluator; the slow path then tests its
/// boolean for the JumpIfFalse direction).
fn compare_shim(op: CoreOpcode) -> usize {
    match op {
        CoreOpcode::LessThanInt32 => operation_compare_less as usize,
        CoreOpcode::LessEqualInt32 => operation_compare_lesseq as usize,
        CoreOpcode::GreaterThanInt32 => operation_compare_greater as usize,
        _ => unreachable!("compare_shim only handles the fused relational set"),
    }
}

/// The Stage-1 fused relational set (== JSC `op_jless`/`op_jlesseq`/`op_jgreater`).
fn is_fusible_relational(op: CoreOpcode) -> bool {
    matches!(
        op,
        CoreOpcode::LessThanInt32 | CoreOpcode::LessEqualInt32 | CoreOpcode::GreaterThanInt32
    )
}

/// Map a binary arith `CoreOpcode` to its [`ArithFamilyOp`], or `None` if it is
/// not a Stage-1 arith op. `AddInt32`/`SubInt32`/`MulInt32` (== JSC op_add/sub/mul)
/// carry the int32+double fast path; `DivNumber` (== op_div) the double-only path;
/// the bitwise/shift ops stay int32-only.
fn arith_family_of(op: CoreOpcode) -> Option<ArithFamilyOp> {
    Some(match op {
        CoreOpcode::AddInt32 => ArithFamilyOp::Add,
        CoreOpcode::SubInt32 => ArithFamilyOp::Sub,
        CoreOpcode::MulInt32 => ArithFamilyOp::Mul,
        CoreOpcode::DivNumber => ArithFamilyOp::Div,
        CoreOpcode::BitAndInt32 => ArithFamilyOp::BitAnd,
        CoreOpcode::BitOrInt32 => ArithFamilyOp::BitOr,
        CoreOpcode::BitXorInt32 => ArithFamilyOp::BitXor,
        CoreOpcode::LeftShiftInt32 => ArithFamilyOp::LShift,
        CoreOpcode::RightShiftInt32 => ArithFamilyOp::RShift,
        _ => return None,
    })
}

/// A register operand resolved to its frame slot (the raw VirtualRegister value),
/// rejecting constant-pool operands (whose load lowering is deferred).
fn frame_slot(register: crate::bytecode::VirtualRegister) -> Result<i32, EmitFunctionError> {
    if register.is_constant() {
        return Err(EmitFunctionError::ConstantOperand);
    }
    Ok(register.raw())
}

/// Decode the `CoreOpcode` at a bytecode index (ordinal), or an error.
fn core_opcode_at(code_block: &CodeBlock, bci: usize) -> Result<CoreOpcode, EmitFunctionError> {
    let decoded = code_block
        .unlinked()
        .instructions()
        .decoded_at(BytecodeIndex::from_offset(bci as u32))?;
    CoreOpcode::from_opcode(decoded.opcode).ok_or(EmitFunctionError::UnknownOpcode)
}

/// True iff `register` appears as ANY register operand in instructions
/// `[from_bci, count)`.
///
/// LOAD-BEARING for the fusion deadness guard: the FUSED int32 compare-and-branch
/// folds the comparison's boolean INTO the branch and stores NOTHING to the
/// comparison's dst (`cmp_dst`). So fusing `relational(cmp_dst) ; JumpIfFalse(cmp_dst)`
/// is only sound when `cmp_dst` is DEAD afterward — otherwise a later read of it
/// would observe the stale op_enter `undefined` slot (a silent mis-compile vs the
/// interpreter, which DOES materialize the boolean). This proves deadness
/// conservatively: a later WRITE to the slot also counts as a use, so a CodeBlock
/// that merely re-defines the temp before reading it is DECLINED (stays in the
/// interpreter, the S4 gate's safe-by-rejection posture) rather than risking the
/// stale read. A precise read-only liveness analysis is a future refinement.
fn register_used_from(
    code_block: &CodeBlock,
    register: crate::bytecode::VirtualRegister,
    from_bci: usize,
    count: usize,
) -> Result<bool, EmitFunctionError> {
    let stream = code_block.unlinked().instructions();
    for bci in from_bci..count {
        let decoded = stream.decoded_at(BytecodeIndex::from_offset(bci as u32))?;
        for index in 0..decoded.operands.len() {
            if let Ok(operand_register) = decoded.register_operand(index) {
                if operand_register == register {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Count the `get_by_id`/`put_by_id` DataIC sites in bytecode order — the size of
/// the `HandlerPropertyInlineCacheRecord` store the install path allocates before
/// emit (`BaselineJITData` `propertyCacheSize`, CodeBlock.cpp:802). This MUST equal
/// the emitter's final `property_site_index` so every baked record index is in
/// bounds: both walk every instruction once (GetByName/PutByName are never fused
/// away — only relational+JumpIfFalse pairs fuse), counting the SAME opcodes.
pub(crate) fn count_property_ic_sites(code_block: &CodeBlock) -> Result<usize, EmitFunctionError> {
    let count = code_block.unlinked().instructions().instruction_count();
    let mut sites = 0usize;
    for bci in 0..count {
        if matches!(
            core_opcode_at(code_block, bci)?,
            CoreOpcode::GetByName | CoreOpcode::PutByName
        ) {
            sites += 1;
        }
    }
    Ok(sites)
}

/// Scan every register operand for the maximum negative-local index used, so
/// op_enter can zero-fill exactly the callee var slots (DIVERGENCE #1).
fn count_callee_locals(code_block: &CodeBlock) -> Result<u32, EmitFunctionError> {
    let stream = code_block.unlinked().instructions();
    let count = stream.instruction_count();
    let mut max_local: i64 = -1;
    for bci in 0..count {
        let decoded = stream.decoded_at(BytecodeIndex::from_offset(bci as u32))?;
        for index in 0..decoded.operands.len() {
            if let Ok(register) = decoded.register_operand(index) {
                if let Some(local_index) = register.to_local_index() {
                    max_local = max_local.max(local_index as i64);
                }
            }
        }
    }
    Ok((max_local + 1) as u32)
}

/// Lower a WHOLE function's bytecode (`code_block`) to ONE callable ARM64 image,
/// mirroring `JIT::privateCompile`'s 3 passes. Executes as
/// `extern "C" fn(vm: *mut Vm, cfr: u64) -> u64` (the raw return-seed C-ABI lane,
/// op_add's harness): arg0 = the `*mut Vm` for the operation slow paths, arg1 = the
/// frame base (`cfr`); the result is `op_ret`'s boxed value in `returnValueGPR`.
///
/// `jit_pending_address` is the baked `AbsoluteAddress` of `Vm::m_exception` (D3);
/// it must remain stable for the call's duration (a pin-stable `Vm`, op_add prereq #3).
/// `soft_stack_limit_address` is the baked `AbsoluteAddress` of the VM's soft stack
/// limit (`VM::addressOfSoftStackLimit`, A1.4) the prologue's overflow check loads;
/// same pin-stability requirement.
///
/// Returns `Err` (REJECTS the function) for any opcode/operand outside the Stage-1
/// int32/control-flow allowlist — the S4 gate behavior (the function stays in the
/// interpreter rather than being mis-lowered).
///
/// Each op_call is lowered by [`FunctionEmitter::emit_op_call_dynamic`]: a per-call
/// RE-RESOLVE of the callee's installed baseline entry that takes the native JIT->JIT
/// fast path when the callee is itself baseline-JIT'd (the R-lever) and falls to the
/// `operation_call` slow path otherwise. Cell-carrying calls are rooted by the A1.5
/// JIT-frame conservative scan (see [`FunctionEmitter::emit_op_call_native_linked`]).
pub(crate) fn emit_baseline_function(
    code_block: &CodeBlock,
    jit_pending_address: usize,
    soft_stack_limit_address: usize,
) -> Result<FunctionImage, EmitFunctionError> {
    emit_baseline_function_with_linked_calls(
        code_block,
        jit_pending_address,
        soft_stack_limit_address,
        &[],
    )
}

/// As [`emit_baseline_function`], but resolves the op_call sites named in
/// `linked_calls` to the A1.2 native JIT->JIT fast path (a direct `blr` to the
/// callee's installed-image entry) instead of the `operation_call` slow path. An
/// op_call NOT named here keeps the slow path. This is the A1.3 first-cut linker:
/// the proof harness pre-seeds ONE monomorphic target; the live path passes `&[]`.
pub(crate) fn emit_baseline_function_with_linked_calls(
    code_block: &CodeBlock,
    jit_pending_address: usize,
    soft_stack_limit_address: usize,
    linked_calls: &[LinkedCallTarget],
) -> Result<FunctionImage, EmitFunctionError> {
    let count = code_block.unlinked().instructions().instruction_count();
    if count == 0 {
        return Err(EmitFunctionError::EmptyFunction);
    }

    let num_locals = count_callee_locals(code_block)?;
    let mut emitter = FunctionEmitter::new(count, jit_pending_address, soft_stack_limit_address);

    // === PROLOGUE + op_enter ===============================================
    emitter.emit_prologue(num_locals);
    emitter.emit_op_enter(num_locals);

    // === MAIN pass (privateCompileMainPass) ================================
    let mut bci = 0;
    while bci < count {
        emitter.labels[bci] = Some(emitter.h.masm().label());
        let decoded = code_block
            .unlinked()
            .instructions()
            .decoded_at(BytecodeIndex::from_offset(bci as u32))?;
        let op = CoreOpcode::from_opcode(decoded.opcode).ok_or(EmitFunctionError::UnknownOpcode)?;

        // FUSION: a relational op immediately followed by `JumpIfFalse` reading its
        // result == JSC's fused `op_jless`/`op_jnless` (DIVERGENCE #2). The fused
        // branch materializes NO value for the comparison's dst, so it is only sound
        // when that dst is DEAD after the pair (see `register_used_from`); otherwise
        // we do NOT fuse and fall through to the relational reject arm so the S4 gate
        // DECLINES the CodeBlock (a later read of the boolean would otherwise observe
        // the stale op_enter `undefined` slot — a silent mis-compile).
        if is_fusible_relational(op) && bci + 1 < count {
            let next_op = core_opcode_at(code_block, bci + 1)?;
            let cmp_dst = decoded.register_operand(0)?;
            if next_op == CoreOpcode::JumpIfFalse {
                let next = code_block
                    .unlinked()
                    .instructions()
                    .decoded_at(BytecodeIndex::from_offset((bci + 1) as u32))?;
                let jfalse_cond = next.register_operand(0)?;
                if jfalse_cond == cmp_dst
                    && !register_used_from(code_block, cmp_dst, bci + 2, count)?
                {
                    let lhs = frame_slot(decoded.register_operand(1)?)?;
                    let rhs = frame_slot(decoded.register_operand(2)?)?;
                    let target_bci = next.bytecode_index_operand(1)?.offset() as usize;
                    emitter.emit_compare_and_jump(op, lhs, rhs, target_bci, bci + 2);
                    // The folded JumpIfFalse site: label it just after the fused
                    // branch so the labels-by-bci table stays fully populated.
                    emitter.labels[bci + 1] = Some(emitter.h.masm().label());
                    bci += 2;
                    continue;
                }
            }
        }

        match op {
            CoreOpcode::AddInt32
            | CoreOpcode::SubInt32
            | CoreOpcode::MulInt32
            | CoreOpcode::DivNumber
            | CoreOpcode::BitAndInt32
            | CoreOpcode::BitOrInt32
            | CoreOpcode::BitXorInt32
            | CoreOpcode::LeftShiftInt32
            | CoreOpcode::RightShiftInt32 => {
                let family = arith_family_of(op).expect("arm matches arith_family_of");
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let lhs = frame_slot(decoded.register_operand(1)?)?;
                let rhs = frame_slot(decoded.register_operand(2)?)?;
                emitter.emit_op_arith(family, dst, lhs, rhs, bci + 1);
            }
            CoreOpcode::Move => {
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let src = frame_slot(decoded.register_operand(1)?)?;
                emitter.emit_op_mov(dst, src);
            }
            CoreOpcode::LoadInt32 => {
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let value = decoded.signed_immediate_operand(1)?;
                emitter.emit_load_int32(dst, value);
            }
            CoreOpcode::LoadDouble => {
                // The raw f64 bits are split across two 32-bit immediate operands
                // (low, high), reconstructed exactly as the interpreter does
                // (interpreter/mod.rs:6417-6425) before boxing in `emit_load_double`.
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let low = decoded.unsigned_immediate_operand(1)?;
                let high = decoded.unsigned_immediate_operand(2)?;
                let bits = u64::from(low) | (u64::from(high) << 32);
                emitter.emit_load_double(dst, bits);
            }
            CoreOpcode::Jump => {
                let target_bci = decoded.bytecode_index_operand(0)?.offset() as usize;
                emitter.emit_op_jmp(target_bci);
            }
            CoreOpcode::JumpIfFalse => {
                let cond = frame_slot(decoded.register_operand(0)?)?;
                let target_bci = decoded.bytecode_index_operand(1)?.offset() as usize;
                emitter.emit_op_jfalse(cond, target_bci, bci + 1);
            }
            CoreOpcode::Return => {
                let value = frame_slot(decoded.register_operand(0)?)?;
                emitter.emit_op_ret(value);
            }
            // op_get_by_val / op_put_by_val — the asm.js HEAP element access. Operand
            // order matches the dispatch handlers (interpreter/mod.rs): get is
            // (dst, base, key); put is (base, key, value). Lowered as a leaf far-call
            // to the typed-array element bridge (see `emit_get_by_val`).
            CoreOpcode::GetByValue => {
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let base = frame_slot(decoded.register_operand(1)?)?;
                let key = frame_slot(decoded.register_operand(2)?)?;
                emitter.emit_get_by_val(dst, base, key);
            }
            CoreOpcode::PutByValue => {
                let base = frame_slot(decoded.register_operand(0)?)?;
                let key = frame_slot(decoded.register_operand(1)?)?;
                let value = frame_slot(decoded.register_operand(2)?)?;
                emitter.emit_put_by_val(base, key, value);
            }
            // op_get_by_id / op_put_by_id — the named-property DataIC. Operand order
            // matches the dispatch handlers (interpreter/mod.rs:8357,8431): get is
            // (dst, base, identifier); put is (base, identifier, value). Each site
            // consumes the next dense `property_site_index` and bakes the stable
            // record store base + index*16 (the install path sized the store to the
            // SAME bytecode-order GetByName+PutByName count). A site reached with no
            // store installed DECLINES the whole function (S4 gate).
            CoreOpcode::GetByName => {
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let base = frame_slot(decoded.register_operand(1)?)?;
                let key_index = decoded.identifier_index_operand(2)?;
                let record_base = code_block
                    .baseline_jit_data_record_store_base()
                    .ok_or(EmitFunctionError::MissingPropertyRecordStore)?;
                let record_index = emitter.property_site_index;
                emitter.property_site_index += 1;
                let record_addr =
                    record_base as usize + record_index as usize * PROPERTY_IC_RECORD_STRIDE;
                emitter.emit_get_by_id(dst, base, record_addr, record_index, key_index, bci as u32);
            }
            CoreOpcode::PutByName => {
                let base = frame_slot(decoded.register_operand(0)?)?;
                let key_index = decoded.identifier_index_operand(1)?;
                let value = frame_slot(decoded.register_operand(2)?)?;
                let record_base = code_block
                    .baseline_jit_data_record_store_base()
                    .ok_or(EmitFunctionError::MissingPropertyRecordStore)?;
                let record_index = emitter.property_site_index;
                emitter.property_site_index += 1;
                let record_addr =
                    record_base as usize + record_index as usize * PROPERTY_IC_RECORD_STRIDE;
                emitter.emit_put_by_id(
                    base,
                    value,
                    record_addr,
                    record_index,
                    key_index,
                    bci as u32,
                );
            }
            // op_call — the B5-first-cut UNLINKED VIRTUAL CALL. Operand order matches
            // the interpreter's `dispatch_call` and the bytecompiler's `emit_call`
            // (bytecompiler/mod.rs:5182-5192): operand 0 = dst, operand 1 = callee,
            // operand 2 = argc (UnsignedImmediate, EXCLUDING `this`), operands
            // 3..3+argc = the explicit-argument registers. A wrong mapping here would
            // hand the callee the wrong arguments (a silent wrong-answer), so this
            // mirrors `dispatch_call` exactly. Arity beyond the register ABI is
            // DECLINED (S4 gate) so the function stays interpreted.
            CoreOpcode::Call => {
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let callee = frame_slot(decoded.register_operand(1)?)?;
                let argc = decoded.unsigned_immediate_operand(2)?;
                if argc as usize > MAX_REGISTER_CALL_ARGS {
                    return Err(EmitFunctionError::UnsupportedCallArity { argc });
                }
                let mut args = Vec::with_capacity(argc as usize);
                for arg_index in 0..argc as usize {
                    args.push(frame_slot(decoded.register_operand(3 + arg_index)?)?);
                }
                // A pre-seeded LINKED target (the A1.3 proof) routes this site through
                // the native fast path with an ABSOLUTE baked entry; otherwise — the
                // LIVE install path — `emit_op_call_dynamic` RE-RESOLVES the callee's
                // installed baseline entry per call (A1.x broad engagement) and takes
                // the native fast path when the callee is itself baseline-JIT'd, else
                // the `operation_call` slow path. Cell-carrying calls are now allowed
                // (A1.5 roots the native JIT frames — see `emit_op_call_native_linked`).
                match linked_calls.iter().find(|t| t.bytecode_index == bci) {
                    Some(target) => {
                        emitter.emit_op_call_native_linked(dst, callee, &args, target.entry, bci)
                    }
                    None => {
                        emitter.emit_op_call_dynamic(dst, callee, &args, ThisSource::Undefined, bci)
                    }
                }
            }
            // op_call_with_this — the METHOD-call form (`o.m(args)`). Operand order
            // matches the interpreter's `dispatch_call_with_this` and the bytecompiler's
            // method-call emit (interpreter/mod.rs:12449-12480; bytecompiler/mod.rs:
            // 5262-5273): operand 0 = dst, operand 1 = callee, operand 2 = `this`
            // (the explicit RECEIVER), operand 3 = argc (UnsignedImmediate, EXCLUDING
            // `this`), operands 4..4+argc = the explicit-argument registers. The ONLY
            // delta from plain `Call` is that `this` is the receiver operand (not
            // `undefined`) — faithful to JSC's `compileSetupFrame` storing `thisValue`
            // into the callee frame's `thisArgument` slot (JITCall.cpp:117-119). The
            // live install path always RE-RESOLVES via `emit_op_call_dynamic` (no
            // pre-seeded LINKED proof for the method form). Arity beyond the
            // method-form register ABI (one fewer than plain Call, `this` consumes a
            // register) is DECLINED (S4 gate) so the function stays interpreted.
            CoreOpcode::CallWithThis => {
                let dst = frame_slot(decoded.register_operand(0)?)?;
                let callee = frame_slot(decoded.register_operand(1)?)?;
                let this_slot = frame_slot(decoded.register_operand(2)?)?;
                let argc = decoded.unsigned_immediate_operand(3)?;
                if argc as usize > MAX_REGISTER_CALL_WITH_THIS_ARGS {
                    return Err(EmitFunctionError::UnsupportedCallArity { argc });
                }
                let mut args = Vec::with_capacity(argc as usize);
                for arg_index in 0..argc as usize {
                    args.push(frame_slot(decoded.register_operand(4 + arg_index)?)?);
                }
                emitter.emit_op_call_dynamic(
                    dst,
                    callee,
                    &args,
                    ThisSource::Receiver(this_slot),
                    bci,
                );
            }
            // op_loop_hint is a tier-up/OSR marker with no value effect (the
            // interpreter treats it as Continue); emit nothing, fall through.
            CoreOpcode::LoopHint => {}
            // A relational op NOT fused with a JumpIfFalse needs the deferred
            // boolean-producing (cset) lowering; reject it (S4 gate).
            CoreOpcode::LessThanInt32
            | CoreOpcode::LessEqualInt32
            | CoreOpcode::GreaterThanInt32 => {
                return Err(EmitFunctionError::StandaloneRelational(op));
            }
            other => return Err(EmitFunctionError::UnsupportedOpcode(other)),
        }
        bci += 1;
    }

    // === EPILOGUE (the shared `done`) ======================================
    let done_label = emitter.emit_epilogue();

    // === SLOW pass (privateCompileSlowCases), after the epilogue ===========
    emitter.emit_slow_cases();

    // === EXCEPTION stub: bail to the epilogue without storing (op_add prereq #2,
    //     FIRST CUT — the real unwind is deferred). ==========================
    let exception_label = emitter.h.masm().label();
    let exception_to_done = emitter.h.masm_mut().jump();
    emitter.done_jumps.push(exception_to_done);

    // === A1.4 STACK-OVERFLOW stub (== `ThrowStackOverflowAtPrologue`): throw a
    //     faithful RangeError + minimal prologue teardown (no callee-save refill,
    //     since the prologue check fires before the spills). ==================
    let stack_overflow_label = emitter.emit_stack_overflow_stub();

    // === LINK pass (privateCompileLinkPass): resolve every recorded branch ==
    let mut link_records = emitter.label_link_records;
    for (jump, target_bci) in &emitter.jumps {
        // A branch target outside the labeled stream REJECTS the CodeBlock (S4
        // gate) instead of panicking on an out-of-range index — hardening the gate
        // against a malformed/unsupported stream that the U3/U4 live wiring may let
        // through. In-range targets are always labeled by the MAIN pass.
        let target = emitter.labels.get(*target_bci).copied().flatten().ok_or(
            EmitFunctionError::InvalidBranchTarget {
                target_bci: *target_bci,
            },
        )?;
        link_records.push(jump.to_link_record(target));
    }
    for jump in &emitter.done_jumps {
        link_records.push(jump.to_link_record(done_label));
    }
    for jump in &emitter.exception_jumps {
        link_records.push(jump.to_link_record(exception_label));
    }
    for jump in &emitter.stack_overflow_jumps {
        link_records.push(jump.to_link_record(stack_overflow_label));
    }

    Ok(FunctionImage {
        code: emitter.h.code().to_vec(),
        link_records,
        linked_call_sites: emitter.linked_call_sites,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        CodeKind, LinkContext, Operand, OperandWidth, PackedInstructionStream, TypedInstruction,
        UnlinkedCodeBlock, UnlinkedCodeBlockPhase, VirtualRegister,
    };

    // --- Frame-relative virtual registers for the test functions ----------------
    // Arguments (params) live at POSITIVE argument slots (the caller seeds them, and
    // op_enter never touches them); vars live at NEGATIVE local slots.
    const ARG0: i32 = 6; // VirtualRegister::argument_or_header(6)
    const ARG1: i32 = 7;
    const LOCAL0: i32 = -1; // VirtualRegister::local(0)
    const LOCAL1: i32 = -2;
    const LOCAL2: i32 = -3;
    const LOCAL3: i32 = -4;

    fn reg(raw: i32) -> Operand {
        Operand::Register(VirtualRegister::from_raw(raw))
    }

    fn imm(value: i32) -> Operand {
        Operand::SignedImmediate(value)
    }

    fn jump_target(bci: usize) -> Operand {
        Operand::BytecodeIndex(BytecodeIndex::from_offset(bci as u32))
    }

    fn instr(op: CoreOpcode, operands: Vec<Operand>, bci: usize) -> TypedInstruction {
        TypedInstruction {
            opcode: op.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(BytecodeIndex::from_offset(bci as u32)),
        }
    }

    // --- A1.0 divergence-correction: the emitter now emits the FAITHFUL JSC
    // `emitFunctionPrologue` (`pushPair(fp,lr); mov fp,sp`) and `emitFunctionEpilogue`
    // (`mov sp,fp; ldp fp,lr; ret`), building the CallFrame on the native machine
    // stack — NOT the prior Option-D `mov fp,x1` (fp=arena / sp=native) split. This
    // proves the emitted bytes match the byte-validated entry_prologue.rs contract.
    #[test]
    fn prologue_and_epilogue_match_jsc_generated_frame_contract() {
        use super::super::entry_prologue::{
            ARM64_JSC_BASELINE_GENERATED_EPILOGUE_BYTES,
            ARM64_JSC_BASELINE_GENERATED_PROLOGUE_BYTES,
        };

        // num_locals == 0 omits the `sub sp` reservation, so bytes 0..8 are exactly
        // the `stp fp,lr,[sp,#-16]!; mov fp,sp` head.
        let mut prologue = FunctionEmitter::new(1, 0, 0);
        prologue.emit_prologue(0);
        assert_eq!(
            &prologue.h.code()[0..8],
            ARM64_JSC_BASELINE_GENERATED_PROLOGUE_BYTES,
            "prologue head must be the faithful `stp fp,lr,[sp,#-16]!; mov fp,sp`",
        );

        // The epilogue ends with `mov sp,fp; ldp fp,lr,[sp],#16; ret` (12 bytes)
        // after the callee-save refills.
        let mut epilogue = FunctionEmitter::new(1, 0, 0);
        epilogue.emit_epilogue();
        let code = epilogue.h.code();
        assert_eq!(
            &code[code.len() - 12..],
            ARM64_JSC_BASELINE_GENERATED_EPILOGUE_BYTES,
            "epilogue tail must be the faithful `mov sp,fp; ldp fp,lr; ret`",
        );
    }

    /// A `CoreOpcode::LoadDouble` for `value` — the raw f64 bits split low/high
    /// across two unsigned-immediate operands, BYTE-IDENTICAL to the bytecompiler's
    /// `emit_load_double` (bytecompiler/mod.rs:6712-6730).
    fn load_double_instr(dst: i32, value: f64, bci: usize) -> TypedInstruction {
        let bits = value.to_bits();
        instr(
            CoreOpcode::LoadDouble,
            vec![
                reg(dst),
                Operand::UnsignedImmediate((bits & u64::from(u32::MAX)) as u32),
                Operand::UnsignedImmediate((bits >> 32) as u32),
            ],
            bci,
        )
    }

    // `function f() { return <value>; }` — a single double LITERAL load + return.
    fn return_double_instructions(value: f64) -> Vec<TypedInstruction> {
        vec![
            load_double_instr(LOCAL0, value, 0),
            instr(CoreOpcode::Return, vec![reg(LOCAL0)], 1),
        ]
    }

    // `function f(x) { return x * 2.5 + 1.5; }` — double LITERALS (LoadDouble) feed
    // the JSVALUE64 double fast path. x=ARG0, c0=LOCAL0 (2.5), acc=LOCAL1, c1=LOCAL2
    // (1.5). Ordinals: 0 c0=2.5  1 acc=x*c0  2 c1=1.5  3 acc=acc+c1  4 ret acc.
    fn double_literal_arith_instructions() -> Vec<TypedInstruction> {
        vec![
            load_double_instr(LOCAL0, 2.5, 0),
            instr(
                CoreOpcode::MulInt32,
                vec![reg(LOCAL1), reg(ARG0), reg(LOCAL0)],
                1,
            ),
            load_double_instr(LOCAL2, 1.5, 2),
            instr(
                CoreOpcode::AddInt32,
                vec![reg(LOCAL1), reg(LOCAL1), reg(LOCAL2)],
                3,
            ),
            instr(CoreOpcode::Return, vec![reg(LOCAL1)], 4),
        ]
    }

    /// Build a linked `CodeBlock` directly from a decoded-instruction sequence (the
    /// faithful view a real `CodeBlock::instructions()` iteration yields; the
    /// bytecompiler->linked-CodeBlock path is the U3 live-dispatch coupling).
    fn build_code_block(instructions: Vec<TypedInstruction>) -> CodeBlock {
        let stream = PackedInstructionStream::from_typed_placeholder(instructions);
        let unlinked = UnlinkedCodeBlock::new(CodeKind::Function, stream)
            .with_phase(UnlinkedCodeBlockPhase::Finalized);
        CodeBlock::from_unlinked(unlinked, LinkContext::default())
    }

    // `(a, b) => { var tmp = a; return tmp + b; }` — op_enter/mov/add/ret.
    fn smoke_instructions() -> Vec<TypedInstruction> {
        vec![
            instr(CoreOpcode::Move, vec![reg(LOCAL0), reg(ARG0)], 0),
            instr(
                CoreOpcode::AddInt32,
                vec![reg(LOCAL1), reg(LOCAL0), reg(ARG1)],
                1,
            ),
            instr(CoreOpcode::Return, vec![reg(LOCAL1)], 2),
        ]
    }

    // `function f(n) { var s=0; for (var i=0; i<n; i++) s = s+i; return s; }`
    // s=LOCAL0, i=LOCAL1, one=LOCAL2, cmp=LOCAL3, n=ARG0. Ordinals:
    //   0 s=0  1 i=0  2 one=1  3 (i<n)?  4 jfalse->8  5 s+=i  6 i+=one  7 jmp->3  8 ret s
    fn int_sum_loop_instructions() -> Vec<TypedInstruction> {
        vec![
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL0), imm(0)], 0),
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL1), imm(0)], 1),
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL2), imm(1)], 2),
            instr(
                CoreOpcode::LessThanInt32,
                vec![reg(LOCAL3), reg(LOCAL1), reg(ARG0)],
                3,
            ),
            instr(
                CoreOpcode::JumpIfFalse,
                vec![reg(LOCAL3), jump_target(8)],
                4,
            ),
            instr(
                CoreOpcode::AddInt32,
                vec![reg(LOCAL0), reg(LOCAL0), reg(LOCAL1)],
                5,
            ),
            instr(
                CoreOpcode::AddInt32,
                vec![reg(LOCAL1), reg(LOCAL1), reg(LOCAL2)],
                6,
            ),
            instr(CoreOpcode::Jump, vec![jump_target(3)], 7),
            instr(CoreOpcode::Return, vec![reg(LOCAL0)], 8),
        ]
    }

    // `function f(b) { if (b) return 1; return 2; }` — standalone op_jfalse.
    //   0 jfalse b -> 3   1 r=1   2 ret r   3 r=2   4 ret r
    fn if_else_instructions() -> Vec<TypedInstruction> {
        vec![
            instr(CoreOpcode::JumpIfFalse, vec![reg(ARG0), jump_target(3)], 0),
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL0), imm(1)], 1),
            instr(CoreOpcode::Return, vec![reg(LOCAL0)], 2),
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL0), imm(2)], 3),
            instr(CoreOpcode::Return, vec![reg(LOCAL0)], 4),
        ]
    }

    fn words(code: &[u8]) -> Vec<u32> {
        code.chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    // ------------------------------------------------------------------------
    // STRUCTURAL ORACLE (runs on every platform): the int-sum loop lowers to a
    // 3-pass image whose prologue is byte-identical to op_add's and whose link
    // records cover every recorded branch (forward exit + backward loop +
    // op_ret/exception to the epilogue + the compare slow case's guards/resume).
    // ------------------------------------------------------------------------
    #[test]
    fn int_sum_loop_emits_three_pass_image() {
        let code_block = build_code_block(int_sum_loop_instructions());
        let image = emit_baseline_function(&code_block, 0x1000, 0x2000).expect("emit loop");
        let w = words(&image.code);

        // Prologue: the faithful Option-A `emitFunctionPrologue` (A1) building the
        // CallFrame on the native machine stack, followed by the locals reservation.
        assert_eq!(w[0], 0xa9bf_7bfd, "stp fp,lr,[sp,#-16]!");
        assert_eq!(w[1], 0x9100_03fd, "mov fp, sp (cfr)");
        // `sub sp, sp, #32` reserves the 4 callee locals (LOCAL0..LOCAL3) below fp
        // so the callee-save spills land below them (no op_enter overlap).
        assert_eq!(
            w[2], 0xd100_83ff,
            "sub sp, sp, #32 (reserve 4 callee locals)"
        );
        // The epilogue `ret` plus the exception stub's trailing `b -> done` are the
        // last two words; the very last is the exception stub's unconditional branch.
        assert!(w.contains(&0xd65f_03c0), "image contains the epilogue ret");

        // Every recorded branch is linked. Each AddInt32 now also emits the JSVALUE64
        // double fast path (JITAddGenerator), so its links grow: the int32 path's
        // end-jump SKIPPING the double path (+1, bci-targeted to resume), 3
        // branchIfNotNumber slow guards (so the slow-guard count is overflow+3=4 not
        // 1), and 4 internal double-path control-flow links. Breakdown across the 4
        // control-flow ops (the fused compare at 3-4, the two AddInt32 at 5/6, the
        // op_jmp at 7) plus op_ret (8) and the exception stub:
        //   bci-targeted jumps: compare{fastTarget,slowTarget,slowResume}=3,
        //       add5{end,slowResume}=2, add6{end,slowResume}=2, jmp->3 =1        -> 8
        //   label jumps (guard -> slow label): compare 2, add5{overflow+3}=4,
        //       add6=4, plus the 4+4 double-path internal links                 -> 18
        //   exception jumps (slow exception probe): compare 1, add5 1, add6 1   -> 3
        //   done jumps: op_ret 1, exception stub 1                              -> 2
        //   A1.4 stack-overflow guard: prologue softStackLimit -> overflow stub -> 1
        assert_eq!(image.link_records.len(), 32, "every branch is linked");
    }

    #[test]
    fn rejects_constant_operand_and_empty_function_like_the_s4_gate() {
        // A constant-pool operand is outside the Stage-1 frame-slot allowlist -> the
        // function is REJECTED (stays in the interpreter), not mis-lowered.
        let code_block = build_code_block(vec![instr(
            CoreOpcode::Return,
            vec![Operand::Register(VirtualRegister::constant(0))],
            0,
        )]);
        assert!(matches!(
            emit_baseline_function(&code_block, 0x1000, 0x2000),
            Err(EmitFunctionError::ConstantOperand)
        ));

        let empty = build_code_block(Vec::new());
        assert!(matches!(
            emit_baseline_function(&empty, 0x1000, 0x2000),
            Err(EmitFunctionError::EmptyFunction)
        ));
    }

    // ------------------------------------------------------------------------
    // FUSION DEADNESS GUARD: a CodeBlock shaped `let b = a<c; if(!b){…}; return b;`
    // reads the comparison's boolean AFTER the JumpIfFalse. The fused compare-branch
    // materializes NO value for `b`, so fusing would make the later `return b` read
    // the stale op_enter `undefined` slot. The emitter must DECLINE (not fuse, not
    // silently mis-emit) — falling through to the relational reject arm (S4 gate).
    //   a=ARG0 c=ARG1 b=LOCAL0 r=LOCAL1
    //   0 LessThan b,a,c   1 jfalse b->4   2 r=1   3 ret r   4 ret b   (reads b!)
    // ------------------------------------------------------------------------
    #[test]
    fn declines_fusion_when_comparison_result_is_read_after_the_jump() {
        let live_after = build_code_block(vec![
            instr(
                CoreOpcode::LessThanInt32,
                vec![reg(LOCAL0), reg(ARG0), reg(ARG1)],
                0,
            ),
            instr(
                CoreOpcode::JumpIfFalse,
                vec![reg(LOCAL0), jump_target(4)],
                1,
            ),
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL1), imm(1)], 2),
            instr(CoreOpcode::Return, vec![reg(LOCAL1)], 3),
            instr(CoreOpcode::Return, vec![reg(LOCAL0)], 4),
        ]);
        assert!(
            matches!(
                emit_baseline_function(&live_after, 0x1000, 0x2000),
                Err(EmitFunctionError::StandaloneRelational(
                    CoreOpcode::LessThanInt32
                ))
            ),
            "a comparison result read after its JumpIfFalse must NOT fuse (S4 reject)"
        );

        // Control: with the trailing `return b` removed (b is DEAD after the jump),
        // the SAME shape fuses and emits cleanly — proving the guard is precise, not
        // a blanket refusal of the pattern.
        let dead_after = build_code_block(vec![
            instr(
                CoreOpcode::LessThanInt32,
                vec![reg(LOCAL0), reg(ARG0), reg(ARG1)],
                0,
            ),
            instr(
                CoreOpcode::JumpIfFalse,
                vec![reg(LOCAL0), jump_target(3)],
                1,
            ),
            instr(CoreOpcode::LoadInt32, vec![reg(LOCAL1), imm(1)], 2),
            instr(CoreOpcode::Return, vec![reg(LOCAL1)], 3),
        ]);
        assert!(
            emit_baseline_function(&dead_after, 0x1000, 0x2000).is_ok(),
            "a dead comparison result fuses cleanly"
        );
    }

    // ------------------------------------------------------------------------
    // BRANCH-TARGET BOUNDS: a branch to a bytecode index outside the stream REJECTS
    // the CodeBlock (S4 gate) rather than panicking in the LINK pass.
    // ------------------------------------------------------------------------
    #[test]
    fn rejects_out_of_range_branch_target() {
        let bad_jump = build_code_block(vec![instr(CoreOpcode::Jump, vec![jump_target(5)], 0)]);
        assert!(matches!(
            emit_baseline_function(&bad_jump, 0x1000, 0x2000),
            Err(EmitFunctionError::InvalidBranchTarget { target_bci: 5 })
        ));
    }

    // ------------------------------------------------------------------------
    // ALLOWLIST EXTENSION: a function using a double LITERAL (LoadDouble) is now
    // ADMITTED (lowered to an image) rather than REJECTED with
    // UnsupportedOpcode(LoadDouble) — the S4 gate now lets double-literal asm.js
    // functions tier up.
    // ------------------------------------------------------------------------
    #[test]
    fn load_double_function_is_admitted_by_the_allowlist() {
        let code_block = build_code_block(double_literal_arith_instructions());
        assert!(
            emit_baseline_function(&code_block, 0x1000, 0x2000).is_ok(),
            "a double-literal arith function tiers up (LoadDouble admitted)"
        );
    }

    // ALLOWLIST EXTENSION (structural oracle, every platform): a function doing
    // typed-array element access `arr[i] = v; return arr[i]` (op_put_by_val then
    // op_get_by_val) is now ADMITTED by the S4 allowlist and lowers to a 3-pass
    // image (prologue byte-identical to op_add's, an epilogue `ret`). Each access is
    // a leaf far-call to the element bridge; the exception-stub edges are linked.
    // ------------------------------------------------------------------------
    #[test]
    fn get_put_by_val_typed_array_are_admitted_and_lower_to_a_finalizable_image() {
        // arr=ARG0(6), i=ARG1(7), v=arg2(8), dst=LOCAL0.
        let code_block = build_code_block(vec![
            instr(
                CoreOpcode::PutByValue,
                vec![reg(ARG0), reg(ARG1), reg(8)],
                0,
            ),
            instr(
                CoreOpcode::GetByValue,
                vec![reg(LOCAL0), reg(ARG0), reg(ARG1)],
                1,
            ),
            instr(CoreOpcode::Return, vec![reg(LOCAL0)], 2),
        ]);
        let image = emit_baseline_function(&code_block, 0x1000, 0x2000)
            .expect("get_by_val/put_by_val are admitted (S4 allowlist extension)");
        let w = words(&image.code);
        assert_eq!(
            w[0], 0xa9bf_7bfd,
            "stp fp,lr,[sp,#-16]! (op_add's prologue)"
        );
        assert!(w.contains(&0xd65f_03c0), "image contains the epilogue ret");
        // Two element accesses, each probing the m_exception mirror -> two exception
        // edges, plus op_ret + the exception stub reaching the shared epilogue.
        assert!(
            !image.link_records.is_empty(),
            "the far-call exception/epilogue edges are linked",
        );
    }

    // ------------------------------------------------------------------------
    // THE MILESTONE: emit -> relocate -> EXECUTE a WHOLE function image under W^X.
    // Mirrors op_add/arith's harness (a real Vm + dispatch host + a JSStack frame),
    // but the image is a full CodeBlock-driven function with op_enter, an op_mov,
    // an int32 arith loop, a fused int32 compare-and-branch (forward exit), and a
    // backward loop branch. macOS/aarch64 only (executes native ARM64).
    // ------------------------------------------------------------------------
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    mod execution {
        use super::*;
        use crate::interpreter::CoreOpcodeDispatchHost;
        use crate::jit::executable_allocator::{
            finalize_arm64_link_buffer, MapJitExecutableAllocator,
        };
        use crate::value::{EncodedJsValue, JsValue};
        use crate::vm::jsstack::{JsStack, Register};
        use crate::vm::{Vm, VmConfig};

        struct Frame {
            stack: JsStack,
            fp: usize,
        }

        impl Frame {
            fn new() -> Self {
                // A1: a LIVE mmap JS stack (roomy + low-end PROT_NONE guard page),
                // because the flipped prologue runs the image on the native machine
                // `sp` switched INTO this region — the arith slow-path far-calls
                // (operation_compare/jfalse) descend below `fp` here, so the tiny
                // `with_test_backing` heap box would overflow. `fp` sits near the
                // high end: positive arg/header slots above it, locals + far-call
                // frames descend toward the guard page below it.
                let stack = JsStack::new(1 << 20).expect("live js stack reservation");
                let fp = stack.high_address() - 256;
                Frame { stack, fp }
            }
            fn slot_addr(&self, operand: i32) -> usize {
                (self.fp as isize + operand as isize * 8) as usize
            }
            fn write(&self, operand: i32, bits: u64) {
                assert!(
                    self.stack
                        .write_slot(self.slot_addr(operand), Register::from_bits(bits)),
                    "frame slot in range"
                );
            }
            fn read(&self, operand: i32) -> u64 {
                self.stack
                    .read_slot(self.slot_addr(operand))
                    .unwrap()
                    .bits()
            }
        }

        struct RunResult {
            ret: u64,
            pending: u64,
        }

        /// Finalize + execute a function image, seeding the argument slots first.
        /// Sets the A1.4 soft stack limit to the frame's native stack low bound, so
        /// the prologue overflow check is ARMED but never fires for these
        /// normal-depth frames (the limit sits far below `fp`).
        fn run_function(
            instructions: Vec<TypedInstruction>,
            args: &[(i32, u64)],
        ) -> (RunResult, Frame) {
            let mut host = CoreOpcodeDispatchHost::new();
            let mut vm = Vm::new(VmConfig::interpreter_only());
            vm.set_jit_host(&mut host); // D5: park the host before entering JIT code.
            let jit_pending_address = vm.jit_pending_exception_address() as usize;
            let soft_stack_limit_address = vm.jit_soft_stack_limit_address() as usize;

            let frame = Frame::new();
            for &(operand, bits) in args {
                frame.write(operand, bits);
            }
            // A1.4: arm the prologue overflow check with this stack's real low bound;
            // `fp` is near the high end, so a normal frame top stays well above it.
            vm.set_jit_soft_stack_limit(frame.stack.stack_limit());

            let code_block = build_code_block(instructions);
            let image =
                emit_baseline_function(&code_block, jit_pending_address, soft_stack_limit_address)
                    .expect("emit function");
            let mut records = image.link_records;
            let handle =
                finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                    .expect("finalize function image");

            let vm_ptr: *mut Vm = &mut vm;
            // A1: enter on the native stack via the baseline-JIT entry trampoline.
            // `entry_sp = fp + sizeof(CallerFrameAndPC)` (16): the prologue's
            // `pushPair(fp,lr)` lands sp at `fp` and `mov fp,sp` captures the base,
            // so `addressFor(operand)` reads the slots the args were seeded into.
            let ret = handle.call_baseline_jit_entry(frame.fp + 16, vm_ptr as u64);

            let pending = vm.jit_pending_exception().0;
            vm.clear_jit_host();
            (RunResult { ret, pending }, frame)
        }

        fn i32_bits(value: i32) -> u64 {
            JsValue::from_i32(value).encoded().0
        }

        // --- SMOKE: a straight-line `(a,b)=>{ var tmp=a; return tmp+b; }`. ------
        #[test]
        fn smoke_straight_line_add_returns_native_result() {
            let (r, frame) = run_function(
                smoke_instructions(),
                &[(ARG0, i32_bits(2)), (ARG1, i32_bits(3))],
            );
            assert_eq!(r.ret, i32_bits(5), "returnValue = boxed int32 5");
            assert_eq!(frame.read(LOCAL1), i32_bits(5), "result slot holds 5");
            assert_eq!(r.pending, 0, "no exception");
            assert!(JsValue::from_encoded(EncodedJsValue(r.ret)).is_int32());
        }

        // --- A1.4: the baseline prologue's softStackLimit check (JIT.cpp:773-783).
        // A frame whose top would drop below the soft stack limit throws a FAITHFUL
        // stack-overflow RangeError from the PROLOGUE (createStackOverflowError),
        // bailing cleanly — NOT a SIGSEGV/guard-page fault, NOT silent corruption,
        // and WITHOUT running op_enter or the body. Driven synthetically by setting
        // the limit AT `fp` (modeling "no stack left below this frame", the deep-
        // recursion limit): a real self-recursive native bl would hit the SAME
        // per-frame prologue check, frame by frame. The SAME image with the real
        // (low) limit does NOT trip the check — proving it is the limit, not the
        // code, that decides.
        #[test]
        fn prologue_softstacklimit_throws_stack_overflow_rangeerror() {
            let mut host = CoreOpcodeDispatchHost::new();
            let mut vm = Vm::new(VmConfig::interpreter_only());
            vm.set_jit_host(&mut host); // D5: park the host before entering JIT code.
            let jit_pending_address = vm.jit_pending_exception_address() as usize;
            let soft_stack_limit_address = vm.jit_soft_stack_limit_address() as usize;

            // The straight-line smoke add `(a,b)=>{ var tmp=a; return tmp+b }`.
            let code_block = build_code_block(smoke_instructions());
            let image =
                emit_baseline_function(&code_block, jit_pending_address, soft_stack_limit_address)
                    .expect("emit smoke function");
            let mut records = image.link_records;
            let handle =
                finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                    .expect("finalize function image");

            // A distinctive sentinel pre-seeded into the result slot LOCAL1: if the
            // throw is truly from the prologue, NEITHER op_enter's local zero-fill NOR
            // the body runs, so LOCAL1 keeps the sentinel.
            let sentinel = i32_bits(0x5EED);

            // (1) ARMED-BUT-SAFE: with the stack's real low bound the frame top stays
            //     well above the limit -> the check does NOT fire; 2 + 3 == 5.
            let ok_frame = Frame::new();
            ok_frame.write(ARG0, i32_bits(2));
            ok_frame.write(ARG1, i32_bits(3));
            ok_frame.write(LOCAL1, sentinel);
            vm.set_jit_soft_stack_limit(ok_frame.stack.stack_limit());
            let ok_ret = {
                let vm_ptr: *mut Vm = &mut vm;
                handle.call_baseline_jit_entry(ok_frame.fp + 16, vm_ptr as u64)
            };
            assert_eq!(
                ok_ret,
                i32_bits(5),
                "a normal frame does not trip the check"
            );
            assert_eq!(
                vm.jit_pending_exception().0,
                0,
                "no overflow for a normal frame"
            );
            assert_eq!(
                ok_frame.read(LOCAL1),
                i32_bits(5),
                "the body ran (slot == 5)"
            );

            // (2) OVERFLOW: set the soft limit AT `fp`, so the frame top `fp -
            //     frameSize` (< fp) trips the prologue check before op_enter/body.
            let of_frame = Frame::new();
            of_frame.write(ARG0, i32_bits(2));
            of_frame.write(ARG1, i32_bits(3));
            of_frame.write(LOCAL1, sentinel);
            vm.set_jit_soft_stack_limit(of_frame.fp);
            let of_ret = {
                let vm_ptr: *mut Vm = &mut vm;
                handle.call_baseline_jit_entry(of_frame.fp + 16, vm_ptr as u64)
            };
            // Reaching here proves NO SIGSEGV/guard-page fault: the throw was a clean
            // JS bail through the overflow stub's minimal teardown, not a hard crash.
            let pending = vm.jit_pending_exception();
            assert_ne!(pending.0, 0, "the prologue stamped m_exception (it threw)");
            assert_eq!(of_ret, 0, "the throw edge returns JSValue::empty() bits");
            assert_eq!(
                of_frame.read(LOCAL1),
                sentinel,
                "neither op_enter nor the body ran (LOCAL1 keeps its sentinel): the \
                 throw is from the PROLOGUE",
            );
            let thrown = JsValue::from_encoded(pending);
            assert!(
                thrown.is_cell(),
                "the thrown value is a heap cell (the error)"
            );

            // Confirm the thrown cell is specifically a faithful RangeError.
            vm.clear_jit_host();
            assert!(
                host.jit_bridge_value_is_range_error(thrown),
                "the stack-overflow throw is a RangeError (createStackOverflowError)",
            );
        }

        // --- THE MILESTONE: the int-sum loop runs NATIVELY, the backward branch
        // links and loops, the forward compare-branch exits, the result is correct.
        #[test]
        fn int_sum_loop_runs_natively_with_backward_branch() {
            for (n, expected) in [(0, 0), (1, 0), (2, 1), (5, 10), (10, 45)] {
                let (r, frame) = run_function(int_sum_loop_instructions(), &[(ARG0, i32_bits(n))]);
                assert_eq!(
                    r.ret,
                    i32_bits(expected),
                    "f({n}) returns the boxed int32 sum {expected}"
                );
                assert_eq!(
                    frame.read(LOCAL0),
                    i32_bits(expected),
                    "s slot holds the sum"
                );
                assert_eq!(r.pending, 0, "no exception");
            }
        }

        // --- The fused compare's SLOW path: a DOUBLE loop bound fails the int32
        // guard each iteration -> operation_compare_less -> faithful numeric compare;
        // the loop still computes the right sum natively around the slow compare.
        #[test]
        fn loop_with_double_bound_takes_compare_slow_path() {
            let (r, _frame) = run_function(
                int_sum_loop_instructions(),
                &[(ARG0, JsValue::from_double(5.0).encoded().0)],
            );
            assert_eq!(
                r.ret,
                i32_bits(10),
                "f(5.0) sums 0..5 via the compare slow path"
            );
            assert_eq!(r.pending, 0, "no exception");
        }

        // --- Standalone op_jfalse: boolean fast path + the ToBoolean slow path for
        // non-boolean (int) operands. `function f(b){ if(b) return 1; return 2; }`.
        #[test]
        fn standalone_jfalse_boolean_and_truthy_slow_path() {
            // Boolean operands -> the inline LSB fast path.
            let (r, _) = run_function(
                if_else_instructions(),
                &[(ARG0, JsValue::from_bool(true).encoded().0)],
            );
            assert_eq!(r.ret, i32_bits(1), "if(true) -> 1");

            let (r, _) = run_function(
                if_else_instructions(),
                &[(ARG0, JsValue::from_bool(false).encoded().0)],
            );
            assert_eq!(r.ret, i32_bits(2), "if(false) -> 2");

            // Non-boolean (int) operands -> the operation_jfalse (ToBoolean) slow path.
            let (r, _) = run_function(if_else_instructions(), &[(ARG0, i32_bits(5))]);
            assert_eq!(r.ret, i32_bits(1), "if(5) is truthy -> 1 (slow path)");

            let (r, _) = run_function(if_else_instructions(), &[(ARG0, i32_bits(0))]);
            assert_eq!(r.ret, i32_bits(2), "if(0) is falsy -> 2 (slow path)");
        }

        // --- THE DOUBLE MILESTONE (full CodeBlock): a whole function whose
        // arithmetic runs through the JSVALUE64 DOUBLE fast path natively. The
        // straight-line `(a,b)=>{ var tmp=a; return tmp+b; }` add, and a
        // `(a,b)=>a/b` op_div, both compute their results with in-register FP
        // (unbox/scvtf -> fadd/fdiv -> boxDouble) and return the boxed double. Runs
        // under debug AND release.
        #[test]
        fn double_arith_function_tiers_up_and_runs_native_fp() {
            let d = |x: f64| JsValue::from_double(x).encoded().0;

            // op_add via the double fast path: 2.5 + 0.25 = 2.75 (non-integral, so
            // the native boxDouble matches the runtime from_double bit-for-bit).
            let (r, frame) = run_function(smoke_instructions(), &[(ARG0, d(2.5)), (ARG1, d(0.25))]);
            assert_eq!(r.ret, d(2.75), "(2.5)+(0.25) returns boxed double 2.75");
            assert_eq!(frame.read(LOCAL1), d(2.75), "result slot holds 2.75");
            assert_eq!(r.pending, 0, "no exception");
            assert!(
                JsValue::from_encoded(EncodedJsValue(r.ret)).is_double(),
                "the add result is a double (FP fast path)"
            );

            // Mixed int/double through the SAME add: 2 + 0.5 = 2.5.
            let (r, _) = run_function(smoke_instructions(), &[(ARG0, i32_bits(2)), (ARG1, d(0.5))]);
            assert_eq!(
                r.ret,
                d(2.5),
                "(int 2)+(0.5) -> 2.5 via the double fast path"
            );

            // op_div: `(a,b)=>a/b`. The double-only JITDivGenerator path.
            let div = vec![
                instr(
                    CoreOpcode::DivNumber,
                    vec![reg(LOCAL0), reg(ARG0), reg(ARG1)],
                    0,
                ),
                instr(CoreOpcode::Return, vec![reg(LOCAL0)], 1),
            ];
            // double / double = 3.5.
            let (r, _) = run_function(div.clone(), &[(ARG0, d(7.0)), (ARG1, d(2.0))]);
            assert_eq!(r.ret, d(3.5), "7.0 / 2.0 -> 3.5 (native fdiv)");
            assert_eq!(r.pending, 0, "no exception");
            // int / int divides via double too: 15 / 4 = 3.75.
            let (r, _) = run_function(div.clone(), &[(ARG0, i32_bits(15)), (ARG1, i32_bits(4))]);
            assert_eq!(
                r.ret,
                d(3.75),
                "15 / 4 -> 3.75 (int operands, double divide)"
            );
            // div by zero -> +Infinity (no throw).
            let (r, _) = run_function(div, &[(ARG0, d(1.0)), (ARG1, i32_bits(0))]);
            assert_eq!(r.ret, d(f64::INFINITY), "1.0 / 0 -> +Infinity");
            assert_eq!(r.pending, 0, "div by zero raises no exception");
        }

        // --- LoadDouble MATERIALIZATION, bit-for-bit vs the interpreter: the boxed
        // constant the native image writes must equal the interpreter's
        // `RuntimeValue::from_double(f64::from_bits(bits))` (interpreter/mod.rs:6426-
        // 6431) EXACTLY, for both representation classes from_double can produce.
        #[test]
        fn load_double_materializes_boxed_constant_bit_for_bit() {
            // (1) A TRUE (non-integral) double stays an offset-encoded double.
            let (r, frame) = run_function(return_double_instructions(2.5), &[]);
            let expected = JsValue::from_double(2.5).encoded().0;
            assert_eq!(
                r.ret, expected,
                "LoadDouble 2.5 == interpreter from_double(2.5)"
            );
            assert_eq!(frame.read(LOCAL0), expected, "slot holds the boxed 2.5");
            assert!(
                JsValue::from_encoded(EncodedJsValue(r.ret)).is_double(),
                "2.5 stays a boxed double"
            );
            assert_eq!(r.pending, 0, "no exception");

            // (2) An exactly-representable INTEGRAL double CANONICALIZES to a boxed
            // int32 (the from_double strict-int32 fold) — the JIT must reproduce
            // this, not emit an offset-encoded double. This is the canonical-int
            // wrinkle the comment documents.
            let (r, _) = run_function(return_double_instructions(4.0), &[]);
            let expected = JsValue::from_double(4.0).encoded().0;
            assert_eq!(
                r.ret, expected,
                "LoadDouble 4.0 == interpreter from_double(4.0)"
            );
            assert_eq!(
                expected,
                JsValue::from_i32(4).encoded().0,
                "from_double(4.0) canonicalizes to boxed int32 4"
            );
            assert!(
                JsValue::from_encoded(EncodedJsValue(r.ret)).is_int32(),
                "4.0 canonicalizes to a boxed int32"
            );
            assert_eq!(r.pending, 0, "no exception");
        }

        // --- THE LoadDouble MILESTONE: a whole function using double LITERALS in
        // arithmetic — `function f(x){ return x*2.5 + 1.5; }` — TIERS UP and runs
        // the JSVALUE64 double fast path natively, the literals materialized by
        // LoadDouble. Inputs are chosen so the result is NON-INTEGRAL (the native
        // boxDouble then equals the value layer's from_double bit-for-bit, avoiding
        // the integral int-fold wrinkle). Runs under debug AND release.
        #[test]
        fn double_literal_arith_function_tiers_up_native() {
            let d = |x: f64| JsValue::from_double(x).encoded().0;

            // int arg: 2 * 2.5 + 1.5 = 6.5 (non-integral).
            let (r, frame) =
                run_function(double_literal_arith_instructions(), &[(ARG0, i32_bits(2))]);
            assert_eq!(
                r.ret,
                d(6.5),
                "f(2) = 2*2.5+1.5 = 6.5 (double literals, native FP)"
            );
            assert_eq!(frame.read(LOCAL1), d(6.5), "accumulator slot holds 6.5");
            assert!(
                JsValue::from_encoded(EncodedJsValue(r.ret)).is_double(),
                "6.5 is a boxed double"
            );
            assert_eq!(r.pending, 0, "no exception");

            // double arg: 2.5 * 2.5 + 1.5 = 7.75.
            let (r, _) = run_function(double_literal_arith_instructions(), &[(ARG0, d(2.5))]);
            assert_eq!(r.ret, d(7.75), "f(2.5) = 2.5*2.5+1.5 = 7.75");
            assert_eq!(r.pending, 0, "no exception");

            // another int arg: 4 * 2.5 + 1.5 = 11.5.
            let (r, _) = run_function(double_literal_arith_instructions(), &[(ARG0, i32_bits(4))]);
            assert_eq!(r.ret, d(11.5), "f(4) = 4*2.5+1.5 = 11.5");
            assert_eq!(r.pending, 0, "no exception");
        }

        // --- BASELINE PROPERTY DataIC (get_by_id / put_by_id), the K2 R-lever -----
        // These execute the NATIVE structure-guard + record-fill DataIC: a property
        // function tiers up (the gate admits GetByName/PutByName), runs native, and
        // native == interpreter == oracle across object shapes. native == interpreter
        // is STRUCTURAL: the slow/hit far-calls run the interpreter's OWN resolution
        // (`Vm::operation_get_by_id_*` -> `host.jit_get_by_id` -> `get_property_value`,
        // the SAME code the interpreter dispatch runs), so the asserted oracle value
        // is the interpreter's value too. macOS/aarch64 only (executes native ARM64).
        use crate::interpreter::CorePropertyKey;
        use crate::jit::executable_allocator::ExecutableMemoryHandle as PropertyIcHandle;

        fn key_x() -> CorePropertyKey {
            CorePropertyKey::String("x".into())
        }

        /// A plain object `{ x: <value> }` allocated in the host's object store (the
        /// store the DataIC slow-path bridge resolves against). Returns the boxed
        /// cell value the frame seeds and generated code unboxes.
        fn object_with_x(host: &mut CoreOpcodeDispatchHost, value: i32) -> u64 {
            let object = host.jit_test_allocate_object();
            host.jit_test_set_own_data(object, &key_x(), JsValue::from_i32(value));
            object.encoded().0
        }

        fn structure_of(host: &CoreOpcodeDispatchHost, boxed: u64) -> u32 {
            host.jit_test_structure_id(JsValue::from_encoded(EncodedJsValue(boxed)))
                .expect("live object cell")
        }

        /// `f(o) { return o.x }` — get_by_id only (identifier index 0 == "x").
        fn get_x_instructions() -> Vec<TypedInstruction> {
            vec![
                instr(
                    CoreOpcode::GetByName,
                    vec![reg(LOCAL0), reg(ARG0), Operand::IdentifierIndex(0)],
                    0,
                ),
                instr(CoreOpcode::Return, vec![reg(LOCAL0)], 1),
            ]
        }

        /// `f(o) { o.x = o.x + 1; return o.x }` — get_by_id + put_by_id + arith, no
        /// method call (tiers up without CallWithThis). Sites in bytecode order:
        /// GetByName@0 -> record 0, PutByName@3 -> record 1, GetByName@4 -> record 2.
        fn inc_x_instructions() -> Vec<TypedInstruction> {
            vec![
                instr(
                    CoreOpcode::GetByName,
                    vec![reg(LOCAL0), reg(ARG0), Operand::IdentifierIndex(0)],
                    0,
                ),
                instr(CoreOpcode::LoadInt32, vec![reg(LOCAL1), imm(1)], 1),
                instr(
                    CoreOpcode::AddInt32,
                    vec![reg(LOCAL2), reg(LOCAL0), reg(LOCAL1)],
                    2,
                ),
                instr(
                    CoreOpcode::PutByName,
                    vec![reg(ARG0), Operand::IdentifierIndex(0), reg(LOCAL2)],
                    3,
                ),
                instr(
                    CoreOpcode::GetByName,
                    vec![reg(LOCAL3), reg(ARG0), Operand::IdentifierIndex(0)],
                    4,
                ),
                instr(CoreOpcode::Return, vec![reg(LOCAL3)], 5),
            ]
        }

        /// Run an already-finalized property image once: seed `arg0_bits` at the
        /// `o` parameter slot on a fresh native JS stack, arm the prologue overflow
        /// check, enter via the trampoline, and return the boxed result + pending
        /// mirror. Host + code block are parked by the caller for the whole sequence.
        fn run_property_once(vm: &mut Vm, handle: &PropertyIcHandle, arg0_bits: u64) -> RunResult {
            let frame = Frame::new();
            frame.write(ARG0, arg0_bits);
            vm.set_jit_soft_stack_limit(frame.stack.stack_limit());
            let vm_ptr: *mut Vm = vm;
            let ret = handle.call_baseline_jit_entry(frame.fp + 16, vm_ptr as u64);
            let pending = vm.jit_pending_exception().0;
            RunResult { ret, pending }
        }

        /// Install the DataIC record store, park host + code block, emit + finalize.
        fn install_property_image(
            vm: &mut Vm,
            host: &mut CoreOpcodeDispatchHost,
            code_block: &CodeBlock,
        ) -> PropertyIcHandle {
            vm.set_jit_host(host); // D5: park the dispatch host.
            vm.set_jit_code_block(code_block as *const CodeBlock); // S3: park the CodeBlock.
            let jit_pending_address = vm.jit_pending_exception_address() as usize;
            let soft_stack_limit_address = vm.jit_soft_stack_limit_address() as usize;
            // Allocate the record store sized to the property-site count BEFORE emit.
            let site_count = count_property_ic_sites(code_block).expect("count property sites");
            code_block.install_baseline_jit_data(site_count);
            let image =
                emit_baseline_function(code_block, jit_pending_address, soft_stack_limit_address)
                    .expect("property function must tier up (gate admits GetByName/PutByName)");
            let mut records = image.link_records;
            finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                .expect("finalize property image")
        }

        // The DataIC FILLS on the first miss, HITS a same-structure receiver, and
        // re-fills on a different-structure receiver — native == oracle each time.
        #[test]
        fn get_by_id_native_fills_then_hits_then_refills() {
            let mut host = CoreOpcodeDispatchHost::new();
            host.jit_test_register_identifier(0, "x");

            let o1 = object_with_x(&mut host, 41);
            let o2 = object_with_x(&mut host, 99); // SAME single-prop shape as o1.
                                                   // o3: a DIFFERENT structure (a `y` property added before `x`).
            let o3 = {
                let object = host.jit_test_allocate_object();
                host.jit_test_set_own_data(
                    object,
                    &CorePropertyKey::String("y".into()),
                    JsValue::from_i32(5),
                );
                host.jit_test_set_own_data(object, &key_x(), JsValue::from_i32(7));
                object.encoded().0
            };
            let s1 = structure_of(&host, o1);
            let s2 = structure_of(&host, o2);
            let s3 = structure_of(&host, o3);
            assert_eq!(
                s1, s2,
                "same-shape siblings share one structure (HIT setup)"
            );
            assert_ne!(s1, s3, "o3's extra `y` gives it a different structure");

            let mut vm = Vm::new(VmConfig::interpreter_only());
            let code_block = build_code_block(get_x_instructions());
            let handle = install_property_image(&mut vm, &mut host, &code_block);

            // RUN 1 (o1): SENTINEL guard miss -> optimize fills the record.
            let r1 = run_property_once(&mut vm, &handle, o1);
            assert_eq!(r1.pending, 0, "no throw");
            assert_eq!(
                r1.ret,
                JsValue::from_i32(41).encoded().0,
                "native o1.x == 41"
            );
            let rec = code_block.baseline_property_ic_record(0).expect("record 0");
            assert_eq!(rec.structure_id, s1, "record filled with o1's structure");
            assert!(rec.offset >= 0, "record filled with a real offset");
            let cached_offset = rec.offset;

            // RUN 2 (o2 SAME structure): structure guard HITS -> cheap cached load.
            let r2 = run_property_once(&mut vm, &handle, o2);
            assert_eq!(
                r2.ret,
                JsValue::from_i32(99).encoded().0,
                "native o2.x == 99 (HIT)"
            );
            let rec2 = code_block.baseline_property_ic_record(0).expect("record 0");
            assert_eq!(
                rec2.structure_id, s1,
                "record unchanged after a same-structure HIT"
            );
            assert_eq!(
                rec2.offset, cached_offset,
                "cached offset unchanged after HIT"
            );

            // RUN 3 (o3 DIFFERENT structure): guard misses -> optimize re-fills.
            let r3 = run_property_once(&mut vm, &handle, o3);
            assert_eq!(
                r3.ret,
                JsValue::from_i32(7).encoded().0,
                "native o3.x == 7 (re-fill)"
            );
            let rec3 = code_block.baseline_property_ic_record(0).expect("record 0");
            assert_eq!(
                rec3.structure_id, s3,
                "record re-filled with o3's structure"
            );

            vm.clear_jit_code_block();
            vm.clear_jit_host();
        }

        // put_by_id + get_by_id round-trip: `o.x = o.x + 1; return o.x` tiers up,
        // runs native, mutates the object in place, and returns the updated value
        // across several shapes (native == oracle). The put REPLACE record fills.
        #[test]
        fn put_and_get_by_id_native_round_trips_and_fills() {
            let mut host = CoreOpcodeDispatchHost::new();
            host.jit_test_register_identifier(0, "x");

            let o1 = object_with_x(&mut host, 41);
            let o2 = object_with_x(&mut host, 99); // same structure as o1.
            let s1 = structure_of(&host, o1);

            let mut vm = Vm::new(VmConfig::interpreter_only());
            let code_block = build_code_block(inc_x_instructions());
            let handle = install_property_image(&mut vm, &mut host, &code_block);

            // RUN 1 (o1): 41 -> 42. Fills all three records (get@0, put@3, get@4).
            let r1 = run_property_once(&mut vm, &handle, o1);
            assert_eq!(r1.pending, 0, "no throw");
            assert_eq!(
                r1.ret,
                JsValue::from_i32(42).encoded().0,
                "native (o1.x=o1.x+1) == 42"
            );
            assert_eq!(
                structure_of(&host, o1),
                s1,
                "a replace put keeps the structure (no transition)",
            );
            let get0 = code_block
                .baseline_property_ic_record(0)
                .expect("get record 0");
            let put1 = code_block
                .baseline_property_ic_record(1)
                .expect("put record 1");
            let get2 = code_block
                .baseline_property_ic_record(2)
                .expect("get record 2");
            assert_eq!(get0.structure_id, s1, "get_by_id record filled");
            assert_eq!(put1.structure_id, s1, "put_by_id REPLACE record filled");
            assert_eq!(get2.structure_id, s1, "second get_by_id record filled");

            // RUN 2 (o2 same structure): all three sites HIT. 99 -> 100. The
            // returned value is `o.x` read AFTER the put, so == 100 proves the
            // native put actually stored o2.x = 100 in place.
            let r2 = run_property_once(&mut vm, &handle, o2);
            assert_eq!(
                r2.ret,
                JsValue::from_i32(100).encoded().0,
                "native (o2.x=o2.x+1) == 100 (HIT)"
            );

            vm.clear_jit_code_block();
            vm.clear_jit_host();
        }

        // `g(a) { return a.length }` on an array: `arr.length` is exotic /
        // OpaqueOrUncacheable, so the DataIC must NOT arm — the record stays SENTINEL
        // and the site always slow-paths, still returning the correct length.
        #[test]
        fn arr_length_get_by_id_stays_uncached_but_correct() {
            let mut host = CoreOpcodeDispatchHost::new();
            host.jit_test_register_identifier(0, "length");
            let array = host.jit_test_allocate_array().encoded().0;

            let mut vm = Vm::new(VmConfig::interpreter_only());
            let code_block = build_code_block(get_x_instructions()); // reads identifier 0 == "length"
            let handle = install_property_image(&mut vm, &mut host, &code_block);

            let r1 = run_property_once(&mut vm, &handle, array);
            assert_eq!(r1.pending, 0, "no throw");
            assert_eq!(
                r1.ret,
                JsValue::from_i32(0).encoded().0,
                "native empty arr.length == 0"
            );
            let rec = code_block.baseline_property_ic_record(0).expect("record 0");
            assert_eq!(
                rec.structure_id, 0,
                "arr.length record stays SENTINEL (exotic / uncacheable)",
            );

            // A second run is still correct (always slow-pathing the uncacheable site).
            let r2 = run_property_once(&mut vm, &handle, array);
            assert_eq!(
                r2.ret,
                JsValue::from_i32(0).encoded().0,
                "native arr.length == 0 again"
            );
            assert_eq!(
                code_block
                    .baseline_property_ic_record(0)
                    .unwrap()
                    .structure_id,
                0,
                "still SENTINEL after a second uncacheable slow-path",
            );

            vm.clear_jit_code_block();
            vm.clear_jit_host();
        }
    }

    // ========================================================================
    // A1.2 + A1.3 — THE FIRST JIT->JIT NATIVE CALL (the R-lever existence proof).
    //
    // Two baseline-compilable INTEGER functions run natively on ONE unified native
    // stack: `callee(a,b){ return a+b }` and `caller(x){ return callee(x,1) }`. The
    // callee is pre-compiled; its installed-image entry is pre-seeded as the caller's
    // op_call LINKED target (A1.3 first cut). Entering `caller` via the A1.1 entry
    // bridge, its emitted op_call (A1.2) lays out the callee CallFrame CONTIGUOUSLY
    // below its own and native-`bl`s the callee (replacing the `operation_call`
    // interpreter re-entry), then `op_ret` returns the boxed result.
    //
    // Asserts the R-lever header-adjacency + contiguity contract DIRECTLY off the
    // live native stack: callerFrame@0 == caller_fp, returnPC@1 == bl-site+4, the
    // callee frame sits at caller_fp - framesize, the seeded callee header/args are
    // correct, and jit_pending == 0. NO GC dependency (arith-only -> the JIT frames
    // hold only numbers, so the A1.5 scoped root-walk is not yet required).
    // macOS/aarch64 only (executes native ARM64).
    // ========================================================================
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    mod native_jit_to_jit_call {
        use super::super::{
            emit_baseline_function_with_linked_calls, LinkedCallSite, LinkedCallTarget,
        };
        use super::{build_code_block, imm, instr, reg, ARG0, ARG1, LOCAL0, LOCAL1, LOCAL2};
        use crate::bytecode::{CoreOpcode, Operand, TypedInstruction};
        use crate::interpreter::CoreOpcodeDispatchHost;
        use crate::jit::executable_allocator::{
            finalize_arm64_link_buffer, ExecutableMemoryHandle, MapJitExecutableAllocator,
        };
        use crate::value::JsValue;
        use crate::vm::jsstack::{CallFrameSlot, JsStack, Register, REGISTER_SIZE_IN_BYTES};
        use crate::vm::{Vm, VmConfig};

        /// Emit + finalize one CodeBlock into an executable image, resolving the
        /// op_call sites in `linked_calls` to the native JIT->JIT fast path.
        fn finalize_image(
            instructions: Vec<TypedInstruction>,
            jit_pending_address: usize,
            soft_stack_limit_address: usize,
            linked_calls: &[LinkedCallTarget],
        ) -> (ExecutableMemoryHandle, Vec<LinkedCallSite>) {
            let code_block = build_code_block(instructions);
            let image = emit_baseline_function_with_linked_calls(
                &code_block,
                jit_pending_address,
                soft_stack_limit_address,
                linked_calls,
            )
            .expect("emit baseline function");
            let sites = image.linked_call_sites.clone();
            let mut records = image.link_records;
            let handle =
                finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                    .expect("finalize image");
            (handle, sites)
        }

        /// Byte address of frame slot `slot_index` within the frame at `fp`.
        fn slot_addr(fp: usize, slot_index: i32) -> usize {
            (fp as isize + slot_index as isize * REGISTER_SIZE_IN_BYTES as isize) as usize
        }

        #[test]
        fn first_jit_to_jit_native_call_proves_header_adjacency_and_contiguity() {
            // A real Vm supplies the jit_pending mirror (D3) the op_call native fast
            // path probes and the `*mut Vm` the prologue pins in x19 (the callee
            // receives it via the caller's `mov x0,x19` before the `bl`). Arith-only
            // -> no slow path / throw occurs.
            let mut host = CoreOpcodeDispatchHost::new();
            let mut vm = Vm::new(VmConfig::interpreter_only());
            vm.set_jit_host(&mut host);
            let jit_pending_address = vm.jit_pending_exception_address() as usize;
            // A1.4: bake the prologue soft-limit address; the VALUE is set per-run
            // below to the current stack's low bound so the arith-only frames (well
            // above it) never trip the check.
            let soft_stack_limit_address = vm.jit_soft_stack_limit_address() as usize;

            // (1) Pre-compile the CALLEE `callee(a,b){ return a+b }`; resolve its entry.
            let callee_instructions = vec![
                instr(
                    CoreOpcode::AddInt32,
                    vec![reg(LOCAL0), reg(ARG0), reg(ARG1)],
                    0,
                ),
                instr(CoreOpcode::Return, vec![reg(LOCAL0)], 1),
            ];
            let (callee_handle, _) = finalize_image(
                callee_instructions,
                jit_pending_address,
                soft_stack_limit_address,
                &[],
            );
            let callee_entry = callee_handle.entry_address();

            // (2) Compile the CALLER `caller(x){ return callee(x, 1) }`, pre-seeding
            //     its op_call (bci 1) as the LINKED monomorphic target = callee entry.
            //     LOCAL2 is the callee operand slot (undefined; dispatch uses the
            //     resolved entry). arg0 = x (ARG0), arg1 = 1 (LOCAL1, from LoadInt32).
            let caller_instructions = vec![
                instr(CoreOpcode::LoadInt32, vec![reg(LOCAL1), imm(1)], 0),
                instr(
                    CoreOpcode::Call,
                    vec![
                        reg(LOCAL0),
                        reg(LOCAL2),
                        Operand::UnsignedImmediate(2),
                        reg(ARG0),
                        reg(LOCAL1),
                    ],
                    1,
                ),
                instr(CoreOpcode::Return, vec![reg(LOCAL0)], 2),
            ];
            let linked = [LinkedCallTarget {
                bytecode_index: 1,
                entry: callee_entry,
            }];
            let (caller_handle, sites) = finalize_image(
                caller_instructions,
                jit_pending_address,
                soft_stack_limit_address,
                &linked,
            );
            assert_eq!(sites.len(), 1, "the caller emitted one native call edge");
            let site = sites[0];
            assert_eq!(site.bytecode_index, 1, "the native edge is op_call@bci 1");
            let caller_base = caller_handle.entry_address();

            let undefined_bits = JsValue::undefined().encoded().0;

            for &x in &[0_i32, 1, 7, -4, 1000, 123_456] {
                // A fresh native JS stack per run. `caller_fp` near the high end:
                // positive arg/header slots above it, frames descend below it.
                let stack = JsStack::new(1 << 20).expect("native js stack");
                let caller_fp = stack.high_address() - 256;
                // A1.4: arm the prologue soft-limit check with THIS stack's low bound
                // (both caller + callee frames sit near the high end, far above it, so
                // neither prologue trips). Set before entry (the callee runs on the
                // same stack via the native bl).
                vm.set_jit_soft_stack_limit(stack.stack_limit());
                // Seed the caller's argument x at ARG0 (untouched by op_enter).
                assert!(
                    stack.write_slot(
                        slot_addr(caller_fp, ARG0),
                        Register::from_bits(JsValue::from_i32(x).encoded().0),
                    ),
                    "ARG0 slot in range",
                );

                // Enter `caller` via the A1.1 entry bridge: sp = caller_fp + 16.
                let vm_ptr: *mut Vm = &mut vm;
                let ret = caller_handle.call_baseline_jit_entry(caller_fp + 16, vm_ptr as u64);

                // The callee frame the op_call laid out, at caller_fp + offset (<0).
                let callee_fp =
                    (caller_fp as isize + site.callee_frame_offset_from_fp as isize) as usize;
                let read = |slot_index: i32| -> u64 {
                    stack
                        .read_slot(slot_addr(callee_fp, slot_index))
                        .expect("callee frame slot in range")
                        .bits()
                };

                // --- The R-lever contract -----------------------------------------
                // Functional: caller(x) == callee(x, 1) == x + 1.
                assert_eq!(
                    ret,
                    JsValue::from_i32(x + 1).encoded().0,
                    "caller({x}) native-called callee -> x+1",
                );
                // CONTIGUITY: the callee frame sits strictly below the caller frame.
                assert!(
                    callee_fp < caller_fp,
                    "callee frame ({callee_fp:#x}) is below caller_fp ({caller_fp:#x})",
                );
                // HEADER ADJACENCY: callerFrame@0 == caller_fp (the callee prologue's
                // pushPair stored the caller's fp at the callee frame's slot 0).
                assert_eq!(
                    read(CallFrameSlot::CALLER_FRAME),
                    caller_fp as u64,
                    "callerFrame@0 == caller_fp",
                );
                // HEADER ADJACENCY: returnPC@1 == the instruction after the bl
                // (bl-site + 4), a real return address into the caller image.
                assert_eq!(
                    read(CallFrameSlot::RETURN_PC),
                    (caller_base + site.return_pc_offset) as u64,
                    "returnPC@1 == bl-site + 4",
                );
                // The caller laid out the callee header + args at the callee frame.
                assert_eq!(
                    read(CallFrameSlot::CALLEE),
                    undefined_bits,
                    "callee slot = the boxed callee operand (undefined here)",
                );
                assert_eq!(
                    read(CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS),
                    3,
                    "argumentCountIncludingThis == argc(2) + this",
                );
                assert_eq!(
                    read(CallFrameSlot::THIS_ARGUMENT),
                    undefined_bits,
                    "this == undefined (op_call's implicit receiver)",
                );
                assert_eq!(
                    read(CallFrameSlot::FIRST_ARGUMENT),
                    JsValue::from_i32(x).encoded().0,
                    "arg0 == x",
                );
                assert_eq!(
                    read(CallFrameSlot::FIRST_ARGUMENT + 1),
                    JsValue::from_i32(1).encoded().0,
                    "arg1 == 1",
                );
                // NO throw: the jit_pending mirror is clean (proves the post-call
                // exception probe saw 0; native sp was restored so the epilogue read
                // the right spills and returned cleanly to the host trampoline).
                assert_eq!(
                    vm.jit_pending_exception().0,
                    0,
                    "jit_pending == 0 (no throw)",
                );
            }

            vm.clear_jit_host();
        }
    }
}
