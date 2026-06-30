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
    operation_call, operation_compare_greater, operation_compare_less, operation_compare_lesseq,
    operation_get_by_val, operation_jfalse, operation_put_by_val, MAX_REGISTER_CALL_ARGS,
};
use crate::value::{JsValue, NUMBER_TAG};
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
    /// A1.2/A1.3: the native JIT->JIT call sites this image emitted (one per
    /// op_call that resolved to a [`LinkedCallTarget`]). Empty for the live install
    /// path (every dynamic op_call stays on the `operation_call` slow path). Carries
    /// the call edge's `returnPC`/calleeFrame geometry for the R-lever proof.
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
    /// Bytes the prologue reserved below `fp` for this function's callee locals
    /// (`reserved_locals_bytes(num_locals)`), captured in [`Self::emit_prologue`].
    /// The native op_call edge (A1.2) needs it to place the callee frame BELOW the
    /// caller's whole used region and to restore `sp` after the call.
    reserved_locals_bytes: i32,
    /// A1.2/A1.3: native JIT->JIT call sites emitted in this image (one per op_call
    /// resolved to a [`LinkedCallTarget`]); surfaced on the [`FunctionImage`].
    linked_call_sites: Vec<LinkedCallSite>,
}

const MODE: TagRegistersMode = TagRegistersMode::HaveTagRegisters;

impl FunctionEmitter {
    fn new(instruction_count: usize, jit_pending_address: usize) -> Self {
        Self {
            h: AssemblyHelpers::new(),
            labels: vec![None; instruction_count],
            jumps: Vec::new(),
            done_jumps: Vec::new(),
            exception_jumps: Vec::new(),
            slow: Vec::new(),
            label_link_records: Vec::new(),
            jit_pending_address,
            reserved_locals_bytes: 0,
            linked_call_sites: Vec::new(),
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
        // D3 throw edge: the callee threw (or a non-callable callee's TypeError)
        // stamps the mirror; branch to the shared exception stub BEFORE storing the
        // empty result. The probe reuses SCRATCH_GPR; x0 (the boxed result) is intact.
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst)); // dst = x0 (boxed result)
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
    /// ⚠ A1.5 DEPENDENCY — CELL-CARRYING CALLS ARE GATED OUT. A JIT CallFrame on the
    /// native stack holding a CELL (object/string) across a collection would not be
    /// rooted: the scoped conservative scan of the native-stack JIT-frame span is
    /// A1.5 (the UNIFIED FRAME CONTRACT, jsstack.md), NOT YET landed. So this fast
    /// path is engaged ONLY for a pre-seeded [`LinkedCallTarget`] (the arith/int
    /// proof), NEVER for a dynamic op_call — the live install path supplies NO linked
    /// targets, so every cell-carrying op_call (the `vm_op_call_b5_*` cases) stays on
    /// the `operation_call` slow path ([`Self::emit_op_call`]). Broadly routing real
    /// op_calls here is UNSAFE until A1.5.
    ///
    /// DIVERGENCE from JSC (A1.3 first cut, documented on [`LinkedCallTarget`]): the
    /// callee entry is an ABSOLUTE `blr` immediate baked at emit time, not a runtime
    /// load of a patchable `CallLinkInfo::m_monomorphicCallDestination`; the data-IC
    /// link-on-first-exec + callee-identity guard + arity stub are deferred. The
    /// callee-frame `codeBlock`@2 slot is left unwritten (the callee's emitted image
    /// never consults it; it is needed only when a callee slow path re-enters the
    /// interpreter — a follow-up alongside A1.5).
    fn emit_op_call_native_linked(
        &mut self,
        dst: i32,
        callee: i32,
        args: &[i32],
        entry: usize,
        bytecode_index: usize,
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
        // Initialize `this` = undefined (op_call's implicit receiver, the same value
        // `operation_call` supplies on the slow path).
        let undefined_bits = JsValue::undefined().encoded().0;
        self.h
            .masm_mut()
            .move_imm64(TrustedImm64::new(undefined_bits as i64), SCRATCH_GPR);
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
        // The linked near call (`jit.call(callTargetGPR, ...)`, CallLinkInfo.cpp:363),
        // here an ABSOLUTE `blr` to the resolved callee entry (A1.3 first cut).
        self.h.masm_mut().far_call(TrustedImm64::new(entry as i64));
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
        // D3 throw edge (a callee slow path stamped the mirror), then store x0 -> dst.
        self.emit_exception_probe();
        self.h.masm_mut().store64(RESULT_GPR, address_for(dst));
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
///
/// Returns `Err` (REJECTS the function) for any opcode/operand outside the Stage-1
/// int32/control-flow allowlist — the S4 gate behavior (the function stays in the
/// interpreter rather than being mis-lowered).
///
/// Every op_call is lowered to the `operation_call` slow path: the LIVE install path
/// supplies NO linked targets, so no dynamic (possibly cell-carrying) op_call is
/// routed through the A1.2 native fast path until the A1.5 JIT-frame GC scan lands
/// (see [`FunctionEmitter::emit_op_call_native_linked`]).
pub(crate) fn emit_baseline_function(
    code_block: &CodeBlock,
    jit_pending_address: usize,
) -> Result<FunctionImage, EmitFunctionError> {
    emit_baseline_function_with_linked_calls(code_block, jit_pending_address, &[])
}

/// As [`emit_baseline_function`], but resolves the op_call sites named in
/// `linked_calls` to the A1.2 native JIT->JIT fast path (a direct `blr` to the
/// callee's installed-image entry) instead of the `operation_call` slow path. An
/// op_call NOT named here keeps the slow path. This is the A1.3 first-cut linker:
/// the proof harness pre-seeds ONE monomorphic target; the live path passes `&[]`.
pub(crate) fn emit_baseline_function_with_linked_calls(
    code_block: &CodeBlock,
    jit_pending_address: usize,
    linked_calls: &[LinkedCallTarget],
) -> Result<FunctionImage, EmitFunctionError> {
    let count = code_block.unlinked().instructions().instruction_count();
    if count == 0 {
        return Err(EmitFunctionError::EmptyFunction);
    }

    let num_locals = count_callee_locals(code_block)?;
    let mut emitter = FunctionEmitter::new(count, jit_pending_address);

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
                // A1.2/A1.3: a pre-seeded LINKED target routes this site through the
                // native JIT->JIT fast path; otherwise (always, for the live install
                // path) the `operation_call` slow path. The native path is gated to
                // pre-seeded targets because cell-carrying calls need the A1.5
                // JIT-frame GC scan (see `emit_op_call_native_linked`).
                match linked_calls.iter().find(|t| t.bytecode_index == bci) {
                    Some(target) => {
                        emitter.emit_op_call_native_linked(dst, callee, &args, target.entry, bci)
                    }
                    None => emitter.emit_op_call(dst, callee, &args),
                }
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
        let mut prologue = FunctionEmitter::new(1, 0);
        prologue.emit_prologue(0);
        assert_eq!(
            &prologue.h.code()[0..8],
            ARM64_JSC_BASELINE_GENERATED_PROLOGUE_BYTES,
            "prologue head must be the faithful `stp fp,lr,[sp,#-16]!; mov fp,sp`",
        );

        // The epilogue ends with `mov sp,fp; ldp fp,lr,[sp],#16; ret` (12 bytes)
        // after the callee-save refills.
        let mut epilogue = FunctionEmitter::new(1, 0);
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
        let image = emit_baseline_function(&code_block, 0x1000).expect("emit loop");
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
        assert_eq!(image.link_records.len(), 31, "every branch is linked");
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
            emit_baseline_function(&code_block, 0x1000),
            Err(EmitFunctionError::ConstantOperand)
        ));

