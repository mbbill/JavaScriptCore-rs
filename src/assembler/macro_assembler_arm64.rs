//! MacroAssemblerARM64 composite-operation layer (baseline-JIT subset).
//!
//! Faithful port of the per-opcode composite operations in
//! `Source/JavaScriptCore/assembler/MacroAssemblerARM64.h`: the methods a
//! baseline JIT calls (`add32`, `load64`, `branch32`, `jump`, ...). Each
//! composite lowers to one or more raw [`Arm64Encoder`] instruction-word emits,
//! exactly as JSC's `MacroAssemblerARM64` lowers to `m_assembler.<mnemonic>`
//! calls. The struct owns the code buffer (`m_assembler`'s `AssemblerBuffer`)
//! and hands out a transient [`Arm64Encoder`] per instruction (JSC's
//! `m_assembler.insn` -> `m_buffer.putInt`).
//!
//! Branches return the [`Jump`]/[`Label`]/[`Call`] offset tokens from
//! [`super::labels`]; the actual relative-displacement patching happens later in
//! [`super::link_records`] (already landed). This layer only EMITS the branch
//! placeholder words and captures the token.
//!
//! Scope: the baseline composites named in the port unit — integer arithmetic,
//! logic, shifts, multiply, negate; single-register loads/stores over `Address`
//! and `BaseIndex`; register/immediate/double moves and swap; and the
//! comparison/branch family. Pure-safe byte computation; UNWIRED dead code until
//! the baseline JIT (src/jit/arm64_baseline.rs) emits through it (serial step
//! B7).
//!
//! DEFERRED JSC behaviors (commented at each site, correctness-preserving, only
//! affecting code size / instruction choice, never the computed result):
//!   - the `CachedTempRegister` machinery that elides redundant scratch `move`s
//!     across consecutive ops (MacroAssemblerARM64.h:7559-7637 region); every
//!     op here invalidates and reloads the scratch, matching JSC's *single-op*
//!     output exactly.
//!   - the `LogicalImmediate` bitmask-immediate single-instruction forms for
//!     `and/or/xor/tst/move` (the `movi`/`orr (imm)` path) — replaced by the
//!     move-to-scratch fallback, which JSC also uses when the bitmask form is
//!     invalid.
//!   - the compare-and-branch (`cbz`/`cbnz`) / test-bit (`tbz`/`tbnz`) folds in
//!     `branch*`/`branchTest*` — JSC's `makeCompareAndBranch`/`makeTestBitAndBranch`
//!     /`attemptToFoldToBitTest*`; these need the two-word expansion link forms
//!     that [`super::link_records`] also defers. The faithful `cmp`/`tst` +
//!     `b.cond` form is emitted instead.
#![allow(dead_code)]

use super::arm64_encoder::{
    AddOp, Arm64Encoder, Condition, DataOp2Source, Datasize, LogicalOp, MemOp, MemOpSize,
    MoveWideOp, SetFlags,
};
use super::labels::{Call, CallFlags, Jump, Label};
use super::link_records::JumpType;
use super::operands::{Address, BaseIndex, Extend, Scale, TrustedImm32, TrustedImm64};
use super::registers::{FPRegisterID, RegisterID};
use super::AssemblerLabel;

/// `AbstractMacroAssembler<ARM64Assembler>::RelationalCondition` as specialized
/// by `MacroAssemblerARM64` (MacroAssemblerARM64.h:111-122). Each variant's
/// value IS the ARM64 `Condition` it maps to via `ARM64Condition`
/// (MacroAssemblerARM64.h:7543-7546, a bare `static_cast`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationalCondition {
    Equal,
    NotEqual,
    Above,
    AboveOrEqual,
    Below,
    BelowOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl RelationalCondition {
    /// `ARM64Condition(RelationalCondition)` (MacroAssemblerARM64.h:7543): the
    /// enum value reinterpreted as the ARM64 `Condition`.
    #[inline]
    pub const fn arm64_condition(self) -> Condition {
        match self {
            RelationalCondition::Equal => Condition::Eq,
            RelationalCondition::NotEqual => Condition::Ne,
            RelationalCondition::Above => Condition::Hi,
            RelationalCondition::AboveOrEqual => Condition::Hs,
            RelationalCondition::Below => Condition::Lo,
            RelationalCondition::BelowOrEqual => Condition::Ls,
            RelationalCondition::GreaterThan => Condition::Gt,
            RelationalCondition::GreaterThanOrEqual => Condition::Ge,
            RelationalCondition::LessThan => Condition::Lt,
            RelationalCondition::LessThanOrEqual => Condition::Le,
        }
    }
}

/// `MacroAssemblerARM64::ResultCondition` (MacroAssemblerARM64.h:124-131): the
/// flag-result conditions branched on after a flag-setting op.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultCondition {
    Carry,
    Overflow,
    Signed,
    PositiveOrZero,
    Zero,
    NonZero,
}

impl ResultCondition {
    /// `ARM64Condition(ResultCondition)` (MacroAssemblerARM64.h:7548).
    #[inline]
    pub const fn arm64_condition(self) -> Condition {
        match self {
            ResultCondition::Carry => Condition::Hs, // ConditionCS == ConditionHS
            ResultCondition::Overflow => Condition::Vs,
            ResultCondition::Signed => Condition::Mi,
            ResultCondition::PositiveOrZero => Condition::Pl,
            ResultCondition::Zero => Condition::Eq,
            ResultCondition::NonZero => Condition::Ne,
        }
    }
}

/// `MacroAssemblerARM64`, the composite-op layer over [`Arm64Encoder`].
///
/// Owns the code buffer (the moral equivalent of `ARM64Assembler::m_buffer`).
/// `dataTempRegister`/`memoryTempRegister` (`ip0`/`ip1`) are the two scratch
/// GPRs JSC reserves for the MacroAssembler (MacroAssemblerARM64.h:56-57).
#[derive(Default)]
pub struct MacroAssemblerArm64 {
    buffer: Vec<u8>,
}

impl MacroAssemblerArm64 {
    /// `dataTempRegister = ARM64Registers::ip0` (MacroAssemblerARM64.h:56).
    pub const DATA_TEMP_REGISTER: RegisterID = RegisterID::IP0;
    /// `memoryTempRegister = ARM64Registers::ip1` (MacroAssemblerARM64.h:57).
    pub const MEMORY_TEMP_REGISTER: RegisterID = RegisterID::IP1;

    #[inline]
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// The emitted machine-code bytes so far (`ARM64Assembler::buffer()`).
    #[inline]
    pub fn code(&self) -> &[u8] {
        &self.buffer
    }

    /// Current code-buffer byte offset as an [`AssemblerLabel`]
    /// (`ARM64Assembler::label()`).
    #[inline]
    fn current_label(&self) -> AssemblerLabel {
        AssemblerLabel(self.buffer.len() as u32)
    }

    /// `label()` (AbstractMacroAssembler.h:466-472): a destination point in the
    /// instruction stream.
    #[inline]
    pub fn label(&self) -> Label {
        Label::new(self.current_label())
    }

