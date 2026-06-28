//! `AssemblyHelpers` — the JSValue tag/box layer over `MacroAssemblerArm64`.
//!
//! Faithful port of the JSVALUE64 subset of `Source/JavaScriptCore/jit/
//! AssemblyHelpers.h` that the int32 fast paths need. C++ `AssemblyHelpers`
//! derives from `MacroAssembler` (`class AssemblyHelpers : public MacroAssembler`,
//! AssemblyHelpers.h:60) and adds the JSValue-aware helpers — tag-register
//! materialization, int32 type checks, and int32 box/unbox. Rust has no
//! inheritance, so [`AssemblyHelpers`] OWNS a [`MacroAssemblerArm64`] by
//! composition (the same "is-a MacroAssembler" relationship expressed as
//! has-a + delegation); every helper here lowers to `masm` composite ops.
//!
//! This layer is SAFE (`jit/` is `#![deny(unsafe_code)]`): it only computes
//! instruction bytes, exactly like the assembler layers beneath it. UNWIRED dead
//! code until a baseline/DFG emitter calls it (Rank-2 per-opcode encoders +
//! int32 fast paths).
//!
//! VALUE-REPRESENTATION SAFETY (the one corruption risk): the constants this
//! layer materializes into the tag registers and ORs into boxed int32 are the
//! SHARED [`crate::value::NUMBER_TAG`] / [`crate::value::NOT_CELL_MASK`] symbols
//! the runtime `value` module encodes/decodes with — never copied literals. A
//! JIT that boxed int32 against a different NumberTag than the runtime decodes
//! with would silently corrupt every value; referencing the one symbol makes
//! that drift impossible.
#![allow(dead_code)]

use crate::assembler::labels::Jump;
use crate::assembler::macro_assembler_arm64::{MacroAssemblerArm64, RelationalCondition};
use crate::assembler::operands::TrustedImm64;
use crate::assembler::registers::RegisterID;
use crate::value::{NOT_CELL_MASK, NUMBER_TAG};

/// `TagRegistersMode` (GPRInfo.h:430-434): whether the dedicated `numberTag`
/// (x27) / `notCellMask` (x28) registers are already materialized at this point.
/// `HaveTagRegisters` is the materialized fast path (compare/OR against the live
/// register); `DoNotHaveTagRegisters` re-materializes the constant inline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagRegistersMode {
    DoNotHaveTagRegisters,
    HaveTagRegisters,
}

/// `AssemblyHelpers : public MacroAssembler` (AssemblyHelpers.h:60), the
/// JSVALUE64 tag/box layer. Owns its `MacroAssemblerArm64` (the `MacroAssembler`
/// base) by composition.
#[derive(Default)]
pub struct AssemblyHelpers {
    masm: MacroAssemblerArm64,
}

impl AssemblyHelpers {
    /// `GPRInfo::numberTagRegister = ARM64Registers::x27` (GPRInfo.h:583). Holds
    /// `JSValue::NumberTag` once [`Self::emit_materialize_tag_check_registers`]
    /// has run.
    pub const NUMBER_TAG_REGISTER: RegisterID = RegisterID::X27;
    /// `GPRInfo::notCellMaskRegister = ARM64Registers::x28` (GPRInfo.h:584).
    /// Holds `JSValue::NotCellMask`.
    pub const NOT_CELL_MASK_REGISTER: RegisterID = RegisterID::X28;

    #[inline]
    pub fn new() -> Self {
        Self {
            masm: MacroAssemblerArm64::new(),
        }
    }

    /// The owned `MacroAssembler` base (read-only; e.g. for `code()`/`label()`).
    #[inline]
    pub fn masm(&self) -> &MacroAssemblerArm64 {
        &self.masm
    }

    /// The owned `MacroAssembler` base (mutable; for composite ops outside this
    /// JSValue-aware subset).
    #[inline]
    pub fn masm_mut(&mut self) -> &mut MacroAssemblerArm64 {
        &mut self.masm
    }

    /// The emitted machine-code bytes so far (delegates to the `MacroAssembler`
    /// base's `code()`).
    #[inline]
    pub fn code(&self) -> &[u8] {
        self.masm.code()
    }