        let empty = build_code_block(Vec::new());
        assert!(matches!(
            emit_baseline_function(&empty, 0x1000),
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
                emit_baseline_function(&live_after, 0x1000),
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
            emit_baseline_function(&dead_after, 0x1000).is_ok(),
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
            emit_baseline_function(&bad_jump, 0x1000),
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
            emit_baseline_function(&code_block, 0x1000).is_ok(),
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
        let image = emit_baseline_function(&code_block, 0x1000)
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
        fn run_function(
            instructions: Vec<TypedInstruction>,
            args: &[(i32, u64)],
        ) -> (RunResult, Frame) {
            let mut host = CoreOpcodeDispatchHost::new();
            let mut vm = Vm::new(VmConfig::interpreter_only());
            vm.set_jit_host(&mut host); // D5: park the host before entering JIT code.
            let jit_pending_address = vm.jit_pending_exception_address() as usize;

            let frame = Frame::new();
            for &(operand, bits) in args {
                frame.write(operand, bits);
            }

            let code_block = build_code_block(instructions);
            let image =
                emit_baseline_function(&code_block, jit_pending_address).expect("emit function");
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
            linked_calls: &[LinkedCallTarget],
        ) -> (ExecutableMemoryHandle, Vec<LinkedCallSite>) {
            let code_block = build_code_block(instructions);
            let image = emit_baseline_function_with_linked_calls(
                &code_block,
                jit_pending_address,
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

            // (1) Pre-compile the CALLEE `callee(a,b){ return a+b }`; resolve its entry.
            let callee_instructions = vec![
                instr(
                    CoreOpcode::AddInt32,
                    vec![reg(LOCAL0), reg(ARG0), reg(ARG1)],
                    0,
                ),
                instr(CoreOpcode::Return, vec![reg(LOCAL0)], 1),
            ];
            let (callee_handle, _) = finalize_image(callee_instructions, jit_pending_address, &[]);
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
            let (caller_handle, sites) =
                finalize_image(caller_instructions, jit_pending_address, &linked);
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
