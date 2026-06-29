//! ARM64 baseline `op_add` lowering — the first real bytecode op executed
//! end-to-end through the baseline JIT (fast int32 path + runtime slow path).
//!
//! C++ JSC map (source of truth):
//! - `jit/JITArithmetic.cpp` `JIT::emit_op_add` (:792) -> `emitMathICFast<OpAdd>`
//!   (:855): load both operands with `emitGetVirtualRegister`, run the int32
//!   generator inline, `addSlowCase(slowPathJumps)`, then `emitPutVirtualRegister`
//!   the result. `JIT::emitSlow_op_add` (:800) -> `emitMathICSlow` calls
//!   `operationValueAdd(globalObject, left, right)` and `emitPutVirtualRegister`s
//!   the call result.
//! - `jit/JITAddGenerator.cpp` `generateInline` (:36-73, int32 path): the operand
//!   guards `branchIfNotInt32(left/right)`, the overflowing add
//!   `branchAdd32(Overflow, right.payloadGPR(), left.payloadGPR(), scratch)` with
//!   `scratch = m_result.payloadGPR()` when result aliases neither operand, then
//!   `boxInt32(scratch, m_result)`.
//! - `jit/AssemblyHelpers.h` `addressFor(vreg) = Address(x29, vreg.offset()*8)`
//!   (:1290-1298, cfr == x29) — `emitGetVirtualRegister`/`emitPutVirtualRegister`
//!   are `load64`/`store64` over that address (the JSStack frame model,
//!   docs/design/jsstack.md).
//! - `jit/JITOperations.cpp` `operationValueAdd` (:4860) -> the runtime shim
//!   [`crate::jit::operations::operation_value_add`] (the JIT<->runtime bridge,
//!   docs/design/jit-runtime-bridge.md).
//!
//! Register identity (GPRInfo.h ARM64): operands occupy `argumentGPR1`/
//! `argumentGPR2` (the `operationValueAdd` arg slots, kept live across the fast
//! path because the fast-path add targets the RESULT register, not the operands —
//! exactly JSC's MathIC discipline); the result/scratch is `returnValueGPR`; the
//! tag pair is `numberTagRegister`/`notCellMaskRegister` (x27/x28).
//!
//! Unsafe boundary: this module is entirely SAFE (`jit/mod.rs` is
//! `#![deny(unsafe_code)]`; nothing here re-enables it). It only COMPUTES
//! instruction bytes (like `AssemblyHelpers`/`MacroAssemblerArm64`) and, in the
//! execution test, COMPOSES three already-audited unsafe islands: the W^X
//! executable-call path (`executable_allocator` / `unsafe_platform_boundary`), the
//! runtime-call shim's `*mut Vm`/`*mut host` reborrows (`jit/operations.rs`, D1/D5,
//! Miri-verified), and the JSStack provenance gate (`vm/jsstack.rs`). The op_add
//! lowering itself introduces no new `unsafe`.
//!
//! SCOPE — this proof is INT32/PRIMITIVE only. `operation_value_add` over
//! (int32,int32) overflow or (number,number) is sound with the bridge as-is.
//! Three documented forward prerequisites (jit-runtime-bridge.md "Hard forward
//! prerequisites") are DEFERRED and commented at their sites below:
//!   1. OBJECT operands need the real active-frame `CodeBlock` (`{} + 1`'s
//!      ToNumber reads it); the primitive AddInt32 path never does, so the
//!      placeholder `CodeBlock` in `Vm::operation_value_add` is sound only for
//!      primitives. Do NOT claim object-operand support.
//!   2. A real `TypeError` cell for an engine `Fail` — the exception stub here is
//!      a FIRST-CUT labeled bail (the real unwind is deferred).
//!   3. The baked `jit_pending` address must be pin-stable for the call's
//!      duration (the trampoline must not move the `Vm`).

#![allow(dead_code)]