    /// `emitMaterializeTagCheckRegisters()` (AssemblyHelpers.h:484-489, JSVALUE64):
    /// load the two dedicated tag-check registers.
    ///
    /// C++:
    /// ```cpp
    /// move(TrustedImm64(JSValue::NumberTag), numberTagRegister);          // x27
    /// or64(TrustedImm32(JSValue::OtherTag), numberTagRegister, notCellMaskRegister); // x28
    /// ```
    /// x27 := NumberTag, x28 := NumberTag | OtherTag == NotCellMask.
    ///
    /// DIVERGENCE (benign, instruction selection only): C++ computes x28 with an
    /// OR-immediate (`or64(OtherTag, x27, x28)`), whose single-instruction
    /// `LogicalImmediate` form is DEFERRED across this whole macro layer (see the
    /// `MacroAssemblerArm64` module note). This port materializes the identical
    /// [`NOT_CELL_MASK`] constant directly via `move`, producing the SAME x28
    /// register value. Both registers load the SHARED runtime constants, so the
    /// JIT's tag checks/boxing agree bit-for-bit with the runtime `value` module.
    pub fn emit_materialize_tag_check_registers(&mut self) {
        self.masm.move_imm64(
            TrustedImm64::new(NUMBER_TAG as i64),
            Self::NUMBER_TAG_REGISTER,
        );
        self.masm.move_imm64(
            TrustedImm64::new(NOT_CELL_MASK as i64),
            Self::NOT_CELL_MASK_REGISTER,
        );
    }

    /// `branchIfInt32(GPRReg, mode)` (AssemblyHelpers.h:773-784, JSVALUE64): a
    /// boxed int32 is `NumberTag | uint32`, so as an unsigned 64-bit value it is
    /// `>= NumberTag`; every other encoding is below it. Hence
    /// `branch64(AboveOrEqual, gpr, numberTag)`.
    pub fn branch_if_int32(&mut self, gpr: RegisterID, mode: TagRegistersMode) -> Jump {
        match mode {
            TagRegistersMode::HaveTagRegisters => self.masm.branch64(
                RelationalCondition::AboveOrEqual,
                gpr,
                Self::NUMBER_TAG_REGISTER,
            ),
            TagRegistersMode::DoNotHaveTagRegisters => {
                self.branch64_against_number_tag(RelationalCondition::AboveOrEqual, gpr)
            }
        }
    }

    /// `branchIfNotInt32(GPRReg, mode)` (AssemblyHelpers.h:794-805, JSVALUE64):
    /// the negation of [`Self::branch_if_int32`] —
    /// `branch64(Below, gpr, numberTag)`.
    pub fn branch_if_not_int32(&mut self, gpr: RegisterID, mode: TagRegistersMode) -> Jump {
        match mode {
            TagRegistersMode::HaveTagRegisters => {
                self.masm
                    .branch64(RelationalCondition::Below, gpr, Self::NUMBER_TAG_REGISTER)
            }
            TagRegistersMode::DoNotHaveTagRegisters => {
                self.branch64_against_number_tag(RelationalCondition::Below, gpr)
            }
        }
    }

    /// `boxInt32(intGPR, boxedRegs, mode)` (AssemblyHelpers.h:1710-1724,
    /// JSVALUE64): tag a raw int32 into a JSValue. `boxedRegs.gpr()` is the
    /// destination here.
    ///
    /// C++:
    /// ```cpp
    /// if (mode == DoNotHaveTagRegisters)
    ///     or64(TrustedImm64(JSValue::NumberTag), intGPR, boxedRegs.gpr());
    /// else
    ///     or64(numberTagRegister, intGPR, boxedRegs.gpr());
    /// ```
    /// This is the EXACT inverse of the runtime `JsValue::from_i32`
    /// (`NumberTag | uint32(value)`, repr.rs / JSCJSValue.h:1023-1026).
    pub fn box_int32(
        &mut self,
        int_gpr: RegisterID,
        result_gpr: RegisterID,
        mode: TagRegistersMode,
    ) {
        match mode {
            TagRegistersMode::HaveTagRegisters => {
                self.masm
                    .or64(Self::NUMBER_TAG_REGISTER, int_gpr, result_gpr);
            }
            TagRegistersMode::DoNotHaveTagRegisters => {
                // or64(TrustedImm64(NumberTag), intGPR, result): the NumberTag
                // immediate's single-instruction LogicalImmediate form is deferred
                // in this macro layer, so materialize NumberTag into the data temp
                // and register-OR. Identical result; identical NumberTag symbol.
                self.masm.move_imm64(
                    TrustedImm64::new(NUMBER_TAG as i64),
                    MacroAssemblerArm64::DATA_TEMP_REGISTER,
                );
                self.masm
                    .or64(MacroAssemblerArm64::DATA_TEMP_REGISTER, int_gpr, result_gpr);
            }
        }
    }

