//! ARM64 baseline int32 ARITH FAMILY lowering — `op_sub` / `op_mul` /
//! `op_bitand` / `op_bitor` / `op_bitxor` / `op_lshift` / `op_rshift`, templated
//! from the verified `op_add` lowering ([`super::op_add`]). Each emits a
//! standalone callable image whose int32 fast path runs entirely in registers and
//! whose slow path far-calls the matching `operationValue*` shim — the SAME
//! emit -> relocate -> EXECUTE proof op_add carries, generalized over the family.
//!
//! C++ JSC map (source of truth):
//! - `jit/JITArithmetic.cpp` `emit_op_sub` (:1099) / `emit_op_mul` (:1083) /
//!   `emit_op_bitand` (:723) / `emit_op_bitor` (:728) / `emit_op_bitxor` (:733) /
//!   `emit_op_lshift` (:738) / `emit_op_rshift` (:782): load both operands, run
//!   the per-op generator inline, `addSlowCase`, then `emitPutVirtualRegister`.
//!   The matching `emitSlow_op_*` calls `operationValue*` and stores its result.
//! - The per-op int32 generators (the variable fast path), JSVALUE64 paths:
//!   - `jit/JITSubGenerator.cpp` `generateInline`/`generateFastPath`: ARM64 path
//!     `branchSub32(Overflow, m_left, m_right, scratch)` — left FIRST, right
//!     SECOND (computes `left - right`), NOT the x86 `move+branchSub32` form.
//!   - `jit/JITMulGenerator.cpp` `generateInline`: `branchMul32(Overflow, m_right,
//!     m_left, scratch)` then `branchTest32(Zero, scratch)` — the negative-zero
//!     guard ("Go slow if potential negative zero"), ported faithfully.
//!   - `jit/JITBitAndGenerator.cpp` (JSVALUE64 intVar&intVar): `and64(left, right,
//!     scratch)` then a SINGLE `branchIfNotInt32(scratch)` — the boxed operands
//!     carry `NumberTag` in their high bits, so `&` of two int32s stays a boxed
//!     int32 and the one guard rejects any non-int32 operand. No per-operand guard,
//!     no separate box.
//!   - `jit/JITBitOrGenerator.cpp` (JSVALUE64): `branchIfNotInt32(left)`,
//!     `branchIfNotInt32(right)`, `or64(right, left, result)` — `|` likewise
//!     preserves the tag; no box.
//!   - `jit/JITBitXorGenerator.cpp` (JSVALUE64): guards, then `xor32(right, left,
//!     result)` + `boxInt32` — `^` CANCELS the tag bits, so a 32-bit xor + re-box.
//!   - `jit/JITLeftShiftGenerator.cpp` (JSVALUE64 intVar<<intVar): guard RIGHT
//!     first, then LEFT, `lshift32(left, right, result)` + `boxInt32` (the shift
//!     amount is masked to 5 bits by `lslv`).
//!   - `jit/JITRightShiftGenerator.cpp` (JSVALUE64 intVar>>intVar, SignedShift):
//!     guard RIGHT first, then LEFT, `rshift32(left, right, result)` + `boxInt32`.
//! - `jit/JITOperations.cpp` `operationValue{Sub,Mul,BitAnd,BitOr,BitXor,LShift,
//!   RShift}` (:5225/:4978/...) -> the runtime shims
//!   [`crate::jit::operations`]`::operation_value_*` (the shared D1/D5 bridge).
//!
//! CANONICAL REGISTER CONVENTIONS (propagated from op_add; the verifier required
//! they not drift): `leftRegs == argumentGPR1` (x1), `rightRegs == argumentGPR2`
//! (x2), `resultRegs == returnValueGPR` (x0). Operands are pre-placed in the
//! operation's arg slots so the slow path needs ZERO operand moves. The pinned VM
//! lives in [`PINNED_VM_GPR`] (x19) — the SHARED const op_add also references.
//!
//! Unsafe boundary: this module is entirely SAFE (`jit/mod.rs` is
//! `#![deny(unsafe_code)]`). It only COMPUTES instruction bytes; the execution
//! test composes the same three already-audited unsafe islands op_add does (W^X
//! executable call, the runtime-shim reborrows, the JSStack provenance gate). The
//! family lowering itself introduces NO new `unsafe`.
//!
//! SCOPE — INT32/PRIMITIVE only, the same three forward prerequisites op_add
//! documents (object operands need the real `CodeBlock`; a real `TypeError` cell;
//! a pin-stable `Vm`). DEFERRED, flagged at their sites: `op_urshift` entirely
//! (its double-result fallback when the unsigned result exceeds `i32::MAX` is
//! unported) and the inline double-operand fast paths of the sub/mul/shift
//! generators (a non-int32 operand instead takes the faithful slow path, which
//! returns the correct double/int via `arithmetic_binary_result`).

#![allow(dead_code)]