use crate::assembler::link_records::Arm64LinkRecord;
use crate::assembler::macro_assembler_arm64::ResultCondition;
use crate::assembler::operands::{Address, TrustedImm64};
use crate::assembler::registers::RegisterID;
use crate::jit::assembly_helpers::{AssemblyHelpers, TagRegistersMode};
use crate::jit::operations::operation_value_add;

/// `sizeof(Register)` (JSVALUE64); `addressFor` scales the VirtualRegister by it.
const REGISTER_SIZE_BYTES: i32 = 8;

// --- Register identity (GPRInfo.h, ARM64 baseline) ---------------------------

/// `x29 == cfr` (GPRInfo.h:582; AssemblyHelpers.h:1290-1298). The frame pointer
/// every `addressFor(vreg)` is relative to.
const CALL_FRAME_GPR: RegisterID = RegisterID::Fp;
/// `x30 == lr`. Saved/restored around the operation far-call (a `blr` clobbers it).
const LINK_GPR: RegisterID = RegisterID::Lr;

/// The frame base arrives in the raw C-ABI argument `x1`, matching the existing
/// ARM64 return-seed lane (`entry_prologue.rs`: `mov fp, x1`). The prologue moves
/// it into `cfr`; `x1` is then free and reused as `leftRegs` below.
const RAW_FRAME_ARG_GPR: RegisterID = RegisterID::X1;
/// The `*mut Vm` arrives in the raw C-ABI argument `x0` and is also the operation
/// shim's arg0 (`operationValueAdd`'s `globalObject` slot, PRE-RESOLVED to the Vm
/// per jit-runtime-bridge.md D2c). The prologue stashes it into the pinned-VM reg.
const RAW_VM_ARG_GPR: RegisterID = RegisterID::X0;

/// `leftRegs == argumentGPR1` (x1): op1 lives here through the fast path and is
/// reused, unmodified, as the operation's arg1 in the slow path.
const LEFT_GPR: RegisterID = RegisterID::X1;
/// `rightRegs == argumentGPR2` (x2): op2, likewise reused as arg2.
const RIGHT_GPR: RegisterID = RegisterID::X2;
/// `resultRegs == returnValueGPR` (x0): the add scratch and the boxed result; in
/// the slow path it is also the operation's arg0 (the Vm) and its return value.
const RESULT_GPR: RegisterID = RegisterID::X0;

/// The pinned-VM carrier (jit-runtime-bridge.md D2c). JSC's baseline reaches the
/// VM via `globalObject->vm()` and has no pinned VM register; this port reserves
/// one (`BASELINE_PINNED_VM_REGISTER`, jit/abi.rs:573) and concretizes it here as
/// a CALLEE-SAVED GPR (x19 / regCS0) so the `*mut Vm` survives the operation
/// far-call (AAPCS64 callees preserve x19-x28). The prologue spills/refills it so
/// the emitted function also honors AAPCS64 toward its own caller.
const PINNED_VM_GPR: RegisterID = RegisterID::X19;
/// Paired with `PINNED_VM_GPR` only to keep `stp`/`ldp` 16-byte aligned; unused.
const PINNED_VM_PAIR_GPR: RegisterID = RegisterID::X20;

/// `numberTagRegister` (x27) / `notCellMaskRegister` (x28), the
/// `HaveTagRegisters` pair the fast path's int32 guards and box rely on. Spilled
/// in the prologue (callee-saved) and refilled in the epilogue.
const NUMBER_TAG_GPR: RegisterID = AssemblyHelpers::NUMBER_TAG_REGISTER;
const NOT_CELL_MASK_GPR: RegisterID = AssemblyHelpers::NOT_CELL_MASK_REGISTER;

/// Caller-saved scratch for the `branchTestPtr(NonZero, AbsoluteAddress(...))`
/// exception-word probe after the call (free once the result is in `x0`).
const EXC_ADDR_GPR: RegisterID = RegisterID::X3;