    /// `unboxInt32(boxed, dest)`: the inverse of [`Self::box_int32`]. JSVALUE64
    /// has NO dedicated `unboxInt32` helper in `AssemblyHelpers.h` — a boxed
    /// int32 is `NumberTag | uint32(value)` and `JSValue::asInt32()` is just
    /// `static_cast<int32_t>(asInt64)` (JSCJSValue.h:956-960), i.e. the low 32
    /// bits. So the machine-level unbox is `zeroExtend32ToWord(boxed, dest)` (a
    /// 32-bit `mov` that drops the high tag bits), matching `asInt32` exactly.
    pub fn unbox_int32(&mut self, boxed_gpr: RegisterID, result_gpr: RegisterID) {
        self.masm.zero_extend_32_to_word(boxed_gpr, result_gpr);
    }

    /// The `DoNotHaveTagRegisters` arm of `branchIfInt32`/`branchIfNotInt32`:
    /// `branch64(cond, gpr, TrustedImm64(JSValue::NumberTag))`
    /// (AssemblyHelpers.h:777/798). NumberTag is not a foldable add/sub
    /// immediate, so JSC (MacroAssemblerARM64.h:4742-4744) moves it into the data
    /// temp then compares register-register; this mirrors that lowering with the
    /// SHARED [`NUMBER_TAG`] constant.
    fn branch64_against_number_tag(&mut self, cond: RelationalCondition, gpr: RegisterID) -> Jump {
        self.masm.move_imm64(
            TrustedImm64::new(NUMBER_TAG as i64),
            MacroAssemblerArm64::DATA_TEMP_REGISTER,
        );
        self.masm
            .branch64(cond, gpr, MacroAssemblerArm64::DATA_TEMP_REGISTER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::arm64_encoder::Condition;
    use crate::assembler::AssemblerLabel;

    fn words(code: &[u8]) -> Vec<u32> {
        code.chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    // ------------------------------------------------------------------------
    // VALUE-REP CONSISTENCY: the tag constants this JIT layer emits MUST be the
    // SAME symbols the runtime `value` module encodes/decodes int32 with. The
    // helpers reference `crate::value::{NUMBER_TAG, NOT_CELL_MASK}` directly (no
    // copied literal), so this asserts the shared symbols' identity and exact
    // bit pattern (JSCJSValue.h:457,479). If these drift, boxed int32 silently
    // corrupts.
    // ------------------------------------------------------------------------
    #[test]
    fn tag_constants_are_the_shared_runtime_symbols() {
        // assembly_helpers references the very symbol used by JsValue::from_i32.
        assert_eq!(NUMBER_TAG, crate::value::NUMBER_TAG);
        assert_eq!(NOT_CELL_MASK, crate::value::NOT_CELL_MASK);
        // The faithful JSVALUE64 bit patterns (JSCJSValue.h:457,479).
        assert_eq!(NUMBER_TAG, 0xfffe_0000_0000_0000);
        assert_eq!(NOT_CELL_MASK, NUMBER_TAG | 0x2);
    }

    #[test]
    fn materialize_tag_check_registers_loads_shared_constants() {
        // x27 := NumberTag (0xfffe<<48) -> movz x27, #0xfffe, lsl #48 : 0xd2ffffdb
        // x28 := NotCellMask (0xfffe...0002) -> movz x28, #2 : 0xd280005c
        //                                       movk x28, #0xfffe, lsl #48 : 0xf2ffffdc
        let mut h = AssemblyHelpers::new();
        h.emit_materialize_tag_check_registers();
        assert_eq!(words(h.code()), vec![0xd2ff_ffdb, 0xd280_005c, 0xf2ff_ffdc]);
    }

    #[test]
    fn branch_if_int32_have_tag_registers() {
        // branch64(AboveOrEqual, x0, x27) -> cmp x0, x27 (subs xzr) : 0xeb1b001f ;
        // b.hs #0 : 0x54000002 ; nop.
        let mut h = AssemblyHelpers::new();
        let j = h.branch_if_int32(RegisterID::X0, TagRegistersMode::HaveTagRegisters);
        assert_eq!(words(h.code()), vec![0xeb1b_001f, 0x5400_0002, 0xd503_201f]);
        assert_eq!(j.condition(), Condition::Hs);
        assert_eq!(j.label().label(), AssemblerLabel(4));
    }

    #[test]
    fn branch_if_not_int32_have_tag_registers() {
        // branch64(Below, x0, x27) -> cmp x0, x27 : 0xeb1b001f ; b.lo #0 : 0x54000003 ; nop.
        let mut h = AssemblyHelpers::new();
        h.branch_if_not_int32(RegisterID::X0, TagRegistersMode::HaveTagRegisters);
        assert_eq!(words(h.code()), vec![0xeb1b_001f, 0x5400_0003, 0xd503_201f]);
    }

    #[test]
    fn branch_if_int32_without_tag_registers_materializes_number_tag() {
        // movz x16, #0xfffe, lsl #48 : 0xd2ffffd0 ;
        // cmp x0, x16 (subs xzr) : 0xeb10001f ; b.hs #0 : 0x54000002 ; nop.
        let mut h = AssemblyHelpers::new();
        h.branch_if_int32(RegisterID::X0, TagRegistersMode::DoNotHaveTagRegisters);
        assert_eq!(
            words(h.code()),
            vec![0xd2ff_ffd0, 0xeb10_001f, 0x5400_0002, 0xd503_201f]
        );
    }

    #[test]
    fn box_int32_have_tag_registers_ors_number_tag() {
        // boxInt32(x1 -> x0): or64(x27, x1, x0) -> orr x0, x27, x1 : 0xaa010360.
        let mut h = AssemblyHelpers::new();
        h.box_int32(
            RegisterID::X1,
            RegisterID::X0,
            TagRegistersMode::HaveTagRegisters,
        );
        assert_eq!(words(h.code()), vec![0xaa01_0360]);
    }

    #[test]
    fn box_int32_without_tag_registers_materializes_then_ors() {
        // movz x16, #0xfffe, lsl #48 : 0xd2ffffd0 ; orr x0, x16, x1 : 0xaa010200.
        let mut h = AssemblyHelpers::new();
        h.box_int32(
            RegisterID::X1,
            RegisterID::X0,
            TagRegistersMode::DoNotHaveTagRegisters,
        );
        assert_eq!(words(h.code()), vec![0xd2ff_ffd0, 0xaa01_0200]);
    }

    #[test]
    fn unbox_int32_reads_low_word() {
        // unboxInt32(x0 -> x1) == zeroExtend32ToWord(x0, x1) -> mov w1, w0 : 0x2a0003e1.
        let mut h = AssemblyHelpers::new();
        h.unbox_int32(RegisterID::X0, RegisterID::X1);
        assert_eq!(words(h.code()), vec![0x2a00_03e1]);
    }

    #[test]
    fn box_then_unbox_round_trips_the_tag() {
        // Boxing ORs NumberTag in; unboxing reads the low word back. The boxed
        // form ORs the SHARED NumberTag the runtime decodes with, so the emitted
        // box/unbox is the machine mirror of JsValue::from_i32 / asInt32.
        let mut h = AssemblyHelpers::new();
        h.box_int32(
            RegisterID::X1,
            RegisterID::X0,
            TagRegistersMode::HaveTagRegisters,
        );
        h.unbox_int32(RegisterID::X0, RegisterID::X2);
        // orr x0, x27, x1 ; mov w2, w0.
        assert_eq!(words(h.code()), vec![0xaa01_0360, 0x2a00_03e2]);
    }
}