use crate::assembler::labels::Jump;
use crate::assembler::link_records::Arm64LinkRecord;
use crate::assembler::macro_assembler_arm64::ResultCondition;
use crate::assembler::operands::{Address, TrustedImm64};
use crate::assembler::registers::RegisterID;
use crate::jit::assembly_helpers::{AssemblyHelpers, TagRegistersMode};
use crate::jit::operations::{
    operation_value_bitand, operation_value_bitor, operation_value_bitxor, operation_value_lshift,
    operation_value_mul, operation_value_rshift, operation_value_sub,
};

/// `sizeof(Register)` (JSVALUE64); `addressFor` scales the VirtualRegister by it.
const REGISTER_SIZE_BYTES: i32 = 8;

// --- Register identity (GPRInfo.h, ARM64 baseline) — the canonical conventions
//     op_add established, kept identical so the family does not drift. ----------

/// `x29 == cfr` (GPRInfo.h:582; AssemblyHelpers.h:1290-1298): the frame pointer
/// `addressFor(vreg)` is relative to.
const CALL_FRAME_GPR: RegisterID = RegisterID::Fp;
/// `x30 == lr`. Saved/restored around the operation far-call.
const LINK_GPR: RegisterID = RegisterID::Lr;

/// The frame base arrives in raw C-ABI `x1` (`entry_prologue.rs`); the prologue
/// moves it into `cfr`, then `x1` is reused as `leftRegs`.
const RAW_FRAME_ARG_GPR: RegisterID = RegisterID::X1;
/// The `*mut Vm` arrives in raw C-ABI `x0` and is the operation shim's arg0.
const RAW_VM_ARG_GPR: RegisterID = RegisterID::X0;

/// `leftRegs == argumentGPR1` (x1): op1 through the fast path, reused as arg1.
const LEFT_GPR: RegisterID = RegisterID::X1;
/// `rightRegs == argumentGPR2` (x2): op2, reused as arg2.
const RIGHT_GPR: RegisterID = RegisterID::X2;
/// `resultRegs == returnValueGPR` (x0): the scratch/result; in the slow path also
/// the operation's arg0 (the Vm) and its return value.
const RESULT_GPR: RegisterID = RegisterID::X0;

/// The canonical pinned-VM carrier (jit-runtime-bridge.md D2c), SHARED by the whole
/// int32 arith family (`op_add` imports this exact const). JSC's baseline reaches
/// the VM via `globalObject->vm()` and has no pinned VM register; this port reserves
/// one (`BASELINE_PINNED_VM_REGISTER`, jit/abi.rs:573) and concretizes it here as a
/// CALLEE-SAVED GPR (x19 / regCS0) so the `*mut Vm` survives the operation far-call
/// (AAPCS64 callees preserve x19-x28). The prologue spills/refills it so the emitted
/// function also honors AAPCS64 toward its own caller. Made a single shared const so
/// no family member can silently pick a different physical register.
pub(crate) const PINNED_VM_GPR: RegisterID = RegisterID::X19;
/// Paired with `PINNED_VM_GPR` only to keep `stp`/`ldp` 16-byte aligned; unused.
const PINNED_VM_PAIR_GPR: RegisterID = RegisterID::X20;

/// `numberTagRegister` (x27) / `notCellMaskRegister` (x28), the `HaveTagRegisters`
/// pair the int32 guards and box rely on. Spilled in the prologue, refilled in the
/// epilogue.
const NUMBER_TAG_GPR: RegisterID = AssemblyHelpers::NUMBER_TAG_REGISTER;
const NOT_CELL_MASK_GPR: RegisterID = AssemblyHelpers::NOT_CELL_MASK_REGISTER;

/// Caller-saved scratch for the post-call `branchTest` exception-word probe.
const EXC_ADDR_GPR: RegisterID = RegisterID::X3;

/// The int32 arith FAMILY templated here (op_add lives in its own module as the
/// original; it is the `Add` member of this same family).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArithFamilyOp {
    Sub,
    Mul,
    BitAnd,
    BitOr,
    BitXor,
    LShift,
    RShift,
}

impl ArithFamilyOp {
    /// The `operationValue*` shim address this op's slow path far-calls.
    fn operation_shim(self) -> usize {
        match self {
            ArithFamilyOp::Sub => operation_value_sub as usize,
            ArithFamilyOp::Mul => operation_value_mul as usize,
            ArithFamilyOp::BitAnd => operation_value_bitand as usize,
            ArithFamilyOp::BitOr => operation_value_bitor as usize,
            ArithFamilyOp::BitXor => operation_value_bitxor as usize,
            ArithFamilyOp::LShift => operation_value_lshift as usize,
            ArithFamilyOp::RShift => operation_value_rshift as usize,
        }
    }
}

/// `AssemblyHelpers::addressFor(VirtualRegister) = Address(x29, vreg.offset()*8)`
/// (AssemblyHelpers.h:1290-1298).
fn address_for(operand: i32) -> Address {
    Address::new(CALL_FRAME_GPR, operand.wrapping_mul(REGISTER_SIZE_BYTES))
}