/// `AssemblyHelpers::addressFor(VirtualRegister) = Address(x29, vreg.offset()*8)`
/// (AssemblyHelpers.h:1290-1298). The operand IS the VirtualRegister's raw value
/// (`VirtualRegister::offsetInBytes = operand * sizeof(Register)`,
/// VirtualRegister.h:79): locals (operand < 0) land below `cfr`, arguments/header
/// (operand >= 0) at/above it.
fn address_for(operand: i32) -> Address {
    Address::new(CALL_FRAME_GPR, operand.wrapping_mul(REGISTER_SIZE_BYTES))
}

/// An emitted (but not-yet-finalized) `op_add` image: the assembler byte buffer
/// plus the forward-branch link records the LinkBuffer pass resolves. Mirrors the
/// `(code, jumpsToLink)` an `JIT`/`LinkBuffer` carries before
/// `finalizeCodeWithoutDisassembly`.
pub(crate) struct OpAddImage {
    pub(crate) code: Vec<u8>,
    pub(crate) link_records: Vec<Arm64LinkRecord>,
}

/// Emit the baseline `op_add` lowering for `dst = op1 + op2` (all
/// `VirtualRegister` raw operands) as a standalone callable image.
///
/// Faithful to `emit_op_add` + `JITAddGenerator::generateInline` (int32) +
/// `emitSlow_op_add`. The result executes as `extern "C" fn(vm: *mut Vm, cfr: u64)
/// -> u64` (the raw return-seed C-ABI lane, `entry_prologue.rs`): arg0 = the
/// `*mut Vm` for the operation slow path, arg1 = the frame base (`cfr`).
///
/// `jit_pending_address` is the baked `AbsoluteAddress` of the `Vm`'s
/// `m_exception` mirror word (`Vm::jit_pending_exception_address`, D3). FORWARD
/// PREREQ #3: it must remain stable for the call's duration (a pin-stable `Vm`).
pub(crate) fn emit_baseline_op_add_int32(
    op1: i32,
    op2: i32,
    dst: i32,
    jit_pending_address: usize,
) -> OpAddImage {
    let mut h = AssemblyHelpers::new();
    let mode = TagRegistersMode::HaveTagRegisters;

    // === PROLOGUE ===========================================================
    // `emitFunctionPrologue` (push fp/lr, fp := cfr) for the raw C-ABI lane
    // (frame base in x1; entry_prologue.rs), plus the callee-saved spills this op
    // clobbers so the emitted function obeys AAPCS64 toward its caller.
    h.masm_mut().push_pair(CALL_FRAME_GPR, LINK_GPR); // stp fp,lr,[sp,#-16]!
    h.masm_mut().move_rr(RAW_FRAME_ARG_GPR, CALL_FRAME_GPR); // mov fp, x1  (cfr)
    h.masm_mut().push_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // spill x19 (+x20 pad)
    h.masm_mut().push_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // spill tag pair x27/x28
    h.masm_mut().move_rr(RAW_VM_ARG_GPR, PINNED_VM_GPR); // pinned-VM := *mut Vm (x0)
    h.emit_materialize_tag_check_registers(); // x27 = NumberTag, x28 = NotCellMask

    // === FAST PATH (JITAddGenerator::generateInline, int32) =================
    // emitGetVirtualRegister(op1, leftRegs) / (op2, rightRegs).
    h.masm_mut().load64(address_for(op1), LEFT_GPR);
    h.masm_mut().load64(address_for(op2), RIGHT_GPR);
    // branchIfNotInt32(left) / branchIfNotInt32(right) -> slowPathJumps.
    let slow_left_not_int = h.branch_if_not_int32(LEFT_GPR, mode);
    let slow_right_not_int = h.branch_if_not_int32(RIGHT_GPR, mode);
    // branchAdd32(Overflow, right.payloadGPR(), left.payloadGPR(), scratch) with
    // scratch = m_result.payloadGPR() (result aliases neither operand). RESULT_GPR
    // (x0) = RIGHT + LEFT (32-bit, flag-setting); overflow -> slowPathJumps.
    let slow_overflow =
        h.masm_mut()
            .branch_add32(ResultCondition::Overflow, RIGHT_GPR, LEFT_GPR, RESULT_GPR);
    // boxInt32(scratch, m_result): RESULT_GPR := NumberTag | uint32(sum).
    h.box_int32(RESULT_GPR, RESULT_GPR, mode);
    // emitPutVirtualRegister(result, resultRegs) (fast-path store).
    h.masm_mut().store64(RESULT_GPR, address_for(dst));
    // endJumpList.append(jump()): skip the slow path.
    let fast_to_done = h.masm_mut().jump();

    // === SLOW PATH (emitSlow_op_add / emitMathICSlow) =======================
    let slow_label = h.masm().label();
    // loadGlobalObject(globalObjectGPR) analog: arg0 := the pinned `*mut Vm`
    // (jit-runtime-bridge.md D2c — the Vm is `globalObject->vm()` pre-resolved).
    h.masm_mut().move_rr(PINNED_VM_GPR, RAW_VM_ARG_GPR); // mov x0, x19
                                                         // left (x1) and right (x2) are STILL the original operands — the fast-path add
                                                         // wrote only x0 — so, exactly like JSC's MathIC slow path, they need no reload
                                                         // and are passed straight through as arg1/arg2.
                                                         //
                                                         // FORWARD PREREQ #1: an OBJECT operand reaching `operationValueAdd` needs the
                                                         // real active-frame CodeBlock (ToNumber/valueOf). `Vm::operation_value_add`
                                                         // uses a placeholder CodeBlock, sound ONLY for the primitive AddInt32 path
                                                         // exercised here; object-operand support is NOT provided.
    let _call = h
        .masm_mut()
        .far_call(TrustedImm64::new(operation_value_add as usize as i64));
    // branchTestPtr(NonZero, AbsoluteAddress(&vm.m_exception)) -> exception stub
    // (D3). No direct memory-operand test in this macro layer yet: materialize the
    // baked absolute address, load the word, and test it.
    h.masm_mut()
        .move_imm64(TrustedImm64::new(jit_pending_address as i64), EXC_ADDR_GPR);
    h.masm_mut()
        .load64(Address::new(EXC_ADDR_GPR, 0), EXC_ADDR_GPR);
    let slow_to_exception =
        h.masm_mut()
            .branch_test64(ResultCondition::NonZero, EXC_ADDR_GPR, EXC_ADDR_GPR);
    // emitPutVirtualRegister(result, resultRegs) (slow-path store; result in x0).
    h.masm_mut().store64(RESULT_GPR, address_for(dst));
    let slow_to_done = h.masm_mut().jump();

    // === EXCEPTION STUB (FIRST CUT) =========================================
    // FORWARD PREREQ #2: a real unwind (lookupExceptionHandler / OperationResult
    // exception path) is deferred. This is the labeled bail the proof detects: it
    // does NOT store the result (dst is left untouched) and falls back to the
    // epilogue with `x0` holding the shim's `JSValue::empty()`. The pending
    // `m_exception` mirror (set by the shim) is the JIT's "exception pending"
    // signal a real handler will consume.
    let exception_label = h.masm().label();
    let exception_to_done = h.masm_mut().jump();

    // === DONE / EPILOGUE ====================================================
    let done_label = h.masm().label();
    h.masm_mut().pop_pair(NUMBER_TAG_GPR, NOT_CELL_MASK_GPR); // refill tag pair
    h.masm_mut().pop_pair(PINNED_VM_GPR, PINNED_VM_PAIR_GPR); // refill x19/x20
    h.masm_mut().pop_pair(CALL_FRAME_GPR, LINK_GPR); // restore caller fp/lr
    h.masm_mut().ret();

    // Resolve every forward branch (the LinkBuffer relink pass runs these in
    // place during finalize). All are `b`/`b.cond` direct branches.
    let link_records = vec![
        slow_left_not_int.to_link_record(slow_label),
        slow_right_not_int.to_link_record(slow_label),
        slow_overflow.to_link_record(slow_label),
        fast_to_done.to_link_record(done_label),
        slow_to_exception.to_link_record(exception_label),
        slow_to_done.to_link_record(done_label),
        exception_to_done.to_link_record(done_label),
    ];

    OpAddImage {
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

    // ------------------------------------------------------------------------
    // BYTE ORACLE of the deterministic prologue + int32 fast path (words 0..=21;
    // the address-dependent far-call/move-imm tail is excluded). Every asserted
    // word is cross-checked against an existing passing byte test
    // (entry_prologue.rs / assembly_helpers.rs / macro_assembler_arm64.rs), so a
    // drift in the lowering is caught without re-deriving the encoder.
    // ------------------------------------------------------------------------
    #[test]
    fn op_add_fast_path_byte_oracle() {
        // op1 = local0 (operand -1 -> [x29,-8]); op2 = local1 ([x29,-16]);
        // dst = local2 ([x29,-24]). jit_pending address is irrelevant to the
        // asserted prefix.
        let image = emit_baseline_op_add_int32(-1, -2, -3, 0x1000);
        let w = words(&image.code);

        // Prologue.
        assert_eq!(w[0], 0xa9bf_7bfd, "stp fp,lr,[sp,#-16]!"); // entry_prologue.rs
        assert_eq!(w[1], 0xaa01_03fd, "mov fp, x1"); // entry_prologue.rs
        assert_eq!(w[4], 0xaa00_03f3, "mov x19, x0 (pinned VM := *mut Vm)");
        // emitMaterializeTagCheckRegisters (assembly_helpers.rs).
        assert_eq!(w[5], 0xd2ff_ffdb, "movz x27,#0xfffe,lsl#48 (NumberTag)");
        assert_eq!(w[6], 0xd280_005c, "movz x28,#2");
        assert_eq!(w[7], 0xf2ff_ffdc, "movk x28,#0xfffe,lsl#48 (NotCellMask)");
        // op1/op2 loads are LDUR off x29 (negative locals); exact imm verified by
        // the execution test, structure verified here.
        assert!(is_ldur_off_cfr(w[8]), "ldur op1 off x29: {:#010x}", w[8]);
        assert!(is_ldur_off_cfr(w[9]), "ldur op2 off x29: {:#010x}", w[9]);
        // branchIfNotInt32(left=x1): cmp x1,x27 ; b.lo ; nop.
        assert_eq!(w[10], 0xeb1b_003f, "cmp x1,x27 (branchIfNotInt32 left)");
        assert_eq!(w[11], 0x5400_0003, "b.lo (Below -> slow)");
        assert_eq!(w[12], 0xd503_201f, "nop");
        // branchIfNotInt32(right=x2): cmp x2,x27 ; b.lo ; nop.
        assert_eq!(w[13], 0xeb1b_005f, "cmp x2,x27 (branchIfNotInt32 right)");
        assert_eq!(w[14], 0x5400_0003, "b.lo (Below -> slow)");
        assert_eq!(w[15], 0xd503_201f, "nop");
        // branchAdd32(Overflow, right=x2, left=x1, result=x0): adds w0,w2,w1 ;
        // b.vs ; nop (macro_assembler_arm64.rs branchAdd32 byte test).
        assert_eq!(
            w[16], 0x2b01_0040,
            "adds w0,w2,w1 (right+left, flag-setting)"
        );
        assert_eq!(w[17], 0x5400_0006, "b.vs (Overflow -> slow)");
        assert_eq!(w[18], 0xd503_201f, "nop");
        // boxInt32(scratch=x0, result=x0): orr x0,x27,x0 (assembly_helpers.rs).
        assert_eq!(w[19], 0xaa00_0360, "orr x0,x27,x0 (boxInt32)");
        // emitPutVirtualRegister(dst): STUR off x29.
        assert!(is_stur_off_cfr(w[20]), "stur dst off x29: {:#010x}", w[20]);
        // endJumpList jump (unlinked placeholder before finalize).
        assert_eq!(w[21], 0x1400_0000, "b (fast -> done, pre-link placeholder)");

        // The image ends in the epilogue `ret` and carries exactly the seven
        // forward branches the lowering creates.
        assert_eq!(*w.last().unwrap(), 0xd65f_03c0, "ret");
        assert_eq!(
            image.link_records.len(),
            7,
            "slow x3 + done x3 + exception x1"
        );
    }

    /// LDUR<64> with base == x29 (cfr): top byte 0xF8, L bit (22) == 1, Rn == 29.
    fn is_ldur_off_cfr(word: u32) -> bool {
        (word >> 24) == 0xf8 && (word >> 22) & 1 == 1 && ((word >> 5) & 0x1f) == 29
    }
    /// STUR<64> with base == x29 (cfr): top byte 0xF8, L bit (22) == 0, Rn == 29.
    fn is_stur_off_cfr(word: u32) -> bool {
        (word >> 24) == 0xf8 && (word >> 22) & 1 == 0 && ((word >> 5) & 0x1f) == 29
    }

    // ------------------------------------------------------------------------
    // THE MILESTONE: emit -> relocate -> EXECUTE under W^X. Build a real `Vm` +
    // real dispatch host + a JSStack frame holding two int32 operands, finalize
    // the op_add image through the MAP_JIT allocator, run it as machine code, and
    // assert the destination slot (and the return register) hold the faithful
    // result for the fast path, the overflow slow path, the non-int slow path,
    // and the throw edge. macOS/aarch64 only (this executes native ARM64);
    // cannot run under Miri (JIT execution is outside Miri's interpreter — the
    // Rust-side D1/D5 reborrows are Miri-verified separately in
    // `jit::operations::tests`).
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

        // op1 = local0, op2 = local1, dst = local2 (operands -1,-2,-3 ->
        // [x29,-8], [x29,-16], [x29,-24]).
        const OP1: i32 = -1;
        const OP2: i32 = -2;
        const DST: i32 = -3;
        // A recognizable pre-seed in dst so the throw edge ("dst left untouched")
        // is detectable; it is NOT a valid result of any tested case.
        const DST_SENTINEL: u64 = 0xABCD_1234_5678_9ABC;

        struct Frame {
            stack: JsStack,
            fp: usize,
        }

        impl Frame {
            fn new() -> Self {
                // A small immovable backing window; `fp` sits high with room for
                // the three locals below it. (A full doVMEntry seed is exercised
                // in the jsstack B2 tests; here the operands stand in for already-
                // materialized temporaries, the input op_add reads.)
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

        fn run_case(op1_bits: u64, op2_bits: u64) -> CaseResult {
            // A REAL store-bearing host (the str+str path would reach its stores);
            // the primitive cases below never touch them but the shim asserts the
            // host is parked (D5).
            let mut host = CoreOpcodeDispatchHost::new();
            let mut vm = Vm::new(VmConfig::interpreter_only());
            // D5: park the host before entering JIT code.
            vm.set_jit_host(&mut host);
            // D3 + FORWARD PREREQ #3: bake the m_exception mirror's address. `vm`
            // is a fixed stack local that is not moved before the call returns, so
            // the baked AbsoluteAddress stays valid.
            let jit_pending_address = vm.jit_pending_exception_address() as usize;

            let frame = Frame::new();
            frame.write(OP1, op1_bits);
            frame.write(OP2, op2_bits);
            frame.write(DST, DST_SENTINEL);

            let image = emit_baseline_op_add_int32(OP1, OP2, DST, jit_pending_address);
            let mut records = image.link_records;
            let handle =
                finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                    .expect("finalize op_add");

            // The trampoline: materialize `*mut Vm` (-> x0, into the pinned-VM reg)
            // and the frame base (-> x1, into cfr) and execute under W^X. D1/D5
            // park discipline: the driver does NOT read/write the parked `&mut vm`
            // or `&mut host` between here and the call's return — the only access
            // to them while JIT runs is the shim's single reborrow each.
            let vm_ptr: *mut Vm = &mut vm;
            let ret = handle.call_finalized_binary_u64(vm_ptr as u64, frame.fp as u64);

            // Parked region over: reading vm/host directly is sound again.
            let dst = frame.read(DST);
            let pending = vm.jit_pending_exception().0;
            vm.clear_jit_host();
            CaseResult { dst, ret, pending }
        }

        #[test]
        fn fast_path_int32_add() {
            // 2 + 3 -> boxed int32 5, entirely in-register (no runtime call).
            let r = run_case(
                JsValue::from_i32(2).encoded().0,
                JsValue::from_i32(3).encoded().0,
            );
            let expected = JsValue::from_i32(5).encoded().0;
            assert_eq!(r.dst, expected, "dst slot holds boxed int32 5");
            assert_eq!(r.ret, expected, "returnValue holds boxed int32 5");
            assert_eq!(r.pending, 0, "fast path raises no exception");
            assert_eq!(
                JsValue::from_encoded(EncodedJsValue(r.dst)),
                JsValue::from_i32(5)
            );
        }

        #[test]
        fn overflow_takes_slow_path_to_boxed_double() {
            // INT_MAX + 1 overflows the int32 add -> slow path -> operationValueAdd
            // -> faithful boxed double 2147483648.0.
            let r = run_case(
                JsValue::from_i32(i32::MAX).encoded().0,
                JsValue::from_i32(1).encoded().0,
            );
            let expected = JsValue::from_double(i32::MAX as f64 + 1.0).encoded().0;
            assert_eq!(r.dst, expected, "dst holds boxed double 2147483648.0");
            assert_eq!(r.ret, expected, "returnValue holds the boxed double");
            assert_eq!(r.pending, 0, "no exception");
            // Sanity: it is genuinely a double, not an int32.
            assert!(!JsValue::from_encoded(EncodedJsValue(r.dst)).is_int32());
        }

        #[test]
        fn non_int_operand_takes_slow_path_to_faithful_sum() {
            // A double operand fails branchIfNotInt32 -> slow path -> 1.5 + 2 = 3.5.
            let r = run_case(
                JsValue::from_double(1.5).encoded().0,
                JsValue::from_i32(2).encoded().0,
            );
            let expected = JsValue::from_double(3.5).encoded().0;
            assert_eq!(r.dst, expected, "dst holds boxed double 3.5");
            assert_eq!(r.ret, expected, "returnValue holds the boxed double");
            assert_eq!(r.pending, 0, "no exception");
        }

        #[test]
        fn throw_edge_takes_exception_stub() {
            // 0x8 is a primitive non-number (ValueKind::Unknown) operand: the
            // evaluator returns Err(Fail) and the shim stamps the m_exception
            // mirror + returns JSValue::empty(). The exception stub bails WITHOUT
            // storing, so dst keeps its sentinel.
            let r = run_case(0x8u64, JsValue::from_i32(7).encoded().0);
            assert_eq!(r.dst, DST_SENTINEL, "exception stub leaves dst unwritten");
            assert_eq!(r.ret, 0, "exception edge returns JSValue::empty() bits");
            assert_ne!(r.pending, 0, "m_exception mirror set on the throw edge");
        }
    }
}