    /// Borrow a transient encoder over the buffer (JSC's `m_assembler`).
    #[inline]
    fn asm(&mut self) -> Arm64Encoder<'_> {
        Arm64Encoder::new(&mut self.buffer)
    }

    // ========================================================================
    // Integer arithmetic — add/sub.
    // ========================================================================

    /// `add32(a, b, dest)` (MacroAssemblerARM64.h:165-171). `sp` may appear at
    /// most once; if it is `b`, swap so it lands in the `rn` slot of `add`.
    pub fn add32(&mut self, a: RegisterID, b: RegisterID, dest: RegisterID) {
        let (a, b) = if b.is_sp() { (b, a) } else { (a, b) };
        self.asm().emit_add_sub_reg(
            Datasize::D32,
            AddOp::Add,
            SetFlags::DontSetFlags,
            dest,
            a,
            b,
        );
    }

    /// `add32(src, dest)` (MacroAssemblerARM64.h:173-179).
    pub fn add32_rr(&mut self, src: RegisterID, dest: RegisterID) {
        if src.is_sp() {
            self.asm().emit_add_sub_reg(
                Datasize::D32,
                AddOp::Add,
                SetFlags::DontSetFlags,
                dest,
                src,
                dest,
            );
        } else {
            self.asm().emit_add_sub_reg(
                Datasize::D32,
                AddOp::Add,
                SetFlags::DontSetFlags,
                dest,
                dest,
                src,
            );
        }
    }

    /// `add32(TrustedImm32 imm, src, dest)` (MacroAssemblerARM64.h:191-210):
    /// fold to an add/sub immediate when the value fits a (possibly shifted,
    /// possibly negated) 12-bit field, else materialize via the scratch.
    pub fn add32_imm(&mut self, imm: TrustedImm32, src: RegisterID, dest: RegisterID) {
        self.add_sub_imm32(AddOp::Add, SetFlags::DontSetFlags, imm, src, dest);
    }

    /// `add64(a, b, dest)` (MacroAssemblerARM64.h:276-282).
    pub fn add64(&mut self, a: RegisterID, b: RegisterID, dest: RegisterID) {
        let (a, b) = if b.is_sp() { (b, a) } else { (a, b) };
        self.asm().emit_add_sub_reg(
            Datasize::D64,
            AddOp::Add,
            SetFlags::DontSetFlags,
            dest,
            a,
            b,
        );
    }

    /// `add64(TrustedImm32 imm, src, dest)` (MacroAssemblerARM64.h:354-364):
    /// fold to add/sub immediate, else sign-extend into the scratch and add.
    pub fn add64_imm(&mut self, imm: TrustedImm32, src: RegisterID, dest: RegisterID) {
        self.add_sub_imm64(AddOp::Add, SetFlags::DontSetFlags, imm, src, dest);
    }

    /// `sub32(left, right, dest)` (MacroAssemblerARM64.h:1493-1496).
    pub fn sub32(&mut self, left: RegisterID, right: RegisterID, dest: RegisterID) {
        self.asm().emit_add_sub_reg(
            Datasize::D32,
            AddOp::Sub,
            SetFlags::DontSetFlags,
            dest,
            left,
            right,
        );
    }

    /// `sub32(src, dest)` (MacroAssemblerARM64.h:1488-1491).
    pub fn sub32_rr(&mut self, src: RegisterID, dest: RegisterID) {
        self.sub32(dest, src, dest);
    }

    /// `sub32(left, TrustedImm32 imm, dest)` (MacroAssemblerARM64.h:1503-1516):
    /// the inverse of `add32_imm` — a foldable immediate becomes a `sub`/`add`
    /// immediate (inverted), else move-to-scratch then `sub` register.
    pub fn sub32_imm(&mut self, left: RegisterID, imm: TrustedImm32, dest: RegisterID) {
        self.add_sub_imm32(AddOp::Sub, SetFlags::DontSetFlags, imm, left, dest);
    }

    /// `sub64(left, right, dest)` (the 64-bit analogue of sub32).
    pub fn sub64(&mut self, left: RegisterID, right: RegisterID, dest: RegisterID) {
        self.asm().emit_add_sub_reg(
            Datasize::D64,
            AddOp::Sub,
            SetFlags::DontSetFlags,
            dest,
            left,
            right,
        );
    }

    /// `neg32(src, dest)` (MacroAssemblerARM64.h:1233-1236):
    /// `neg<32> = sub<32>(dest, zr, src)`.
    pub fn neg32(&mut self, src: RegisterID, dest: RegisterID) {
        self.asm().emit_add_sub_reg(
            Datasize::D32,
            AddOp::Sub,
            SetFlags::DontSetFlags,
            dest,
            RegisterID::Zr,
            src,
        );
    }

    /// `neg32(dest)` (MacroAssemblerARM64.h:1228-1231).
    pub fn neg32_in_place(&mut self, dest: RegisterID) {
        self.neg32(dest, dest);
    }

    // ========================================================================
    // Integer logic — and/or/xor (register forms).
    // ========================================================================

    /// `and32(op1, op2, dest)` (MacroAssemblerARM64.h:471-474).
    pub fn and32(&mut self, op1: RegisterID, op2: RegisterID, dest: RegisterID) {
        self.asm()
            .emit_logical_reg(Datasize::D32, LogicalOp::And, dest, op1, op2);
    }

    /// `and32(src, dest)` (MacroAssemblerARM64.h:461-464).
    pub fn and32_rr(&mut self, src: RegisterID, dest: RegisterID) {
        self.and32(dest, src, dest);
    }

    /// `or32(op1, op2, dest)` (MacroAssemblerARM64.h:1274-1277).
    pub fn or32(&mut self, op1: RegisterID, op2: RegisterID, dest: RegisterID) {
        self.asm()
            .emit_logical_reg(Datasize::D32, LogicalOp::Orr, dest, op1, op2);
    }

    /// `or32(src, dest)` (MacroAssemblerARM64.h:1269-1272).
    pub fn or32_rr(&mut self, src: RegisterID, dest: RegisterID) {
        self.or32(dest, src, dest);
    }

    /// `xor32(op1, op2, dest)` (MacroAssemblerARM64.h:1679-1682).
    pub fn xor32(&mut self, op1: RegisterID, op2: RegisterID, dest: RegisterID) {
        self.asm()
            .emit_logical_reg(Datasize::D32, LogicalOp::Eor, dest, op1, op2);
    }

    /// `xor32(src, dest)` (MacroAssemblerARM64.h:1668-1671).
    pub fn xor32_rr(&mut self, src: RegisterID, dest: RegisterID) {
        self.xor32(dest, src, dest);
    }

    // ========================================================================
    // Shifts (register-amount and immediate-amount).
    // ========================================================================

    /// `lshift32(src, shiftAmount, dest)` (MacroAssemblerARM64.h:1024-1027):
    /// `lsl<32> = lslv<32>`.
    pub fn lshift32(&mut self, src: RegisterID, shift_amount: RegisterID, dest: RegisterID) {
        self.asm()
            .emit_shift_reg(Datasize::D32, DataOp2Source::Lslv, dest, src, shift_amount);
    }

    /// `lshift32(src, TrustedImm32 imm, dest)` (MacroAssemblerARM64.h:1029-1032):
    /// immediate left shift, amount masked to 5 bits.
    pub fn lshift32_imm(&mut self, src: RegisterID, imm: TrustedImm32, dest: RegisterID) {
        self.asm()
            .emit_lsl_imm(Datasize::D32, dest, src, (imm.value as u32) & 0x1f);
    }

    /// `rshift32(src, shiftAmount, dest)` (MacroAssemblerARM64.h:1440-1443):
    /// `asr<32> = asrv<32>` (arithmetic).
    pub fn rshift32(&mut self, src: RegisterID, shift_amount: RegisterID, dest: RegisterID) {
        self.asm()
            .emit_shift_reg(Datasize::D32, DataOp2Source::Asrv, dest, src, shift_amount);
    }

    /// `rshift32(src, TrustedImm32 imm, dest)` (MacroAssemblerARM64.h:1445-1448).
    pub fn rshift32_imm(&mut self, src: RegisterID, imm: TrustedImm32, dest: RegisterID) {
        self.asm()
            .emit_asr_imm(Datasize::D32, dest, src, (imm.value as u32) & 0x1f);
    }

    /// `urshift32(src, shiftAmount, dest)` (MacroAssemblerARM64.h:1620-1623):
    /// `lsr<32> = lsrv<32>` (logical).
    pub fn urshift32(&mut self, src: RegisterID, shift_amount: RegisterID, dest: RegisterID) {
        self.asm()
            .emit_shift_reg(Datasize::D32, DataOp2Source::Lsrv, dest, src, shift_amount);
    }

    /// `urshift32(src, TrustedImm32 imm, dest)`
    /// (MacroAssemblerARM64.h:1625-1628).
    pub fn urshift32_imm(&mut self, src: RegisterID, imm: TrustedImm32, dest: RegisterID) {
        self.asm()
            .emit_lsr_imm(Datasize::D32, dest, src, (imm.value as u32) & 0x1f);
    }

    // ========================================================================
    // Multiply.
    // ========================================================================

    /// `mul32(left, right, dest)` (MacroAssemblerARM64.h:1090-1093).
    pub fn mul32(&mut self, left: RegisterID, right: RegisterID, dest: RegisterID) {
        self.asm().emit_mul(Datasize::D32, dest, left, right);
    }

    /// `mul32(src, dest)` (MacroAssemblerARM64.h:1095-1098).
    pub fn mul32_rr(&mut self, src: RegisterID, dest: RegisterID) {
        self.asm().emit_mul(Datasize::D32, dest, dest, src);
    }

    // ========================================================================
    // Moves.
    // ========================================================================

    /// `move(src, dest)` (MacroAssemblerARM64.h:4277-4285).
    pub fn move_rr(&mut self, src: RegisterID, dest: RegisterID) {
        if src == dest {
            return;
        }
        if src.is_zr() && !dest.is_sp() {
            self.asm()
                .emit_move_wide(Datasize::D64, MoveWideOp::Z, dest, 0, 0);
        } else {
            self.asm().emit_mov_reg(Datasize::D64, dest, src);
        }
    }

    /// `move(TrustedImm32 imm, dest)` (MacroAssemblerARM64.h:4287-4290) ->
    /// `moveInternal<TrustedImm32, int32_t>` (MacroAssemblerARM64.h:7581).
    pub fn move_imm32(&mut self, imm: TrustedImm32, dest: RegisterID) {
        self.move_internal(imm.value as u32 as u64, Datasize::D32, dest);
    }

    /// `move(TrustedImm64 imm, dest)` (MacroAssemblerARM64.h:4297-4300) ->
    /// `moveInternal<TrustedImm64, int64_t>`.
    pub fn move_imm64(&mut self, imm: TrustedImm64, dest: RegisterID) {
        self.move_internal(imm.value as u64, Datasize::D64, dest);
    }

    /// `moveDouble(src, dest)` (MacroAssemblerARM64.h:3254-3258): `fmov<64>`.
    pub fn move_double(&mut self, src: FPRegisterID, dest: FPRegisterID) {
        if src != dest {
            self.asm().emit_fmov_double(dest, src);
        }
    }

    /// `swap(reg1, reg2)` (MacroAssemblerARM64.h:4302-4309): via the data temp.
    pub fn swap(&mut self, reg1: RegisterID, reg2: RegisterID) {
        if reg1 == reg2 {
            return;
        }
        self.move_rr(reg1, Self::DATA_TEMP_REGISTER);
        self.move_rr(reg2, reg1);
        self.move_rr(Self::DATA_TEMP_REGISTER, reg2);
    }

    // ========================================================================
    // Loads / stores over Address and BaseIndex.
    // ========================================================================

    /// `load64(Address, dest)` (MacroAssemblerARM64.h:1807-1814).
    pub fn load64(&mut self, address: Address, dest: RegisterID) {
        self.load_store_address(MemOpSize::Size64, MemOp::Load, dest, address);
    }

    /// `load32(Address, dest)` (MacroAssemblerARM64.h:2013-2020).
    pub fn load32(&mut self, address: Address, dest: RegisterID) {
        self.load_store_address(MemOpSize::Size32, MemOp::Load, dest, address);
    }

    /// `load8(Address, dest)` (MacroAssemblerARM64.h:2215-2222).
    pub fn load8(&mut self, address: Address, dest: RegisterID) {
        self.load_store_address(MemOpSize::Size8Or128, MemOp::Load, dest, address);
    }

    /// `store64(src, Address)` (MacroAssemblerARM64.h:2333-2340).
    pub fn store64(&mut self, src: RegisterID, address: Address) {
        self.load_store_address(MemOpSize::Size64, MemOp::Store, src, address);
    }

    /// `store32(src, Address)` (MacroAssemblerARM64.h:2608-2616).
    pub fn store32(&mut self, src: RegisterID, address: Address) {
        self.load_store_address(MemOpSize::Size32, MemOp::Store, src, address);
    }

    /// `store8(src, Address)` (MacroAssemblerARM64.h store8 family).
    pub fn store8(&mut self, src: RegisterID, address: Address) {
        self.load_store_address(MemOpSize::Size8Or128, MemOp::Store, src, address);
    }

    /// `load64(BaseIndex, dest)` (MacroAssemblerARM64.h:1816-1828).
    pub fn load64_indexed(&mut self, address: BaseIndex, dest: RegisterID) {
        self.load_store_base_index(MemOpSize::Size64, MemOp::Load, dest, address);
    }

    /// `load32(BaseIndex, dest)` (MacroAssemblerARM64.h:2022-2034).
    pub fn load32_indexed(&mut self, address: BaseIndex, dest: RegisterID) {
        self.load_store_base_index(MemOpSize::Size32, MemOp::Load, dest, address);
    }

    /// `load8(BaseIndex, dest)` (MacroAssemblerARM64.h:2224-2236).
    pub fn load8_indexed(&mut self, address: BaseIndex, dest: RegisterID) {
        self.load_store_base_index(MemOpSize::Size8Or128, MemOp::Load, dest, address);
    }

    /// `store64(src, BaseIndex)` (MacroAssemblerARM64.h:2342-2354).
    pub fn store64_indexed(&mut self, src: RegisterID, address: BaseIndex) {
        self.load_store_base_index(MemOpSize::Size64, MemOp::Store, src, address);
    }

    /// `store32(src, BaseIndex)` (MacroAssemblerARM64.h:2617-2629).
    pub fn store32_indexed(&mut self, src: RegisterID, address: BaseIndex) {
        self.load_store_base_index(MemOpSize::Size32, MemOp::Store, src, address);
    }

    /// `store8(src, BaseIndex)` (MacroAssemblerARM64.h:2742+).
    pub fn store8_indexed(&mut self, src: RegisterID, address: BaseIndex) {
        self.load_store_base_index(MemOpSize::Size8Or128, MemOp::Store, src, address);
    }

    // ========================================================================
    // Comparison / branch family.
    // ========================================================================

    /// `branch32(cond, left, right)` (MacroAssemblerARM64.h:4616-4620):
    /// `cmp<32>(left, right)` then a conditional branch.
    pub fn branch32(
        &mut self,
        cond: RelationalCondition,
        left: RegisterID,
        right: RegisterID,
    ) -> Jump {
        self.cmp(Datasize::D32, left, right);
        self.make_branch(cond.arm64_condition())
    }

    /// `branch32(cond, left, TrustedImm32 right)` (MacroAssemblerARM64.h:
    /// 4622-4645), the common non-folded path: a foldable immediate becomes
    /// `cmp`/`cmn` immediate, else move-to-scratch then `cmp` register.
    ///
    /// DEFERRED: the `attemptToFoldToBitTest32` / `commuteCompareToZeroIntoTest`
    /// folds (which would emit `tbz`/`tbnz` or `tst` for power-of-two / zero
    /// immediates) — see the module note.
    pub fn branch32_imm(
        &mut self,
        cond: RelationalCondition,
        left: RegisterID,
        right: TrustedImm32,
    ) -> Jump {
        self.cmp_imm(Datasize::D32, left, right);
        self.make_branch(cond.arm64_condition())
    }

    /// `branch64(cond, left, right)` (MacroAssemblerARM64.h:4683-4697), the
    /// non-`sp`-right path: `cmp<64>` then a conditional branch.
    pub fn branch64(
        &mut self,
        cond: RelationalCondition,
        left: RegisterID,
        right: RegisterID,
    ) -> Jump {
        // `sp` cannot be a register operand of our shifted-register `cmp` (it
        // would silently encode XZR -> a compare against zero). JSC
        // (MacroAssemblerARM64.h:4683-4697) swaps an Equal sp-right into the cmp
        // LEFT and otherwise moves it to the data temp, relying on `cmp`'s
        // EXTENDED-register form to accept an sp LEFT operand. Our `cmp` lacks
        // that extended form yet (deferred encoder batch), so we materialize a
        // right==sp operand into the data temp here for EVERY condition -- always
        // correct; the only divergence is instruction selection on this cold
        // sp-operand path. A left==sp operand is caught by `cmp`'s debug_assert
        // until the extended-register cmp lands.
        let right = if right.is_sp() {
            self.move_rr(right, Self::DATA_TEMP_REGISTER);
            Self::DATA_TEMP_REGISTER
        } else {
            right
        };
        self.cmp(Datasize::D64, left, right);
        self.make_branch(cond.arm64_condition())
    }

    /// `branchPtr(cond, left, right)` (MacroAssemblerARM64.h:4797-4800):
    /// pointer-width comparison == `branch64`.
    pub fn branch_ptr(
        &mut self,
        cond: RelationalCondition,
        left: RegisterID,
        right: RegisterID,
    ) -> Jump {
        self.branch64(cond, left, right)
    }

    /// `branchTest32(cond, reg, mask)` (MacroAssemblerARM64.h:4844-4850), the
    /// general `tst<32>(reg, mask)` + conditional-branch form.
    ///
    /// DEFERRED: the `reg == mask` + `Zero`/`NonZero` fold to `cbz`/`cbnz`
    /// (`makeCompareAndBranch`) — see the module note.
    pub fn branch_test32(
        &mut self,
        cond: ResultCondition,
        reg: RegisterID,
        mask: RegisterID,
    ) -> Jump {
        self.asm()
            .emit_logical_reg(Datasize::D32, LogicalOp::Ands, RegisterID::Zr, reg, mask);
        self.make_branch(cond.arm64_condition())
    }

    /// `branchAdd32(cond, op1, op2, dest)` (MacroAssemblerARM64.h:5079-5083):
    /// `add<32, S>(dest, op1, op2)` (flag-setting) then a conditional branch.
    pub fn branch_add32(
        &mut self,
        cond: ResultCondition,
        op1: RegisterID,
        op2: RegisterID,
        dest: RegisterID,
    ) -> Jump {
        self.asm()
            .emit_add_sub_reg(Datasize::D32, AddOp::Add, SetFlags::S, dest, op1, op2);
        self.make_branch(cond.arm64_condition())
    }

    /// `branchSub32(cond, op1, op2, dest)` (MacroAssemblerARM64.h:5268-5272):
    /// `sub<32, S>(dest, op1, op2)` then a conditional branch.
    pub fn branch_sub32(
        &mut self,
        cond: ResultCondition,
        op1: RegisterID,
        op2: RegisterID,
        dest: RegisterID,
    ) -> Jump {
        self.asm()
            .emit_add_sub_reg(Datasize::D32, AddOp::Sub, SetFlags::S, dest, op1, op2);
        self.make_branch(cond.arm64_condition())
    }

    /// `jump()` (MacroAssemblerARM64.h:5390-5395): an unconditional `b`
    /// placeholder. The label is captured at the `b` (before emit), so the link
    /// pass patches the branch in place.
    pub fn jump(&mut self) -> Jump {
        let label = self.current_label();
        self.asm().emit_b(0);
        Jump::new(label, JumpType::JumpNoCondition, Condition::Invalid)
    }

    /// `nearCall()` (MacroAssemblerARM64.h:5434-5439): an unconditional `bl`
    /// (near call) placeholder; the displacement is resolved at link time.
    ///
    /// Divergence (commented per contract): JSC captures the label AFTER the
    /// `bl` (watchpoint/compaction bookkeeping). This port captures the `bl`'s
    /// own offset to match the `from == bl-offset` convention the landed
    /// [`super::link_records`] call-relocation pass uses (same choice as
    /// [`Self::jump`]). The temp-register invalidation JSC does here is the
    /// deferred caching machinery (see the module note).
    pub fn near_call(&mut self) -> Call {
        let label = self.current_label();
        self.asm().emit_bl(0);
        Call::new(label, CallFlags::LINKABLE_NEAR)
    }

    /// `ret()` (MacroAssemblerARM64.h:5462-5465).
    pub fn ret(&mut self) {
        self.asm().emit_ret();
    }

    /// `nop()` (MacroAssemblerARM64.h:5878-5881).
    pub fn nop(&mut self) {
        self.asm().emit_nop();
    }

    // ========================================================================
    // Private helpers — faithful ports of the MacroAssemblerARM64 internals.
    // ========================================================================

    /// `cmp<datasize>(rn, rm)` (ARM64Assembler.h:1073-1077):
    /// `sub<datasize, S>(zr, rn, rm)`.
    fn cmp(&mut self, ds: Datasize, rn: RegisterID, rm: RegisterID) {
        // The shifted-register form encodes operand 31 as XZR, so an `sp`
        // operand here would silently compare against zero. JSC's `cmp` accepts
        // an sp LEFT operand via the extended-register form (UXTX #0); that
        // encoder path is a deferred batch, so callers must materialize an sp
        // operand into a scratch first (branch64 does). Guard the invariant.
        debug_assert!(
            !rn.is_sp() && !rm.is_sp(),
            "cmp needs the extended-register form for sp operands (deferred); \
             materialize sp into a scratch before cmp"
        );
        self.asm()
            .emit_add_sub_reg(ds, AddOp::Sub, SetFlags::S, RegisterID::Zr, rn, rm);
    }

    /// `cmp`/`cmn<datasize>` immediate (MacroAssemblerARM64.h:4634-4643): a
    /// foldable immediate becomes `cmp` (or `cmn` when negated), else
    /// move-to-scratch then `cmp` register.
    fn cmp_imm(&mut self, ds: Datasize, left: RegisterID, imm: TrustedImm32) {
        if let Some((u12, shift12, inverted)) = try_extract_shifted_imm32(imm.value) {
            // !inverted -> cmp (sub S, zr); inverted -> cmn (add S, zr).
            let op = if inverted { AddOp::Add } else { AddOp::Sub };
            self.asm()
                .emit_add_sub_imm(ds, op, SetFlags::S, RegisterID::Zr, left, u12, shift12);
        } else {
            self.move_into_scratch(ds, imm, Self::DATA_TEMP_REGISTER);
            self.cmp(ds, left, Self::DATA_TEMP_REGISTER);
        }
    }

    /// The shared `add32/sub32(imm)` lowering (MacroAssemblerARM64.h:191-210 /
    /// 1503-1516). `op` is the nominal operation; a foldable immediate uses the
    /// add/sub immediate form, inverting `op` when the value was negated; a
    /// non-foldable immediate is moved into the data temp and applied as a
    /// register op.
    fn add_sub_imm32(
        &mut self,
        op: AddOp,
        s: SetFlags,
        imm: TrustedImm32,
        src: RegisterID,
        dest: RegisterID,
    ) {
        if let Some((u12, shift12, inverted)) = try_extract_shifted_imm32(imm.value) {
            let op = if inverted { invert_add_op(op) } else { op };
            self.asm()
                .emit_add_sub_imm(Datasize::D32, op, s, dest, src, u12, shift12);
            return;
        }
        // Large immediate. JSC `add32(imm, src, dest)` with src != dest
        // (MacroAssemblerARM64.h:203-206) materializes the immediate into DEST
        // then `add32(src, dest)` -- it does NOT touch the data temp. Only the
        // src == dest case (:207-209) and EVERY sub32 (:1513) route through the
        // data temp. The shared helper must honor that split, else it emits
        // non-faithful register fields for the routine `dest = src + bigConst`.
        if matches!(op, AddOp::Add) && src != dest {
            self.move_imm32(imm, dest);
            // add32(src, dest) -- non-flag-setting register add (s is always
            // DontSetFlags on this Add path; the flag-setting immediate add has
            // its own JSC impl, not this shared helper).
            self.add32_rr(src, dest);
        } else {
            self.move_imm32(imm, Self::DATA_TEMP_REGISTER);
            self.asm()
                .emit_add_sub_reg(Datasize::D32, op, s, dest, src, Self::DATA_TEMP_REGISTER);
        }
    }

    /// The 64-bit `add64/sub64(TrustedImm32)` lowering
    /// (MacroAssemblerARM64.h:354-364): foldable immediate -> add/sub immediate;
    /// else sign-extend the 32-bit immediate into the data temp and add register.
    fn add_sub_imm64(
        &mut self,
        op: AddOp,
        s: SetFlags,
        imm: TrustedImm32,
        src: RegisterID,
        dest: RegisterID,
    ) {
        if let Some((u12, shift12, inverted)) = try_extract_shifted_imm32(imm.value) {
            let op = if inverted { invert_add_op(op) } else { op };
            self.asm()
                .emit_add_sub_imm(Datasize::D64, op, s, dest, src, u12, shift12);
            return;
        }
        // signExtend32ToPtr(imm, dataTemp) == move(TrustedImm64(imm), dataTemp).
        self.move_imm64(
            TrustedImm64::new(imm.value as i64),
            Self::DATA_TEMP_REGISTER,
        );
        self.asm()
            .emit_add_sub_reg(Datasize::D64, op, s, dest, src, Self::DATA_TEMP_REGISTER);
    }

    /// Move an immediate into the scratch for a register-form fallback. For a
    /// 32-bit compare this is `move(TrustedImm32)`; for 64-bit, JSC sign-extends
    /// (`moveToCachedReg(TrustedImm32, dataMemoryTempRegister)` widens to 64).
    fn move_into_scratch(&mut self, ds: Datasize, imm: TrustedImm32, dest: RegisterID) {
        match ds {
            Datasize::D64 => self.move_imm64(TrustedImm64::new(imm.value as i64), dest),
            _ => self.move_imm32(imm, dest),
        }
    }

    /// `moveInternal<ImmediateType, rawType>(imm, dest)`
    /// (MacroAssemblerARM64.h:7581-7637): 0 -> `movz #0`; all-ones -> `movn #0`;
    /// else movz/movn voting over the halfwords with `movk` for the rest.
    ///
    /// DEFERRED: the `LogicalImmediate` `movi` single-instruction path
    /// (MacroAssemblerARM64.h:7598-7603); for bitmask-immediate values JSC emits
    /// one `movi`, this emits the equivalent movz/movk sequence (same value).
    fn move_internal(&mut self, raw: u64, ds: Datasize, dest: RegisterID) {
        let num_half = if matches!(ds, Datasize::D64) { 4 } else { 2 };
        let mask = if num_half == 4 { u64::MAX } else { 0xffff_ffff };
        let value = raw & mask;

        if value == 0 {
            self.asm().emit_move_wide(ds, MoveWideOp::Z, dest, 0, 0);
            return;
        }
        if value == mask {
            self.asm().emit_move_wide(ds, MoveWideOp::N, dest, 0, 0);
            return;
        }

        let halfwords: [u16; 4] = [
            (value & 0xffff) as u16,
            ((value >> 16) & 0xffff) as u16,
            ((value >> 32) & 0xffff) as u16,
            ((value >> 48) & 0xffff) as u16,
        ];

        let mut zero_or_negate_vote: i32 = 0;
        for &h in &halfwords[..num_half] {
            if h == 0 {
                zero_or_negate_vote += 1;
            } else if h == 0xffff {
                zero_or_negate_vote -= 1;
            }
        }

        let mut need_clear = true;
        if zero_or_negate_vote >= 0 {
            for (i, &h) in halfwords[..num_half].iter().enumerate() {
                if h != 0 {
                    let shift = 16 * i as u32;
                    if need_clear {
                        self.asm().emit_move_wide(ds, MoveWideOp::Z, dest, h, shift);
                        need_clear = false;
                    } else {
                        self.asm().emit_move_wide(ds, MoveWideOp::K, dest, h, shift);
                    }
                }
            }
        } else {
            for (i, &h) in halfwords[..num_half].iter().enumerate() {
                if h != 0xffff {
                    let shift = 16 * i as u32;
                    if need_clear {
                        self.asm()
                            .emit_move_wide(ds, MoveWideOp::N, dest, !h, shift);
                        need_clear = false;
                    } else {
                        self.asm().emit_move_wide(ds, MoveWideOp::K, dest, h, shift);
                    }
                }
            }
        }
    }

    /// `makeBranch(condition)` (MacroAssemblerARM64.h:7501-7508): emit a
    /// `b.cond` placeholder, capture the jump label, then emit a reserved `nop`
    /// (the second word of the `JumpCondition` form, used by the indirect
    /// expansion at link time).
    ///
    /// Divergence (commented per contract): JSC captures the label AFTER the
    /// `b.cond` (its compaction pass keys off the second slot). This port keys
    /// off the `b.cond` itself so the compaction-deferred direct link pass in
    /// [`super::link_records`] patches it in place; reconcile when jump
    /// compaction lands. The reserved `nop` is still emitted faithfully.
    fn make_branch(&mut self, condition: Condition) -> Jump {
        let label = self.current_label();
        self.asm().emit_b_cond(condition, 0);
        self.asm().emit_nop();
        Jump::new(label, JumpType::JumpCondition, condition)
    }

    /// `indexExtendType(BaseIndex)` (MacroAssemblerARM64.h:1794-1805): map the
    /// operand `Extend` to the ARM64 register-offset extend.
    fn index_extend_type(extend: Extend) -> super::arm64_encoder::ExtendType {
        use super::arm64_encoder::ExtendType;
        match extend {
            Extend::ZExt32 => ExtendType::Uxtw,
            Extend::SExt32 => ExtendType::Sxtw,
            Extend::None => ExtendType::Uxtx,
        }
    }

    /// Shared `load*/store*(Address)` lowering (MacroAssemblerARM64.h:1807-1814
    /// and siblings): try the unscaled signed-imm9 (`ldur`/`stur`) form, then
    /// the unsigned scaled-imm12 (`ldr`/`str`) form, else materialize the
    /// sign-extended offset in the memory temp and use the register-offset form.
    fn load_store_address(&mut self, size: MemOpSize, op: MemOp, rt: RegisterID, address: Address) {
        let offset = address.offset;
        if is_valid_signed_imm9(offset) {
            self.asm()
                .emit_load_store_unscaled(size, op, rt, address.base, offset);
            return;
        }
        if is_valid_scaled_uimm12(size, offset) {
            self.asm()
                .emit_load_store_unsigned(size, op, rt, address.base, offset as u32);
            return;
        }
        // signExtend32ToPtr(offset, memoryTemp); ldr/str rt, [base, memoryTemp].
        self.move_imm64(TrustedImm64::new(offset as i64), Self::MEMORY_TEMP_REGISTER);
        self.asm().emit_load_store_register_offset(
            size,
            op,
            rt,
            address.base,
            Self::MEMORY_TEMP_REGISTER,
            super::arm64_encoder::ExtendType::Uxtx,
            false,
        );
    }

    /// Shared `load*/store*(BaseIndex)` lowering (MacroAssemblerARM64.h:
    /// 1816-1828 and siblings): when the index scale matches the access size (or
    /// is `TimesOne`) and the constant offset folds, emit the register-offset
    /// form directly; otherwise compute the effective address through the memory
    /// temp (sign-extend offset, add the extended index) and load/store with it.
    fn load_store_base_index(
        &mut self,
        size: MemOpSize,
        op: MemOp,
        rt: RegisterID,
        address: BaseIndex,
    ) {
        let extend = Self::index_extend_type(address.extend);
        let scaled = address.scale.log2() != 0;
        if scale_is_valid_for_size(size, address.scale) {
            if let Some(base) = self.try_fold_base_and_offset(address) {
                self.asm().emit_load_store_register_offset(
                    size,
                    op,
                    rt,
                    base,
                    address.index,
                    extend,
                    scaled,
                );
                return;
            }
        }
        // Fallback: memoryTemp = signExtend(offset); memoryTemp += index<<scale;
        // then load/store [base, memoryTemp].
        self.move_imm64(
            TrustedImm64::new(address.offset as i64),
            Self::MEMORY_TEMP_REGISTER,
        );
        self.asm().emit_add_sub_extended_reg(
            Datasize::D64,
            AddOp::Add,
            SetFlags::DontSetFlags,
            Self::MEMORY_TEMP_REGISTER,
            Self::MEMORY_TEMP_REGISTER,
            address.index,
            extend,
            address.scale.log2(),
        );
        self.asm().emit_load_store_register_offset(
            size,
            op,
            rt,
            address.base,
            Self::MEMORY_TEMP_REGISTER,
            super::arm64_encoder::ExtendType::Uxtx,
            false,
        );
    }

    /// `tryFoldBaseAndOffsetPart(BaseIndex)` (MacroAssemblerARM64.h:7942-7958):
    /// the offset-0 case returns the base unchanged; a foldable non-zero offset
    /// is added into the memory temp; otherwise the fold fails (caller takes the
    /// general path).
    fn try_fold_base_and_offset(&mut self, address: BaseIndex) -> Option<RegisterID> {
        if address.offset == 0 {
            return Some(address.base);
        }
        if let Some((u12, shift12, inverted)) = try_extract_shifted_imm32(address.offset) {
            let op = if inverted { AddOp::Sub } else { AddOp::Add };
            self.asm().emit_add_sub_imm(
                Datasize::D64,
                op,
                SetFlags::DontSetFlags,
                Self::MEMORY_TEMP_REGISTER,
                address.base,
                u12,
                shift12,
            );
            return Some(Self::MEMORY_TEMP_REGISTER);
        }
        None
    }
}