/// An emitted (not-yet-finalized) family image — the assembler bytes plus the
/// forward-branch link records the LinkBuffer pass resolves. Mirrors op_add's
/// `OpAddImage` (the `(code, jumpsToLink)` a `JIT`/`LinkBuffer` carries before
/// `finalizeCodeWithoutDisassembly`).
pub(crate) struct ArithImage {
    pub(crate) code: Vec<u8>,
    pub(crate) link_records: Vec<Arm64LinkRecord>,
}

/// The per-op fast path's forward jumps: the int32-guard/overflow/neg-zero exits
/// (all targeting the shared slow label) and the terminal `jump()` to `done`.
struct FastPathJumps {
    slow: Vec<Jump>,
    fast_to_done: Jump,
}

/// Emit ONE family op's int32 fast path (the per-op generator), faithful to the
/// JSVALUE64 path of the matching `JIT*Generator`. Operands are already loaded in
/// `LEFT_GPR`/`RIGHT_GPR`; the result is boxed into `RESULT_GPR` and stored to
/// `dst`. Returns the slow-path jumps + the fast->done jump for the caller to link.
fn emit_fast_path(
    h: &mut AssemblyHelpers,
    op: ArithFamilyOp,
    dst: i32,
    mode: TagRegistersMode,
) -> FastPathJumps {
    let mut slow = Vec::new();
    match op {
        // JITSubGenerator (ARM64): guards left, right; branchSub32(Overflow, LEFT,
        // RIGHT, RESULT) => RESULT = left - right (operand order is load-bearing —
        // `sub` is non-commutative); box.
        ArithFamilyOp::Sub => {
            slow.push(h.branch_if_not_int32(LEFT_GPR, mode));
            slow.push(h.branch_if_not_int32(RIGHT_GPR, mode));
            slow.push(h.masm_mut().branch_sub32(
                ResultCondition::Overflow,
                LEFT_GPR,
                RIGHT_GPR,
                RESULT_GPR,
            ));
            h.box_int32(RESULT_GPR, RESULT_GPR, mode);
        }
        // JITMulGenerator: guards left, right; branchMul32(Overflow, RIGHT, LEFT,
        // RESULT) (smull, commutative); branchTest32(Zero, RESULT) -> slow (the
        // negative-zero guard, faithfully ported); box.
        ArithFamilyOp::Mul => {
            slow.push(h.branch_if_not_int32(LEFT_GPR, mode));
            slow.push(h.branch_if_not_int32(RIGHT_GPR, mode));
            slow.push(h.masm_mut().branch_mul32(
                ResultCondition::Overflow,
                RIGHT_GPR,
                LEFT_GPR,
                RESULT_GPR,
            ));
            // "Go slow if potential negative zero" (JITMulGenerator.cpp): an int32
            // product of 0 may semantically be -0 (a double), which only the slow
            // path can materialize. NOTE: the slow-path evaluator's int*int=0 path
            // currently yields int32 0 (the -0 double materialization is an
            // evaluator concern, not this lowering's) — the guard's PRESENCE is
            // proven by the byte oracle + the extra link record below.
            slow.push(
                h.masm_mut()
                    .branch_test32(ResultCondition::Zero, RESULT_GPR, RESULT_GPR),
            );
            h.box_int32(RESULT_GPR, RESULT_GPR, mode);
        }
        // JITBitAndGenerator (JSVALUE64 intVar&intVar): and64 of the two BOXED
        // operands keeps `NumberTag` in the high bits when both are int32, so the
        // result is already a boxed int32; a SINGLE branchIfNotInt32(RESULT) rejects
        // any non-int32 operand. JSC's `move(scratch, result)` is folded away because
        // RESULT already IS the scratch. No per-operand guard, no separate box.
        ArithFamilyOp::BitAnd => {
            h.masm_mut().and64(LEFT_GPR, RIGHT_GPR, RESULT_GPR);
            slow.push(h.branch_if_not_int32(RESULT_GPR, mode));
        }
        // JITBitOrGenerator (JSVALUE64): guards left, right; or64(RIGHT, LEFT,
        // RESULT) — `|` preserves the tag (both operands carry NumberTag), so the
        // result is already boxed; no box.
        ArithFamilyOp::BitOr => {
            slow.push(h.branch_if_not_int32(LEFT_GPR, mode));
            slow.push(h.branch_if_not_int32(RIGHT_GPR, mode));
            h.masm_mut().or64(RIGHT_GPR, LEFT_GPR, RESULT_GPR);
        }
        // JITBitXorGenerator (JSVALUE64): guards left, right; xor32(RIGHT, LEFT,
        // RESULT) on the low 32 bits — `^` CANCELS the NumberTag bits, so a 32-bit
        // xor + re-box is required (not xor64).
        ArithFamilyOp::BitXor => {
            slow.push(h.branch_if_not_int32(LEFT_GPR, mode));
            slow.push(h.branch_if_not_int32(RIGHT_GPR, mode));
            h.masm_mut().xor32(RIGHT_GPR, LEFT_GPR, RESULT_GPR);
            h.box_int32(RESULT_GPR, RESULT_GPR, mode);
        }
        // JITLeftShiftGenerator (JSVALUE64 intVar<<intVar): guard RIGHT first, then
        // LEFT (JSC's order for shifts); lshift32(LEFT, RIGHT, RESULT) — `lslv`
        // masks the shift amount to 5 bits, matching JS `a << (b & 31)`; box.
        ArithFamilyOp::LShift => {
            slow.push(h.branch_if_not_int32(RIGHT_GPR, mode));
            slow.push(h.branch_if_not_int32(LEFT_GPR, mode));
            h.masm_mut().lshift32(LEFT_GPR, RIGHT_GPR, RESULT_GPR);
            h.box_int32(RESULT_GPR, RESULT_GPR, mode);
        }
        // JITRightShiftGenerator (JSVALUE64 intVar>>intVar, SignedShift): guard RIGHT
        // first, then LEFT; rshift32 = `asrv` (arithmetic, masked to 5 bits); box.
        // DEFERRED: the inline doubleVar>>intVar fast path (branchTruncate /
        // convertDoubleToInt32) — a double left operand takes the slow path, which
        // returns the faithful int32 via bitwise_binary_result(number_to_int32(..)).
        ArithFamilyOp::RShift => {
            slow.push(h.branch_if_not_int32(RIGHT_GPR, mode));
            slow.push(h.branch_if_not_int32(LEFT_GPR, mode));
            h.masm_mut().rshift32(LEFT_GPR, RIGHT_GPR, RESULT_GPR);
            h.box_int32(RESULT_GPR, RESULT_GPR, mode);
        }
    }
    // emitPutVirtualRegister(result) (fast-path store) + endJumpList.append(jump()).
    h.masm_mut().store64(RESULT_GPR, address_for(dst));
    let fast_to_done = h.masm_mut().jump();
    FastPathJumps { slow, fast_to_done }
}

