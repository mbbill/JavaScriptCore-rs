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
    operation_compare_greater, operation_compare_less, operation_compare_lesseq, operation_jfalse,
};
use crate::value::{JsValue, NUMBER_TAG};

use super::arith::{emit_arith_fast_path, ArithFamilyOp, PINNED_VM_GPR};

/// `sizeof(Register)` (JSVALUE64); `addressFor` scales the VirtualRegister by it.
const REGISTER_SIZE_BYTES: i32 = 8;

// --- Register identity (GPRInfo.h, ARM64 baseline) — the canonical conventions
//     op_add/arith established, kept identical so this emitter does not drift. ----

/// `x29 == cfr` (GPRInfo.h:582; AssemblyHelpers.h:1290-1298).
const CALL_FRAME_GPR: RegisterID = RegisterID::Fp;
/// `x30 == lr`. Saved/restored around the operation far-calls.
const LINK_GPR: RegisterID = RegisterID::Lr;
/// The frame base arrives in raw C-ABI `x1` (`entry_prologue.rs`); the prologue
/// moves it into `cfr`.
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

/// `AssemblyHelpers::addressFor(VirtualRegister) = Address(x29, vreg.offset()*8)`
/// (AssemblyHelpers.h:1290-1298). The operand IS the VirtualRegister's raw value.
fn address_for(operand: i32) -> Address {
    Address::new(CALL_FRAME_GPR, operand.wrapping_mul(REGISTER_SIZE_BYTES))
}

/// An emitted (not-yet-finalized) full-function image — the assembler bytes plus
/// the branch link records the LinkBuffer pass resolves. Mirrors op_add's
/// `OpAddImage` (the `(code, jumpsToLink)` a `JIT`/`LinkBuffer` carries before
/// `finalizeCodeWithoutDisassembly`).
pub(crate) struct FunctionImage {
    pub(crate) code: Vec<u8>,
    pub(crate) link_records: Vec<Arm64LinkRecord>,
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
        }
    }

    /// `emitFunctionPrologue` for the raw return-seed C-ABI lane (frame base in x1),
    /// plus the callee-saved spills this emitter clobbers — byte-identical to
    /// op_add/arith's prologue.
    fn emit_prologue(&mut self) {
        self.h.masm_mut().push_pair(CALL_FRAME_GPR, LINK_GPR); // stp fp,lr,[sp,#-16]!
        self.h.masm_mut().move_rr(RAW_FRAME_ARG_GPR, CALL_FRAME_GPR); // mov fp, x1 (cfr)
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

    /// The shared epilogue (`done`): refill the callee-saved spills and `ret`. x0
    /// (the return value / op_ret result) is preserved across the pops.
    fn emit_epilogue(&mut self) -> Label {
        let done = self.h.masm().label();
        self.h
            .masm_mut()
            .pop_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // refill x27/x28
        self.h
            .masm_mut()
            .pop_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // refill x19/x20
        self.h.masm_mut().pop_pair(CALL_FRAME_GPR, LINK_GPR); // restore caller fp/lr
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
pub(crate) fn emit_baseline_function(
    code_block: &CodeBlock,
    jit_pending_address: usize,
) -> Result<FunctionImage, EmitFunctionError> {
    let count = code_block.unlinked().instructions().instruction_count();
    if count == 0 {
        return Err(EmitFunctionError::EmptyFunction);
    }

    let num_locals = count_callee_locals(code_block)?;
    let mut emitter = FunctionEmitter::new(count, jit_pending_address);

    // === PROLOGUE + op_enter ===============================================
    emitter.emit_prologue();
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

        // Prologue (cross-checked against op_add's proven prologue).
        assert_eq!(w[0], 0xa9bf_7bfd, "stp fp,lr,[sp,#-16]!");
        assert_eq!(w[1], 0xaa01_03fd, "mov fp, x1 (cfr)");
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
                // A roomy immovable backing window: fp sits mid-window so positive
                // argument slots (above fp) and negative local slots (below fp) are
                // both in range.
                let stack = JsStack::with_test_backing(64);
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
            let ret = handle.call_finalized_binary_u64(vm_ptr as u64, frame.fp as u64);

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
    }
}