/// `AddOp` inversion used when `tryExtractShiftedImm` reports a negated value.
#[inline]
const fn invert_add_op(op: AddOp) -> AddOp {
    match op {
        AddOp::Add => AddOp::Sub,
        AddOp::Sub => AddOp::Add,
    }
}

/// `isUInt12(int32_t)` (AssemblerCommon.h:70-74): does the value fit an unsigned
/// 12-bit field?
#[inline]
const fn is_uint12(value: i32) -> bool {
    (value & !0xfff) == 0
}

/// `tryExtractShiftedImm<int32_t>(immediate)` (MacroAssemblerARM64.h:7960-7981):
/// returns `(u12, shift12, inverted)` when the value is a (possibly LSL-#12,
/// possibly two's-complement-negated) 12-bit immediate; `None` otherwise.
/// `inverted` means the caller must use the opposite add/sub op.
#[inline]
fn try_extract_shifted_imm32(immediate: i32) -> Option<(u32, bool, bool)> {
    if is_uint12(immediate) {
        return Some((immediate as u32 & 0xfff, false, false));
    }
    let negated = immediate.wrapping_neg();
    if is_uint12(negated) {
        return Some((negated as u32 & 0xfff, false, true));
    }
    let shifted = immediate >> 12;
    if (shifted << 12) == immediate {
        if is_uint12(shifted) {
            return Some((shifted as u32 & 0xfff, true, false));
        }
        let negated_shifted = shifted.wrapping_neg();
        if is_uint12(negated_shifted) {
            return Some((negated_shifted as u32 & 0xfff, true, true));
        }
    }
    None
}