/// Emit the baseline lowering for `dst = op1 <op> op2` (all `VirtualRegister` raw
/// operands) as a standalone callable image, templated from
/// `emit_baseline_op_add_int32`. The result executes as
/// `extern "C" fn(vm: *mut Vm, cfr: u64) -> u64`: arg0 = the `*mut Vm` for the
/// operation slow path, arg1 = the frame base (`cfr`).
///
/// `jit_pending_address` is the baked `AbsoluteAddress` of the `Vm`'s `m_exception`
/// mirror word (`Vm::jit_pending_exception_address`, D3); it must remain stable for
/// the call's duration (a pin-stable `Vm` — op_add forward prereq #3).
pub(crate) fn emit_baseline_arith_int32(
    op: ArithFamilyOp,
    op1: i32,
    op2: i32,
    dst: i32,
    jit_pending_address: usize,
) -> ArithImage {
    let mut h = AssemblyHelpers::new();
    let mode = TagRegistersMode::HaveTagRegisters;

    // === PROLOGUE (identical to op_add) =====================================
    h.masm_mut().push_pair(CALL_FRAME_GPR, LINK_GPR); // stp fp,lr,[sp,#-16]!
    h.masm_mut().move_rr(RAW_FRAME_ARG_GPR, CALL_FRAME_GPR); // mov fp, x1 (cfr)
    h.masm_mut().push_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // spill x19 (+x20 pad)
    h.masm_mut().push_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // spill tag pair x27/x28
    h.masm_mut().move_rr(RAW_VM_ARG_GPR, PINNED_VM_GPR); // pinned-VM := *mut Vm (x0)
    h.emit_materialize_tag_check_registers(); // x27 = NumberTag, x28 = NotCellMask

    // === FAST PATH (the per-op generator) ===================================
    // emitGetVirtualRegister(op1, leftRegs) / (op2, rightRegs).
    h.masm_mut().load64(address_for(op1), LEFT_GPR);
    h.masm_mut().load64(address_for(op2), RIGHT_GPR);
    let fast = emit_fast_path(&mut h, op, dst, mode);

    // === SLOW PATH (emitSlow_op_* / emitMathICSlow) =========================
    let slow_label = h.masm().label();
    // arg0 := the pinned `*mut Vm` (jit-runtime-bridge.md D2c). left (x1)/right (x2)
    // are STILL the original operands (the fast-path op wrote only x0), so — exactly
    // like JSC's MathIC slow path — they pass straight through as arg1/arg2.
    h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // mov x0, x19
                                                         //
                                                         // FORWARD PREREQ #1 (shared with op_add): an OBJECT operand reaching the
                                                         // operation needs the real active-frame CodeBlock; `operation_value_*` uses a
                                                         // placeholder CodeBlock, sound ONLY for the primitive paths exercised here.
    let _call = h
        .masm_mut()
        .far_call(TrustedImm64::new(op.operation_shim() as i64));
    // branchTestPtr(NonZero, AbsoluteAddress(&vm.m_exception)) -> exception stub
    // (D3): materialize the baked absolute address, load the word, and test it.
    h.masm_mut()
        .move_imm64(TrustedImm64::new(jit_pending_address as i64), EXC_ADDR_GPR);
    h.masm_mut()
        .load64(Address::new(EXC_ADDR_GPR, 0), EXC_ADDR_GPR);
    let slow_to_exception =
        h.masm_mut()
            .branch_test64(ResultCondition::NonZero, EXC_ADDR_GPR, EXC_ADDR_GPR);
    // emitPutVirtualRegister(result) (slow-path store; result in x0).
    h.masm_mut().store64(RESULT_GPR, address_for(dst));
    let slow_to_done = h.masm_mut().jump();

    // === EXCEPTION STUB (FIRST CUT, shared with op_add) =====================
    // FORWARD PREREQ #2: a real unwind is deferred. This labeled bail does NOT store
    // the result (dst untouched) and falls to the epilogue with `x0` holding the
    // shim's `JSValue::empty()`; the pending `m_exception` mirror is the JIT's
    // "exception pending" signal a real handler will consume.
    let exception_label = h.masm().label();
    let exception_to_done = h.masm_mut().jump();

    // === DONE / EPILOGUE ====================================================
    let done_label = h.masm().label();
    h.masm_mut().pop_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // refill tag pair
    h.masm_mut().pop_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // refill x19/x20
    h.masm_mut().pop_pair(CALL_FRAME_GPR, LINK_GPR); // restore caller fp/lr
    h.masm_mut().ret();

    // Resolve every forward branch (the LinkBuffer relink pass runs these during
    // finalize). All are `b`/`b.cond` direct branches. The number of slow exits is
    // per-op (sub: 3, mul: 4, bitand: 1, bitor/bitxor/lshift/rshift: 2).
    let mut link_records = Vec::with_capacity(fast.slow.len() + 4);
    for slow_jump in &fast.slow {
        link_records.push(slow_jump.to_link_record(slow_label));
    }
    link_records.push(fast.fast_to_done.to_link_record(done_label));
    link_records.push(slow_to_exception.to_link_record(exception_label));
    link_records.push(slow_to_done.to_link_record(done_label));
    link_records.push(exception_to_done.to_link_record(done_label));

    ArithImage {
        code: h.code().to_vec(),
        link_records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(code: &[u8]) -> Vec<u32> {
        code.chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// LDUR<64> with base == x29 (cfr): top byte 0xF8, L bit (22) == 1, Rn == 29.
    fn is_ldur_off_cfr(word: u32) -> bool {
        (word >> 24) == 0xf8 && (word >> 22) & 1 == 1 && ((word >> 5) & 0x1f) == 29
    }
    /// STUR<64> with base == x29 (cfr): top byte 0xF8, L bit (22) == 0, Rn == 29.
    fn is_stur_off_cfr(word: u32) -> bool {
        (word >> 24) == 0xf8 && (word >> 22) & 1 == 0 && ((word >> 5) & 0x1f) == 29
    }

    /// The shared prologue + operand loads are byte-identical to op_add's proven
    /// prefix (cross-checked there against entry_prologue.rs / assembly_helpers.rs).
    /// Asserting it here pins that the family templates the SAME prologue.
    fn assert_shared_prologue(w: &[u32]) {
        assert_eq!(w[0], 0xa9bf_7bfd, "stp fp,lr,[sp,#-16]!");
        assert_eq!(w[1], 0xaa01_03fd, "mov fp, x1");
        assert_eq!(w[4], 0xaa00_03f3, "mov x19, x0 (pinned VM := *mut Vm)");
        assert_eq!(w[5], 0xd2ff_ffdb, "movz x27,#0xfffe,lsl#48 (NumberTag)");
        assert_eq!(w[6], 0xd280_005c, "movz x28,#2");
        assert_eq!(w[7], 0xf2ff_ffdc, "movk x28,#0xfffe,lsl#48 (NotCellMask)");
        assert!(is_ldur_off_cfr(w[8]), "ldur op1 off x29: {:#010x}", w[8]);
        assert!(is_ldur_off_cfr(w[9]), "ldur op2 off x29: {:#010x}", w[9]);
    }

    // ------------------------------------------------------------------------
    // BYTE ORACLE — op_sub: the int32 fast path differs from op_add ONLY at the
    // arithmetic op (subs vs adds, with the LEFT-RIGHT operand order). Same int32
    // guards, same boxInt32, same store/jump structure.
    // ------------------------------------------------------------------------
    #[test]
    fn op_sub_fast_path_byte_oracle() {
        let image = emit_baseline_arith_int32(ArithFamilyOp::Sub, -1, -2, -3, 0x1000);
        let w = words(&image.code);
        assert_shared_prologue(&w);
        // branchIfNotInt32(left=x1): cmp x1,x27 ; b.lo ; nop.
        assert_eq!(w[10], 0xeb1b_003f, "cmp x1,x27 (branchIfNotInt32 left)");
        assert_eq!(w[11], 0x5400_0003, "b.lo (Below -> slow)");
        assert_eq!(w[12], 0xd503_201f, "nop");
        // branchIfNotInt32(right=x2): cmp x2,x27 ; b.lo ; nop.
        assert_eq!(w[13], 0xeb1b_005f, "cmp x2,x27 (branchIfNotInt32 right)");
        assert_eq!(w[14], 0x5400_0003, "b.lo (Below -> slow)");
        assert_eq!(w[15], 0xd503_201f, "nop");
        // branchSub32(Overflow, LEFT=x1, RIGHT=x2, RESULT=x0): subs w0,w1,w2
        // (left - right, NOT right - left) ; b.vs ; nop.
        assert_eq!(
            w[16], 0x6b02_0020,
            "subs w0,w1,w2 (left-right, flag-setting)"
        );
        assert_eq!(w[17], 0x5400_0006, "b.vs (Overflow -> slow)");
        assert_eq!(w[18], 0xd503_201f, "nop");
        // boxInt32(scratch=x0, result=x0): orr x0,x27,x0.
        assert_eq!(w[19], 0xaa00_0360, "orr x0,x27,x0 (boxInt32)");
        assert!(is_stur_off_cfr(w[20]), "stur dst off x29: {:#010x}", w[20]);
        assert_eq!(w[21], 0x1400_0000, "b (fast -> done, pre-link placeholder)");
        assert_eq!(*w.last().unwrap(), 0xd65f_03c0, "ret");
        // 3 slow exits (left, right, overflow) + fast/exc/slow/exc-done.
        assert_eq!(image.link_records.len(), 7, "sub: 3 slow + 4 structural");
    }

    // ------------------------------------------------------------------------
    // BYTE ORACLE — op_bitand: the structurally DISTINCT bitwise form (and64 of
    // the boxed operands + a SINGLE guard on the result; no per-operand guard, no
    // separate boxInt32).
    // ------------------------------------------------------------------------
    #[test]
    fn op_bitand_fast_path_byte_oracle() {
        let image = emit_baseline_arith_int32(ArithFamilyOp::BitAnd, -1, -2, -3, 0x1000);
        let w = words(&image.code);
        assert_shared_prologue(&w);
        // and64(LEFT=x1, RIGHT=x2, RESULT=x0): and x0,x1,x2.
        assert_eq!(w[10], 0x8a02_0020, "and x0,x1,x2 (and64 of boxed operands)");
        // branchIfNotInt32(RESULT=x0): cmp x0,x27 ; b.lo ; nop.
        assert_eq!(w[11], 0xeb1b_001f, "cmp x0,x27 (branchIfNotInt32 result)");
        assert_eq!(w[12], 0x5400_0003, "b.lo (Below -> slow)");
        assert_eq!(w[13], 0xd503_201f, "nop");
        // No boxInt32: the AND already preserved NumberTag. Straight to the store.
        assert!(is_stur_off_cfr(w[14]), "stur dst off x29: {:#010x}", w[14]);
        assert_eq!(w[15], 0x1400_0000, "b (fast -> done, pre-link placeholder)");
        assert_eq!(*w.last().unwrap(), 0xd65f_03c0, "ret");
        // 1 slow exit (the single result guard) + 4 structural.
        assert_eq!(image.link_records.len(), 5, "bitand: 1 slow + 4 structural");
    }

    // The mul neg-zero guard's PRESENCE is observable as an extra slow exit
    // (mul has 4 slow exits vs sub's 3) — proven without depending on -0 semantics.
    #[test]
    fn op_mul_has_negative_zero_guard_slow_exit() {
        let mul = emit_baseline_arith_int32(ArithFamilyOp::Mul, -1, -2, -3, 0x1000);
        // left, right, overflow, neg-zero = 4 slow + 4 structural.
        assert_eq!(
            mul.link_records.len(),
            8,
            "mul: 4 slow exits (incl. the branchTest32(Zero) neg-zero guard)"
        );
    }

    // The expected per-op slow-exit count, documenting each generator's shape.
    #[test]
    fn family_slow_exit_counts() {
        let count = |op| {
            emit_baseline_arith_int32(op, -1, -2, -3, 0x1000)
                .link_records
                .len()
        };
        assert_eq!(count(ArithFamilyOp::Sub), 7, "3 slow + 4");
        assert_eq!(count(ArithFamilyOp::Mul), 8, "4 slow + 4");
        assert_eq!(count(ArithFamilyOp::BitAnd), 5, "1 slow + 4");
        assert_eq!(count(ArithFamilyOp::BitOr), 6, "2 slow + 4");
        assert_eq!(count(ArithFamilyOp::BitXor), 6, "2 slow + 4");
        assert_eq!(count(ArithFamilyOp::LShift), 6, "2 slow + 4");
        assert_eq!(count(ArithFamilyOp::RShift), 6, "2 slow + 4");
    }

    // ------------------------------------------------------------------------
    // THE MILESTONE: emit -> relocate -> EXECUTE under W^X, for every family op.
    // Mirrors op_add's execution harness exactly (a real `Vm` + dispatch host + a
    // JSStack frame with two int32 operands); asserts the dst slot + return register
    // hold the faithful result for the fast path, the overflow/neg slow paths, the
    // non-int slow path, and the throw edge. macOS/aarch64 only.
    // ------------------------------------------------------------------------
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    mod execution {
        use super::super::*;
        use crate::interpreter::CoreOpcodeDispatchHost;
        use crate::jit::executable_allocator::{
            finalize_arm64_link_buffer, MapJitExecutableAllocator,
        };
        use crate::value::{EncodedJsValue, JsValue};
        use crate::vm::jsstack::{JsStack, Register};
        use crate::vm::{Vm, VmConfig};

        const OP1: i32 = -1;
        const OP2: i32 = -2;
        const DST: i32 = -3;
        // A recognizable pre-seed in dst so the throw edge ("dst left untouched") is
        // detectable; not a valid result of any tested case.
        const DST_SENTINEL: u64 = 0xABCD_1234_5678_9ABC;

        struct Frame {
            stack: JsStack,
            fp: usize,
        }

        impl Frame {
            fn new() -> Self {
                let stack = JsStack::with_test_backing(32);
                let fp = stack.high_address() - 64;
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

        struct CaseResult {
            dst: u64,
            ret: u64,
            pending: u64,
        }

        fn run_case(op: ArithFamilyOp, op1_bits: u64, op2_bits: u64) -> CaseResult {
            let mut host = CoreOpcodeDispatchHost::new();
            let mut vm = Vm::new(VmConfig::interpreter_only());
            vm.set_jit_host(&mut host); // D5: park the host before entering JIT code.
            let jit_pending_address = vm.jit_pending_exception_address() as usize;

            let frame = Frame::new();
            frame.write(OP1, op1_bits);
            frame.write(OP2, op2_bits);
            frame.write(DST, DST_SENTINEL);

            let image = emit_baseline_arith_int32(op, OP1, OP2, DST, jit_pending_address);
            let mut records = image.link_records;
            let handle =
                finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                    .expect("finalize arith image");

            // Park `*mut Vm` (-> x0) + frame base (-> x1) and execute under W^X. The
            // driver does NOT touch the parked `&mut vm`/`&mut host` until the call
            // returns — the only access is the shim's single reborrow each.
            let vm_ptr: *mut Vm = &mut vm;
            let ret = handle.call_finalized_binary_u64(vm_ptr as u64, frame.fp as u64);

            let dst = frame.read(DST);
            let pending = vm.jit_pending_exception().0;
            vm.clear_jit_host();
            CaseResult { dst, ret, pending }
        }

        fn i32_bits(value: i32) -> u64 {
            JsValue::from_i32(value).encoded().0
        }

        /// Assert a fast int32 case: dst + return both equal the boxed int32 result,
        /// no exception, and the value really is an int32 (not a slow-path double).
        fn assert_fast_int32(op: ArithFamilyOp, a: i32, b: i32, expected: i32) {
            let r = run_case(op, i32_bits(a), i32_bits(b));
            let want = i32_bits(expected);
            assert_eq!(r.dst, want, "{op:?}: dst = boxed int32 {expected}");
            assert_eq!(r.ret, want, "{op:?}: returnValue = boxed int32 {expected}");
            assert_eq!(r.pending, 0, "{op:?}: fast path raises no exception");
            assert!(
                JsValue::from_encoded(EncodedJsValue(r.dst)).is_int32(),
                "{op:?}: result is an int32"
            );
        }

        // --- Fast int32 paths (entirely in-register; no runtime call). ---------
        #[test]
        fn fast_int32_paths() {
            assert_fast_int32(ArithFamilyOp::Sub, 7, 3, 4); // 7 - 3
            assert_fast_int32(ArithFamilyOp::Sub, 3, 7, -4); // order matters
            assert_fast_int32(ArithFamilyOp::Mul, 6, 7, 42);
            assert_fast_int32(ArithFamilyOp::Mul, -6, 7, -42);
            assert_fast_int32(ArithFamilyOp::BitAnd, 12, 10, 8);
            assert_fast_int32(ArithFamilyOp::BitOr, 12, 10, 14);
            assert_fast_int32(ArithFamilyOp::BitXor, 12, 10, 6);
            assert_fast_int32(ArithFamilyOp::LShift, 1, 4, 16);
            assert_fast_int32(ArithFamilyOp::LShift, 1, 31, i32::MIN); // 1<<31
            assert_fast_int32(ArithFamilyOp::RShift, 256, 2, 64);
            assert_fast_int32(ArithFamilyOp::RShift, -256, 2, -64); // arithmetic (sign)
            assert_fast_int32(ArithFamilyOp::LShift, 5, 33, 10); // shift amount & 31 == 1
        }

        // --- Sub overflow -> slow path -> faithful boxed double. ---------------
        #[test]
        fn sub_overflow_takes_slow_path_to_boxed_double() {
            // INT_MIN - 1 overflows -> operationValueSub -> -2147483649.0 (double).
            let r = run_case(ArithFamilyOp::Sub, i32_bits(i32::MIN), i32_bits(1));
            let expected = JsValue::from_double(i32::MIN as f64 - 1.0).encoded().0;
            assert_eq!(r.dst, expected, "dst holds boxed double -2147483649.0");
            assert_eq!(r.ret, expected, "returnValue holds the boxed double");
            assert_eq!(r.pending, 0, "no exception");
            assert!(!JsValue::from_encoded(EncodedJsValue(r.dst)).is_int32());
        }

        // --- Mul overflow -> slow path -> faithful boxed double. ---------------
        #[test]
        fn mul_overflow_takes_slow_path_to_boxed_double() {
            // 0x40000000 * 4 overflows int32 -> 4294967296.0 (double).
            let r = run_case(ArithFamilyOp::Mul, i32_bits(0x4000_0000), i32_bits(4));
            let expected = JsValue::from_double(0x4000_0000i64 as f64 * 4.0)
                .encoded()
                .0;
            assert_eq!(r.dst, expected, "dst holds boxed double 4294967296.0");
            assert_eq!(r.ret, expected, "returnValue holds the boxed double");
            assert_eq!(r.pending, 0, "no exception");
            assert!(!JsValue::from_encoded(EncodedJsValue(r.dst)).is_int32());
        }

        // --- The mul negative-zero guard routes int*0 to the slow path. The slow
        // evaluator yields int32 0 here (the -0 double materialization is a separate
        // evaluator concern), so dst == boxed int32 0; the guard's effect (taking the
        // slow path) is what is proven (its presence is also in the byte/link tests).
        #[test]
        fn mul_zero_result_routes_through_neg_zero_guard() {
            let r = run_case(ArithFamilyOp::Mul, i32_bits(-1), i32_bits(0));
            assert_eq!(
                r.dst,
                i32_bits(0),
                "dst = boxed 0 (slow path via neg-zero guard)"
            );
            assert_eq!(r.pending, 0, "no exception");
        }

        // --- A non-int32 (double) operand fails branchIfNotInt32 -> slow path. --
        #[test]
        fn non_int_operand_takes_slow_path() {
            // Sub: 1.5 - 2 = -0.5 (double).
            let r = run_case(
                ArithFamilyOp::Sub,
                JsValue::from_double(1.5).encoded().0,
                i32_bits(2),
            );
            assert_eq!(
                r.dst,
                JsValue::from_double(-0.5).encoded().0,
                "sub double operand -> boxed double -0.5"
            );
            assert_eq!(r.pending, 0, "no exception");

            // Mul: 1.5 * 3 = 4.5 (double).
            let r = run_case(
                ArithFamilyOp::Mul,
                JsValue::from_double(1.5).encoded().0,
                i32_bits(3),
            );
            assert_eq!(
                r.dst,
                JsValue::from_double(4.5).encoded().0,
                "mul double operand -> boxed double 4.5"
            );

            // BitAnd: 1.5 & 3 -> slow (the and64-result guard fires) ->
            // bitwise_binary_result(number_to_int32(1.5)=1, 3) = 1.
            let r = run_case(
                ArithFamilyOp::BitAnd,
                JsValue::from_double(1.5).encoded().0,
                i32_bits(3),
            );
            assert_eq!(
                r.dst,
                i32_bits(1),
                "bitand double operand -> int32 1 via slow path"
            );
            assert_eq!(r.pending, 0, "no exception");

            // LShift: 1.5 << 2 -> slow -> number_to_int32(1.5)=1, 1 << 2 = 4.
            let r = run_case(
                ArithFamilyOp::LShift,
                JsValue::from_double(1.5).encoded().0,
                i32_bits(2),
            );
            assert_eq!(
                r.dst,
                i32_bits(4),
                "lshift double operand -> int32 4 via slow path"
            );
        }

        // --- The throw edge: a primitive non-number operand makes the evaluator
        // Err(Fail); the shim stamps m_exception + returns empty; the exception stub
        // bails WITHOUT storing, so dst keeps its sentinel. (Same edge op_add proves.)
        #[test]
        fn throw_edge_takes_exception_stub() {
            // 0x8 = ValueKind::Unknown primitive: ToNumber rejects it.
            for op in [ArithFamilyOp::Sub, ArithFamilyOp::BitXor] {
                let r = run_case(op, 0x8u64, i32_bits(7));
                assert_eq!(
                    r.dst, DST_SENTINEL,
                    "{op:?}: exception stub leaves dst unwritten"
                );
                assert_eq!(
                    r.ret, 0,
                    "{op:?}: exception edge returns JSValue::empty() bits"
                );
                assert_ne!(
                    r.pending, 0,
                    "{op:?}: m_exception mirror set on the throw edge"
                );
            }
        }
    }
}