/// `isValidSignedImm9(value)` == `isInt9(value)` (AssemblerCommon.h): the
/// `ldur`/`stur` unscaled displacement range, `[-256, 255]`.
#[inline]
const fn is_valid_signed_imm9(value: i32) -> bool {
    value == ((value << 23) >> 23)
}

/// `isValidScaledUImm12<datasize>(offset)` (AssemblerCommon.h:78-89): a
/// non-negative, access-size-aligned offset within `4095 * accessBytes`.
#[inline]
const fn is_valid_scaled_uimm12(size: MemOpSize, offset: i32) -> bool {
    let access = 1i32 << (size as u32);
    let max_pimm = 4095 * access;
    if offset < 0 || offset > max_pimm {
        return false;
    }
    offset & (access - 1) == 0
}

/// Whether a `BaseIndex` scale is directly encodable for an access size: JSC
/// folds `TimesOne` for every size and the access-size-matching scale (the
/// register-offset `S` bit shifts the index by `log2(accessSize)`).
#[inline]
fn scale_is_valid_for_size(size: MemOpSize, scale: Scale) -> bool {
    if scale == Scale::TimesOne {
        return true;
    }
    scale.log2() == size as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(code: &[u8]) -> Vec<u32> {
        code.chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    fn emit(f: impl FnOnce(&mut MacroAssemblerArm64)) -> Vec<u32> {
        let mut masm = MacroAssemblerArm64::new();
        f(&mut masm);
        words(masm.code())
    }

    // ------------------------------------------------------------------------
    // Condition mapping — every variant IS its ARM64 Condition (static_cast).
    // ------------------------------------------------------------------------
    #[test]
    fn relational_and_result_conditions_map_to_arm64() {
        assert_eq!(RelationalCondition::Equal.arm64_condition(), Condition::Eq);
        assert_eq!(
            RelationalCondition::NotEqual.arm64_condition(),
            Condition::Ne
        );
        assert_eq!(RelationalCondition::Below.arm64_condition(), Condition::Lo);
        assert_eq!(RelationalCondition::Above.arm64_condition(), Condition::Hi);
        assert_eq!(
            RelationalCondition::LessThan.arm64_condition(),
            Condition::Lt
        );
        assert_eq!(
            RelationalCondition::GreaterThanOrEqual.arm64_condition(),
            Condition::Ge
        );
        assert_eq!(ResultCondition::Overflow.arm64_condition(), Condition::Vs);
        assert_eq!(ResultCondition::Zero.arm64_condition(), Condition::Eq);
        assert_eq!(ResultCondition::NonZero.arm64_condition(), Condition::Ne);
        assert_eq!(ResultCondition::Carry.arm64_condition(), Condition::Hs);
    }

    // ------------------------------------------------------------------------
    // Arithmetic / logic register forms.
    // ------------------------------------------------------------------------
    #[test]
    fn add_sub_register_forms() {
        // add32(x1, x2, x0) -> add w0, w1, w2 : 0x0b020020.
        assert_eq!(
            emit(|m| m.add32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x0b02_0020]
        );
        // add64(x1, x2, x0) -> add x0, x1, x2 : 0x8b020020.
        assert_eq!(
            emit(|m| m.add64(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x8b02_0020]
        );
        // sub32(x1, x2, x0) -> sub w0, w1, w2 : 0x4b020020.
        assert_eq!(
            emit(|m| m.sub32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x4b02_0020]
        );
        // neg32(x1, x0) -> sub w0, wzr, w1 : 0x4b0103e0.
        assert_eq!(
            emit(|m| m.neg32(RegisterID::X1, RegisterID::X0)),
            vec![0x4b01_03e0]
        );
    }

    #[test]
    fn add32_immediate_fold_and_scratch() {
        // add32(#4, x1, x0) folds to add w0, w1, #4 : 0x11001020.
        assert_eq!(
            emit(|m| m.add32_imm(TrustedImm32::new(4), RegisterID::X1, RegisterID::X0)),
            vec![0x1100_1020]
        );
        // add32(#-1, x1, x0): -1 is not uint12 but +1 is -> sub w0, w1, #1 :
        // 0x51000420.
        assert_eq!(
            emit(|m| m.add32_imm(TrustedImm32::new(-1), RegisterID::X1, RegisterID::X0)),
            vec![0x5100_0420]
        );
        // add32(#0x1000, x1, x0): low 12 bits zero -> add w0, w1, #1, lsl #12 :
        // 0x11400420.
        assert_eq!(
            emit(|m| m.add32_imm(TrustedImm32::new(0x1000), RegisterID::X1, RegisterID::X0)),
            vec![0x1140_0420]
        );
        // add32(#0x12345, x1, x0): not foldable, src != dest -> JSC materializes
        // the immediate into DEST then `add32(src, dest)`
        // (MacroAssemblerARM64.h:203-206), NOT via the data temp:
        // movz w0,#0x2345 : 0x528468a0 ; movk w0,#1,lsl#16 : 0x72a00020 ;
        // add w0, w0, w1 : 0x0b010000.
        assert_eq!(
            emit(|m| m.add32_imm(TrustedImm32::new(0x12345), RegisterID::X1, RegisterID::X0)),
            vec![0x5284_68a0, 0x72a0_0020, 0x0b01_0000]
        );
        // add32(#0x12345, x0, x0): src == dest -> JSC routes through the data
        // temp ip0 (MacroAssemblerARM64.h:207-209): movz w16 ; movk w16 ;
        // add w0, w0, w16 : 0x0b100000.
        assert_eq!(
            emit(|m| m.add32_imm(TrustedImm32::new(0x12345), RegisterID::X0, RegisterID::X0)),
            vec![0x5284_68b0, 0x72a0_0030, 0x0b10_0000]
        );
    }

    #[test]
    fn sub32_immediate_inverts() {
        // sub32(x1, #1, x0) -> sub w0, w1, #1 : 0x51000420.
        assert_eq!(
            emit(|m| m.sub32_imm(RegisterID::X1, TrustedImm32::new(1), RegisterID::X0)),
            vec![0x5100_0420]
        );
        // sub32(x1, #-1, x0) -> inverted -> add w0, w1, #1 : 0x11000420.
        assert_eq!(
            emit(|m| m.sub32_imm(RegisterID::X1, TrustedImm32::new(-1), RegisterID::X0)),
            vec![0x1100_0420]
        );
    }

    #[test]
    fn logic_and_shift_register_forms() {
        // and w0,w1,w2 : 0x0a020020 ; orr : 0x2a020020 ; eor : 0x4a020020.
        assert_eq!(
            emit(|m| m.and32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x0a02_0020]
        );
        assert_eq!(
            emit(|m| m.or32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x2a02_0020]
        );
        assert_eq!(
            emit(|m| m.xor32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x4a02_0020]
        );
        // lslv w0,w1,w2 : 0x1ac22020 ; asrv : 0x1ac22820 ; lsrv : 0x1ac22420.
        assert_eq!(
            emit(|m| m.lshift32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x1ac2_2020]
        );
        assert_eq!(
            emit(|m| m.rshift32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x1ac2_2820]
        );
        assert_eq!(
            emit(|m| m.urshift32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x1ac2_2420]
        );
        // lshift32 imm #1 -> lsl w0,w1,#1 : 0x531f7820.
        assert_eq!(
            emit(|m| m.lshift32_imm(RegisterID::X1, TrustedImm32::new(1), RegisterID::X0)),
            vec![0x531f_7820]
        );
        // mul32(x1, x2, x0) -> mul w0, w1, w2 : 0x1b027c20.
        assert_eq!(
            emit(|m| m.mul32(RegisterID::X1, RegisterID::X2, RegisterID::X0)),
            vec![0x1b02_7c20]
        );
    }

    // ------------------------------------------------------------------------
    // Moves.
    // ------------------------------------------------------------------------
    #[test]
    fn moves_register_immediate_double_swap() {
        // move(x1, x0) -> mov x0, x1 (orr x0, xzr, x1) : 0xaa0103e0.
        assert_eq!(
            emit(|m| m.move_rr(RegisterID::X1, RegisterID::X0)),
            vec![0xaa01_03e0]
        );
        // move(x0, x0) is a no-op.
        assert_eq!(emit(|m| m.move_rr(RegisterID::X0, RegisterID::X0)), vec![]);
        // move(zr, x0) -> movz x0, #0 : 0xd2800000.
        assert_eq!(
            emit(|m| m.move_rr(RegisterID::Zr, RegisterID::X0)),
            vec![0xd280_0000]
        );
        // move(#0x1234, x0) -> movz w0, #0x1234 : 0x52824680.
        assert_eq!(
            emit(|m| m.move_imm32(TrustedImm32::new(0x1234), RegisterID::X0)),
            vec![0x5282_4680]
        );
        // move(#-1, x0) (32-bit all ones) -> movn w0, #0 : 0x12800000.
        assert_eq!(
            emit(|m| m.move_imm32(TrustedImm32::new(-1), RegisterID::X0)),
            vec![0x1280_0000]
        );
        // move64(0x0000_abcd_0000_1234) -> movz x0,#0x1234 ; movk x0,#0xabcd,lsl#32.
        assert_eq!(
            emit(|m| m.move_imm64(TrustedImm64::new(0x0000_abcd_0000_1234), RegisterID::X0)),
            vec![0xd282_4680, 0xf2d5_79a0]
        );
        // moveDouble(q1, q0) -> fmov d0, d1 : 0x1e604020.
        assert_eq!(
            emit(|m| m.move_double(FPRegisterID::Q1, FPRegisterID::Q0)),
            vec![0x1e60_4020]
        );
        // swap(x0, x1) -> mov ip0,x0 ; mov x0,x1 ; mov x1,ip0.
        // mov x16,x0 : 0xaa0003f0 ; mov x0,x1 : 0xaa0103e0 ; mov x1,x16 : 0xaa1003e1.
        assert_eq!(
            emit(|m| m.swap(RegisterID::X0, RegisterID::X1)),
            vec![0xaa00_03f0, 0xaa01_03e0, 0xaa10_03e1]
        );
    }

    // ------------------------------------------------------------------------
    // Loads / stores.
    // ------------------------------------------------------------------------
    #[test]
    fn load_store_address_forms() {
        // load64 Address(x1,#8): isInt9(8) -> ldur x0, [x1, #8] : 0xf8408020.
        assert_eq!(
            emit(|m| m.load64(Address::new(RegisterID::X1, 8), RegisterID::X0)),
            vec![0xf840_8020]
        );
        // load32 Address(x1,#-4): isInt9(-4) -> ldur w0, [x1, #-4] : 0xb85fc020.
        assert_eq!(
            emit(|m| m.load32(Address::new(RegisterID::X1, -4), RegisterID::X0)),
            vec![0xb85f_c020]
        );
        // load8 Address(x1,#1): isInt9(1) -> ldurb w0, [x1, #1] : 0x38401020.
        assert_eq!(
            emit(|m| m.load8(Address::new(RegisterID::X1, 1), RegisterID::X0)),
            vec![0x3840_1020]
        );
        // store64 Address(x1,#8) -> stur x0, [x1, #8] : 0xf8008020.
        assert_eq!(
            emit(|m| m.store64(RegisterID::X0, Address::new(RegisterID::X1, 8))),
            vec![0xf800_8020]
        );
        // load64 Address(x1,#4096): not isInt9, isValidScaledUImm12<64> (4096%8==0)
        // -> ldr x0, [x1, #4096] (imm12 = 4096/8 = 512) : 0xf9480020.
        assert_eq!(
            emit(|m| m.load64(Address::new(RegisterID::X1, 4096), RegisterID::X0)),
            vec![0xf948_0020]
        );
    }

    #[test]
    fn load_store_address_register_offset_fallback() {
        // load64 Address(x1, 0x12345): not imm9, not scaled-uimm12 (unaligned) ->
        // move 0x12345 into ip1 (movz x17,#0x2345 ; movk x17,#1,lsl#16) then
        // ldr x0, [x1, x17] (UXTX, S=0).
        // movz x17,#0x2345 : 0xd28468b1 ; movk x17,#1,lsl#16 : 0xf2a00031 ;
        // ldr x0,[x1,x17] : 0xf8716820.
        assert_eq!(
            emit(|m| m.load64(Address::new(RegisterID::X1, 0x12345), RegisterID::X0)),
            vec![0xd284_68b1, 0xf2a0_0031, 0xf871_6820]
        );
    }

    #[test]
    fn load_store_base_index_forms() {
        // load64 BaseIndex(x1, x2, TimesEight): scale matches 64-bit, offset 0 ->
        // ldr x0, [x1, x2, lsl #3] (UXTX, S=1) : 0xf8627820.
        assert_eq!(
            emit(|m| m.load64_indexed(
                BaseIndex::new(RegisterID::X1, RegisterID::X2, Scale::TimesEight, 0),
                RegisterID::X0
            )),
            vec![0xf862_7820]
        );
        // load32 BaseIndex(x1, x2, TimesFour): scale matches 32-bit ->
        // ldr w0, [x1, x2, lsl #2] : 0xb8627820.
        assert_eq!(
            emit(|m| m.load32_indexed(
                BaseIndex::new(RegisterID::X1, RegisterID::X2, Scale::TimesFour, 0),
                RegisterID::X0
            )),
            vec![0xb862_7820]
        );
        // load8 BaseIndex(x1, x2, TimesOne): ldrb w0, [x1, x2] (S=0) : 0x38626820.
        assert_eq!(
            emit(|m| m.load8_indexed(
                BaseIndex::new(RegisterID::X1, RegisterID::X2, Scale::TimesOne, 0),
                RegisterID::X0
            )),
            vec![0x3862_6820]
        );
        // store32 BaseIndex(x1, x2, TimesFour) -> str w0, [x1, x2, lsl #2] :
        // 0xb8227820.
        assert_eq!(
            emit(|m| m.store32_indexed(
                RegisterID::X0,
                BaseIndex::new(RegisterID::X1, RegisterID::X2, Scale::TimesFour, 0)
            )),
            vec![0xb822_7820]
        );
    }

    // ------------------------------------------------------------------------
    // Branch family — emission + returned token.
    // ------------------------------------------------------------------------
    #[test]
    fn branch32_emits_cmp_bcond_nop_and_returns_token() {
        let mut masm = MacroAssemblerArm64::new();
        let jump = masm.branch32(RelationalCondition::Equal, RegisterID::X0, RegisterID::X1);
        let w = words(masm.code());
        // cmp w0, w1 (subs wzr) : 0x6b01001f ; b.eq #0 : 0x54000000 ; nop : 0xd503201f.
        assert_eq!(w, vec![0x6b01_001f, 0x5400_0000, 0xd503_201f]);
        // The Jump token references the b.cond at offset 4 and carries Eq.
        assert_eq!(jump.label().label(), AssemblerLabel(4));
        assert_eq!(jump.condition(), Condition::Eq);
        assert_eq!(jump.jump_type(), JumpType::JumpCondition);
    }

    #[test]
    fn branch32_immediate_uses_cmp_then_cmn() {
        // branch32(LessThan, x0, #5) -> cmp w0, #5 (subs wzr,w0,#5) : 0x7100141f ;
        // b.lt #0 : 0x5400000b ; nop.
        assert_eq!(
            emit(|m| {
                m.branch32_imm(
                    RelationalCondition::LessThan,
                    RegisterID::X0,
                    TrustedImm32::new(5),
                );
            }),
            vec![0x7100_141f, 0x5400_000b, 0xd503_201f]
        );
        // branch32(Equal, x0, #-1): -1 negates to 1 -> cmn w0, #1 (adds wzr,w0,#1)
        // : 0x3100041f ; b.eq #0 : 0x54000000 ; nop.
        assert_eq!(
            emit(|m| {
                m.branch32_imm(
                    RelationalCondition::Equal,
                    RegisterID::X0,
                    TrustedImm32::new(-1),
                );
            }),
            vec![0x3100_041f, 0x5400_0000, 0xd503_201f]
        );
    }

    #[test]
    fn branch64_and_branch_ptr() {
        // branch64(Below, x0, x1) -> cmp x0, x1 (subs xzr) : 0xeb01001f ;
        // b.lo #0 : 0x54000003 ; nop.
        assert_eq!(
            emit(|m| {
                m.branch64(RelationalCondition::Below, RegisterID::X0, RegisterID::X1);
            }),
            vec![0xeb01_001f, 0x5400_0003, 0xd503_201f]
        );
        // branchPtr == branch64.
        assert_eq!(
            emit(|m| {
                m.branch_ptr(RelationalCondition::Below, RegisterID::X0, RegisterID::X1);
            }),
            vec![0xeb01_001f, 0x5400_0003, 0xd503_201f]
        );
        // branch64(Below, x0, sp): sp can't be the cmp right operand, so it is
        // materialized into ip0 first (add x16, sp, #0 : 0x910003f0), then
        // cmp x0, x16 (subs xzr) : 0xeb10001f ; b.lo #0 : 0x54000003 ; nop. This
        // replaces JSC's swap/extended-cmp on this cold path (correct values).
        assert_eq!(
            emit(|m| {
                m.branch64(RelationalCondition::Below, RegisterID::X0, RegisterID::Sp);
            }),
            vec![0x9100_03f0, 0xeb10_001f, 0x5400_0003, 0xd503_201f]
        );
        // branch64(Equal, x0, sp): same materialization, b.eq #0 : 0x54000000.
        assert_eq!(
            emit(|m| {
                m.branch64(RelationalCondition::Equal, RegisterID::X0, RegisterID::Sp);
            }),
            vec![0x9100_03f0, 0xeb10_001f, 0x5400_0000, 0xd503_201f]
        );
    }

    #[test]
    fn branch_test_add_sub() {
        // branchTest32(NonZero, x0, x1) -> tst w0, w1 (ands wzr) : 0x6a01001f ;
        // b.ne #0 : 0x54000001 ; nop.
        assert_eq!(
            emit(|m| {
                m.branch_test32(ResultCondition::NonZero, RegisterID::X0, RegisterID::X1);
            }),
            vec![0x6a01_001f, 0x5400_0001, 0xd503_201f]
        );
        // branchAdd32(Overflow, x1, x2, x0) -> adds w0, w1, w2 : 0x2b020020 ;
        // b.vs #0 : 0x54000006 ; nop.
        assert_eq!(
            emit(|m| {
                m.branch_add32(
                    ResultCondition::Overflow,
                    RegisterID::X1,
                    RegisterID::X2,
                    RegisterID::X0,
                );
            }),
            vec![0x2b02_0020, 0x5400_0006, 0xd503_201f]
        );
        // branchSub32(Overflow, x1, x2, x0) -> subs w0, w1, w2 : 0x6b020020 ;
        // b.vs #0 ; nop.
        assert_eq!(
            emit(|m| {
                m.branch_sub32(
                    ResultCondition::Overflow,
                    RegisterID::X1,
                    RegisterID::X2,
                    RegisterID::X0,
                );
            }),
            vec![0x6b02_0020, 0x5400_0006, 0xd503_201f]
        );
    }

    #[test]
    fn jump_call_ret_nop_tokens() {
        // jump() -> b #0 : 0x14000000 ; token references offset 0, JumpNoCondition.
        let mut masm = MacroAssemblerArm64::new();
        let j = masm.jump();
        assert_eq!(words(masm.code()), vec![0x1400_0000]);
        assert_eq!(j.label().label(), AssemblerLabel(0));
        assert_eq!(j.jump_type(), JumpType::JumpNoCondition);

        // nearCall() -> bl #0 : 0x94000000.
        let mut masm = MacroAssemblerArm64::new();
        let c = masm.near_call();
        assert_eq!(words(masm.code()), vec![0x9400_0000]);
        assert_eq!(c.label(), AssemblerLabel(0));

        // ret : 0xd65f03c0 ; nop : 0xd503201f.
        assert_eq!(emit(|m| m.ret()), vec![0xd65f_03c0]);
        assert_eq!(emit(|m| m.nop()), vec![0xd503_201f]);
    }

    // ------------------------------------------------------------------------
    // tryExtractShiftedImm and the addressing predicates.
    // ------------------------------------------------------------------------
    #[test]
    fn try_extract_shifted_imm_matches_jsc() {
        assert_eq!(try_extract_shifted_imm32(5), Some((5, false, false)));
        assert_eq!(try_extract_shifted_imm32(-1), Some((1, false, true)));
        assert_eq!(try_extract_shifted_imm32(0x1000), Some((1, true, false)));
        // -0x1000 negates to 0x1000 -> (1, shift, inverted).
        assert_eq!(try_extract_shifted_imm32(-0x1000), Some((1, true, true)));
        // 0x12345: neither uint12 nor a shifted uint12 -> None.
        assert_eq!(try_extract_shifted_imm32(0x12345), None);
        // 0xfff is the max uint12.
        assert_eq!(
            try_extract_shifted_imm32(0xfff),
            Some((0xfff, false, false))
        );
    }

    #[test]
    fn addressing_predicates() {
        assert!(is_valid_signed_imm9(255));
        assert!(is_valid_signed_imm9(-256));
        assert!(!is_valid_signed_imm9(256));
        // scaled uimm12 for 64-bit: aligned to 8, within 4095*8.
        assert!(is_valid_scaled_uimm12(MemOpSize::Size64, 4096));
        assert!(!is_valid_scaled_uimm12(MemOpSize::Size64, 4095)); // not 8-aligned
        assert!(!is_valid_scaled_uimm12(MemOpSize::Size64, -8)); // negative
                                                                 // 8-bit access: any non-negative offset within 4095 is valid.
        assert!(is_valid_scaled_uimm12(MemOpSize::Size8Or128, 1));
        assert!(scale_is_valid_for_size(
            MemOpSize::Size64,
            Scale::TimesEight
        ));
        assert!(scale_is_valid_for_size(MemOpSize::Size32, Scale::TimesFour));
        assert!(scale_is_valid_for_size(
            MemOpSize::Size8Or128,
            Scale::TimesOne
        ));
        assert!(!scale_is_valid_for_size(
            MemOpSize::Size64,
            Scale::TimesFour
        ));
    }
}
