//! ARM64 instruction-word encoder.
//!
//! Faithful port of the instruction-encoding core of
//! `Source/JavaScriptCore/assembler/ARM64Assembler.h`: the private `static int`
//! field-packing helpers (e.g. `addSubtractImmediate`,
//! `loadStoreRegisterPairPreIndex`, `moveWideImediate`,
//! `unconditionalBranchRegister`) and the public `ALWAYS_INLINE void` mnemonic
//! wrappers (`stp`, `ldp`, `mov`, `movz`, `add`, `ldr`, `b`, `ret`, ...).
//!
//! In JSC every wrapper calls `insn(encoder(...))` and `insn` does
//! `m_buffer.putInt(instruction)` — a little-endian 32-bit append. This port
//! mirrors that exactly: [`Arm64Encoder`] borrows the output `Vec<u8>` (the
//! moral equivalent of `ARM64Assembler::m_buffer`) and every `emit_*` method
//! appends one 32-bit instruction word in little-endian order. Encoding is a
//! pure byte computation, so this is all safe Rust — making the bytes
//! executable (W^X / icache) is a separate, out-of-scope layer.
//!
//! Subset covered: the instructions the ARM64 baseline JIT currently hardcodes
//! or emits by hand (prologue/epilogue pair ops, register & wide-immediate
//! moves, add/sub, frame loads/stores, BaseIndex register-offset loads/stores,
//! and the branch family). 32-bit (W-register) datasizes, FP/SIMD instructions,
//! and the unscaled/literal addressing modes are deferred until a consumer
//! needs them.
#![allow(dead_code)]

use super::operands::Scale;
use super::registers::{FPRegisterID, RegisterID};

// ----------------------------------------------------------------------------
// Encoding enums — faithful mirrors of the ARM64Assembler.h field enums.
// ----------------------------------------------------------------------------

/// `ARM64Assembler::Datasize` (ARM64Assembler.h:586-591). The `sf` bit value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Datasize {
    D32 = 0,
    D64 = 1,
    D128 = 2,
    D16 = 3,
}

/// `ARM64Assembler::MemOpSize` (ARM64Assembler.h:593-598). The load/store
/// single-register size field (bits 31:30).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MemOpSize {
    Size8Or128 = 0,
    Size16 = 1,
    Size32 = 2,
    Size64 = 3,
}

/// `ARM64Assembler::MemPairOpSize` (ARM64Assembler.h:747-755). The load/store
/// *pair* size field (bits 31:30).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MemPairOpSize {
    Pair32 = 0,
    PairLoadSigned32 = 1,
    Pair64 = 2,
}

/// `ARM64Assembler::AddOp` (ARM64Assembler.h:600-603).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AddOp {
    Add = 0,
    Sub = 1,
}

/// `ARM64Assembler::MemOp` (ARM64Assembler.h:737-745), restricted to the
/// plain load/store opcodes this subset uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MemOp {
    Store = 0,
    Load = 1,
}

/// `ARM64Assembler::MoveWideOp` (ARM64Assembler.h:757-761).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MoveWideOp {
    /// MOVN (negate). Value 0.
    N = 0,
    /// MOVZ (zero). Value 2.
    Z = 2,
    /// MOVK (keep). Value 3.
    K = 3,
}

/// `ARM64Assembler::LogicalOp` (ARM64Assembler.h:730-735).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LogicalOp {
    And = 0,
    Orr = 1,
    Eor = 2,
    Ands = 3,
}

/// `ARM64Assembler::SetFlags` (ARM64Assembler.h:316-319).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SetFlags {
    DontSetFlags = 0,
    S = 1,
}

/// `ARM64Assembler::BranchType` (ARM64Assembler.h:344-348).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BranchType {
    Jmp = 0,
    Call = 1,
    Ret = 2,
}

/// `ARM64Assembler::ShiftType` (ARM64Assembler.h:308-313).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ShiftType {
    Lsl = 0,
    Lsr = 1,
    Asr = 2,
    Ror = 3,
}

/// `ARM64Assembler::ExtendType` (ARM64Assembler.h:315-325 region).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ExtendType {
    Uxtb = 0,
    Uxth = 1,
    Uxtw = 2,
    Uxtx = 3,
    Sxtb = 4,
    Sxth = 5,
    Sxtw = 6,
    Sxtx = 7,
}

/// `ARM64Assembler::Condition` (ARM64Assembler.h:289-308). The 4-bit condition
/// field for `b.cond`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Condition {
    Eq = 0,
    Ne = 1,
    Hs = 2,
    Lo = 3,
    Mi = 4,
    Pl = 5,
    Vs = 6,
    Vc = 7,
    Hi = 8,
    Ls = 9,
    Ge = 10,
    Lt = 11,
    Gt = 12,
    Le = 13,
    Al = 14,
    /// `ConditionInvalid` (ARM64Assembler.h:290): the sentinel JSC stores in a
    /// `LinkRecord` for non-conditional branches. Never reaches an encoded
    /// instruction field.
    Invalid = 15,
}

/// `ARM64Assembler::DataOp2Source` (ARM64Assembler.h:620-627), restricted to the
/// register-shift opcodes the baseline composite layer needs (`lslv`/`lsrv`/
/// `asrv`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DataOp2Source {
    Lslv = 8,
    Lsrv = 9,
    Asrv = 10,
}

/// `ARM64Assembler::DataOp3Source` (ARM64Assembler.h:629-640): the
/// three-source data-processing opcodes (`madd`/`smaddl`/`smulh`/...). Only the
/// `madd` (32/64-bit `mul`) and `smaddl` (`smull`) forms are emitted today; the
/// rest are ported for fidelity and unused (the module's `#![allow(dead_code)]`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DataOp3Source {
    Madd = 0,
    Msub = 1,
    Smaddl = 2,
    Smsubl = 3,
    Smulh = 4,
    Umaddl = 10,
    Umsubl = 11,
    Umulh = 12,
}

/// `ARM64Assembler::BitfieldOp` (ARM64Assembler.h:605-609), restricted to the
/// `sbfm`/`ubfm` forms that back the immediate shifts (`lsl`/`lsr`/`asr` by an
/// immediate).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BitfieldOp {
    Sbfm = 0,
    Ubfm = 2,
}

/// `ARM64Assembler::FPDataOp1Source` (ARM64Assembler.h:659-660), `FMOV` only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FpDataOp1Source {
    Fmov = 0,
}

/// `ARM64Assembler::FPDataOp2Source` (ARM64Assembler.h:662-672): the FP
/// two-source data-processing opcodes (bits[15:12] of the `0x1e200800` packer).
/// Only the four arithmetic forms (`fmul`/`fdiv`/`fadd`/`fsub`) are emitted by the
/// baseline double fast paths; the rest are ported for fidelity and unused
/// (module `#![allow(dead_code)]`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FpDataOp2Source {
    FMul = 0,
    FDiv = 1,
    FAdd = 2,
    FSub = 3,
    FMax = 4,
    FMin = 5,
    FMaxnm = 6,
    FMinnm = 7,
    FNmul = 8,
}

/// `ARM64Assembler::FPIntConvOp` (ARM64Assembler.h:674-693): the floating-point
/// <-> integer conversion op. Its 5-bit value occupies bits[20:16] of the
/// `0x1e200000` packer as `rmode<<3 | opcode`. Only the three forms the baseline
/// double paths need are listed (all `rmode == 00`: SCVTF plus the two FMOV
/// bit-cast forms); the FCVT* rounding-mode variants are deferred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FpIntConvOp {
    /// `SCVTF` (signed int -> FP): rmode=00, opcode=010.
    Scvtf = 0x02,
    /// `FMOV` FP -> general register (`Xd <- Dn`, the bit-cast): rmode=00, opcode=110.
    FmovFpToGpr = 0x06,
    /// `FMOV` general register -> FP (`Dd <- Xn`, the bit-cast): rmode=00, opcode=111.
    FmovGprToFp = 0x07,
}

// ----------------------------------------------------------------------------
// Register-field helpers — faithful mirrors of ARM64Assembler's xOr* helpers.
// ----------------------------------------------------------------------------

/// `ARM64Assembler::xOrZr` (ARM64Assembler.h:4485-4490): mask to 5 bits so the
/// `zr` alias (`0x3f`) collapses to register field `31`.
#[inline]
const fn x_or_zr(reg: RegisterID) -> u32 {
    reg.value() & 31
}

/// `ARM64Assembler::xOrSp` (ARM64Assembler.h:4478-4483): pass through the raw
/// register value (`sp` is `31`); the caller guarantees it is not `zr`.
#[inline]
const fn x_or_sp(reg: RegisterID) -> u32 {
    reg.value()
}

/// `ARM64Assembler::xOrZrOrSp` (ARM64Assembler.h:4492): pick the `zr` or `sp`
/// interpretation of register field `31` based on whether flags are set.
#[inline]
const fn x_or_zr_or_sp(use_zr: bool, reg: RegisterID) -> u32 {
    if use_zr {
        x_or_zr(reg)
    } else {
        x_or_sp(reg)
    }
}

/// `ARM64Assembler::memPairOffsetShift` (ARM64Assembler.h:785-791): log2 of the
/// pair access size in bytes (64-bit GPR pair -> 3).
#[inline]
const fn mem_pair_offset_shift(v: bool, size: MemPairOpSize) -> u32 {
    let size = size as u32;
    if v {
        size + 2
    } else {
        (size >> 1) + 2
    }
}

// ----------------------------------------------------------------------------
// Instruction-word encoders — one `const fn` per ARM64Assembler.h static helper.
// ----------------------------------------------------------------------------

/// `addSubtractImmediate` (ARM64Assembler.h:4507-4512). `shift12` is the C++
/// `shift == 12` boolean (shift the imm12 left by 12).
#[inline]
const fn add_subtract_immediate(
    sf: Datasize,
    op: AddOp,
    s: SetFlags,
    shift12: bool,
    imm12: u32,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    let use_zr = (s as u32) != 0;
    0x1100_0000
        | (sf as u32) << 31
        | (op as u32) << 30
        | (s as u32) << 29
        | (shift12 as u32) << 22
        | (imm12 & 0xfff) << 10
        | x_or_sp(rn) << 5
        | x_or_zr_or_sp(use_zr, rd)
}

/// `addSubtractShiftedRegister` (ARM64Assembler.h:4514-4519).
#[inline]
const fn add_subtract_shifted_register(
    sf: Datasize,
    op: AddOp,
    s: SetFlags,
    shift: ShiftType,
    rm: RegisterID,
    imm6: u32,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    0x0b00_0000
        | (sf as u32) << 31
        | (op as u32) << 30
        | (s as u32) << 29
        | (shift as u32) << 22
        | x_or_zr(rm) << 16
        | (imm6 & 0x3f) << 10
        | x_or_zr(rn) << 5
        | x_or_zr(rd)
}

/// `addSubtractExtendedRegister` (ARM64Assembler.h:4496-4505). `opt` is always
/// 0 in JSC.
#[inline]
const fn add_subtract_extended_register(
    sf: Datasize,
    op: AddOp,
    s: SetFlags,
    rm: RegisterID,
    option: ExtendType,
    imm3: u32,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    let use_zr = (s as u32) != 0;
    let opt = 0u32;
    0x0b20_0000
        | (sf as u32) << 31
        | (op as u32) << 30
        | (s as u32) << 29
        | opt << 22
        | x_or_zr(rm) << 16
        | (option as u32) << 13
        | (imm3 & 0x7) << 10
        | x_or_sp(rn) << 5
        | x_or_zr_or_sp(use_zr, rd)
}

/// `logicalShiftedRegister` (ARM64Assembler.h:4866-4870). `n` negates `rm`.
#[inline]
const fn logical_shifted_register(
    sf: Datasize,
    opc: LogicalOp,
    shift: ShiftType,
    n: bool,
    rm: RegisterID,
    imm6: u32,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    0x0a00_0000
        | (sf as u32) << 31
        | (opc as u32) << 29
        | (shift as u32) << 22
        | (n as u32) << 21
        | x_or_zr(rm) << 16
        | (imm6 & 0x3f) << 10
        | x_or_zr(rn) << 5
        | x_or_zr(rd)
}

/// `moveWideImediate` (ARM64Assembler.h:4872-4876). `hw` is the halfword shift
/// index (`shift >> 4`).
#[inline]
const fn move_wide_immediate(
    sf: Datasize,
    opc: MoveWideOp,
    hw: u32,
    imm16: u16,
    rd: RegisterID,
) -> u32 {
    0x1280_0000
        | (sf as u32) << 31
        | (opc as u32) << 29
        | hw << 21
        | (imm16 as u32) << 5
        | x_or_zr(rd)
}

/// `loadStoreRegisterUnsignedImmediate` (ARM64Assembler.h:4846-4852), GPR form.
/// `imm12` is already the scaled (pimm / accessSize) immediate.
#[inline]
const fn load_store_register_unsigned_immediate(
    size: MemOpSize,
    v: bool,
    opc: MemOp,
    imm12: u32,
    rn: RegisterID,
    rt: RegisterID,
) -> u32 {
    0x3900_0000
        | (size as u32) << 30
        | (v as u32) << 26
        | (opc as u32) << 22
        | (imm12 & 0xfff) << 10
        | x_or_sp(rn) << 5
        | x_or_zr(rt)
}

/// `loadStoreRegisterRegisterOffset` (ARM64Assembler.h:4817-4823), GPR form.
/// `s` shifts `rm` by log2(accessSize) when set.
#[inline]
const fn load_store_register_register_offset(
    size: MemOpSize,
    v: bool,
    opc: MemOp,
    rm: RegisterID,
    option: ExtendType,
    s: bool,
    rn: RegisterID,
    rt: RegisterID,
) -> u32 {
    0x3820_0800
        | (size as u32) << 30
        | (v as u32) << 26
        | (opc as u32) << 22
        | x_or_zr(rm) << 16
        | (option as u32) << 13
        | (s as u32) << 12
        | x_or_sp(rn) << 5
        | x_or_zr(rt)
}

/// `loadStoreRegisterPairPostIndex` (ARM64Assembler.h:4730-4740), GPR form.
#[inline]
const fn load_store_register_pair_post_index(
    size: MemPairOpSize,
    v: bool,
    opc: MemOp,
    immediate: i32,
    rn: RegisterID,
    rt: RegisterID,
    rt2: RegisterID,
) -> u32 {
    let shift = mem_pair_offset_shift(v, size);
    let imm7 = immediate >> shift;
    0x2880_0000
        | (size as u32) << 30
        | (v as u32) << 26
        | (opc as u32) << 22
        | ((imm7 & 0x7f) as u32) << 15
        | x_or_zr(rt2) << 10
        | x_or_sp(rn) << 5
        | x_or_zr(rt)
}

/// `loadStoreRegisterPairPreIndex` (ARM64Assembler.h:4762-4772), GPR form.
#[inline]
const fn load_store_register_pair_pre_index(
    size: MemPairOpSize,
    v: bool,
    opc: MemOp,
    immediate: i32,
    rn: RegisterID,
    rt: RegisterID,
    rt2: RegisterID,
) -> u32 {
    let shift = mem_pair_offset_shift(v, size);
    let imm7 = immediate >> shift;
    0x2980_0000
        | (size as u32) << 30
        | (v as u32) << 26
        | (opc as u32) << 22
        | ((imm7 & 0x7f) as u32) << 15
        | x_or_zr(rt2) << 10
        | x_or_sp(rn) << 5
        | x_or_zr(rt)
}

/// `loadStoreRegisterPairOffset` (ARM64Assembler.h:4780-4790), GPR form.
#[inline]
const fn load_store_register_pair_offset(
    size: MemPairOpSize,
    v: bool,
    opc: MemOp,
    immediate: i32,
    rn: RegisterID,
    rt: RegisterID,
    rt2: RegisterID,
) -> u32 {
    let shift = mem_pair_offset_shift(v, size);
    let imm7 = immediate >> shift;
    0x2900_0000
        | (size as u32) << 30
        | (v as u32) << 26
        | (opc as u32) << 22
        | ((imm7 & 0x7f) as u32) << 15
        | x_or_zr(rt2) << 10
        | x_or_sp(rn) << 5
        | x_or_zr(rt)
}

/// `unconditionalBranchImmediate` (ARM64Assembler.h:4879-4883). `op` is the
/// link bit; `imm26` is the word (4-byte) offset.
///
/// `pub(crate)` so the LinkBuffer relocation pass (`super::link_records`) can
/// rebuild a `b`/`bl` word with a resolved displacement, exactly as JSC's
/// `linkJumpOrCall` reuses this same encoder (ARM64Assembler.h:4255).
#[inline]
pub(crate) const fn unconditional_branch_immediate(op: bool, imm26: i32) -> u32 {
    0x1400_0000 | (op as u32) << 31 | ((imm26 & 0x03ff_ffff) as u32)
}

/// `conditionalBranchImmediate` (ARM64Assembler.h:4542-4550). `imm19` is the
/// word (4-byte) offset; `o1`/`o0` are always 0.
///
/// `pub(crate)` so the LinkBuffer relocation pass (`super::link_records`) can
/// rebuild a `b.cond` word with a resolved displacement, exactly as JSC's
/// `linkConditionalBranch` reuses this same encoder (ARM64Assembler.h:4299).
#[inline]
pub(crate) const fn conditional_branch_immediate(imm19: i32, cond: Condition) -> u32 {
    0x5400_0000 | ((imm19 & 0x0007_ffff) as u32) << 5 | (cond as u32)
}

/// `unconditionalBranchRegister` (ARM64Assembler.h:4925-4932). `op2` is `0x1f`,
/// `op3`/`op4` are 0.
#[inline]
const fn unconditional_branch_register(opc: BranchType, rn: RegisterID) -> u32 {
    let op2 = 0x1fu32;
    0xd600_0000 | (opc as u32) << 21 | op2 << 16 | x_or_zr(rn) << 5
}

/// `system` (ARM64Assembler.h:4894-4897).
#[inline]
const fn system(l: bool, op0: u32, op1: u32, crn: u32, crm: u32, op2: u32, rt: RegisterID) -> u32 {
    0xd500_0000
        | (l as u32) << 21
        | op0 << 19
        | op1 << 16
        | crn << 12
        | crm << 8
        | op2 << 5
        | x_or_zr(rt)
}

/// `hintPseudo` (ARM64Assembler.h:4899-4903).
#[inline]
const fn hint_pseudo(imm: u32) -> u32 {
    system(false, 0, 3, 2, (imm >> 3) & 0xf, imm & 0x7, RegisterID::Zr)
}

/// `nopPseudo` (ARM64Assembler.h:4905-4908).
#[inline]
const fn nop_pseudo() -> u32 {
    hint_pseudo(0)
}

/// `dataProcessing2Source` (ARM64Assembler.h, the `0x1ac00000` packer): the
/// register-register shift/divide family. `S` is always 0 in the forms this
/// subset emits.
#[inline]
const fn data_processing_2_source(
    sf: Datasize,
    rm: RegisterID,
    opcode: DataOp2Source,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    0x1ac0_0000
        | (sf as u32) << 31
        | x_or_zr(rm) << 16
        | (opcode as u32) << 10
        | x_or_zr(rn) << 5
        | x_or_zr(rd)
}

/// `dataProcessing3Source` (ARM64Assembler.h:4592-4598, the `0x1b000000`
/// packer): the three-source family (`madd`/`smaddl`/`smulh`/...). The opcode is
/// split into `op54`/`op31`/`op0` exactly as JSC does. `mul = madd(rd,rn,rm,zr)`
/// and `smull = smaddl(rd,rn,rm,zr)`.
#[inline]
const fn data_processing_3_source(
    sf: Datasize,
    opcode: DataOp3Source,
    rm: RegisterID,
    ra: RegisterID,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    let opcode = opcode as u32;
    let op54 = opcode >> 4;
    let op31 = (opcode >> 1) & 7;
    let op0 = opcode & 1;
    0x1b00_0000
        | (sf as u32) << 31
        | op54 << 29
        | op31 << 21
        | x_or_zr(rm) << 16
        | op0 << 15
        | x_or_zr(ra) << 10
        | x_or_zr(rn) << 5
        | x_or_zr(rd)
}

/// `bitfield` (ARM64Assembler.h, the `0x13000000` packer). `N` is tied to `sf`
/// (the 64-bit bitfield variant sets `N`).
#[inline]
const fn bitfield(
    sf: Datasize,
    opc: BitfieldOp,
    immr: u32,
    imms: u32,
    rn: RegisterID,
    rd: RegisterID,
) -> u32 {
    let n = sf as u32;
    0x1300_0000
        | (sf as u32) << 31
        | (opc as u32) << 29
        | n << 22
        | (immr & 0x3f) << 16
        | (imms & 0x3f) << 10
        | x_or_zr(rn) << 5
        | x_or_zr(rd)
}

/// `floatingPointDataProcessing1Source` (ARM64Assembler.h, the `0x1e204000`
/// packer). `M`/`S` are 0; `type` is the FP `Datasize` (`D64` -> double).
#[inline]
const fn floating_point_data_processing_1_source(
    type_: Datasize,
    opcode: FpDataOp1Source,
    rn: FPRegisterID,
    rd: FPRegisterID,
) -> u32 {
    0x1e20_4000 | (type_ as u32) << 22 | (opcode as u32) << 15 | rn.value() << 5 | rd.value()
}

/// `floatingPointDataProcessing2Source` (ARM64Assembler.h, the `0x1e200800`
/// packer): the FP two-source arithmetic family (`fadd`/`fsub`/`fmul`/`fdiv`).
/// `type` is the FP `Datasize` (`D64` -> double); `opcode` is bits[15:12].
#[inline]
const fn floating_point_data_processing_2_source(
    type_: Datasize,
    rm: FPRegisterID,
    opcode: FpDataOp2Source,
    rn: FPRegisterID,
    rd: FPRegisterID,
) -> u32 {
    0x1e20_0800
        | (type_ as u32) << 22
        | rm.value() << 16
        | (opcode as u32) << 12
        | rn.value() << 5
        | rd.value()
}

/// `floatingPointIntegerConversions` (ARM64Assembler.h, the `0x1e200000`
/// packer): FP <-> integer conversion (`scvtf`, FMOV bit-casts). `sf` is the GPR
/// datasize, `type` the FP datasize; `rmode_opcode` occupies bits[20:16]. `rn`/`rd`
/// are raw 5-bit register fields — one side is a GPR and the other an FP register
/// per the conversion direction, so this takes the already-extracted fields (each
/// caller passes `.value()`, masked to 5 bits like `xOrZr`).
#[inline]
const fn floating_point_integer_conversions(
    sf: Datasize,
    type_: Datasize,
    rmode_opcode: FpIntConvOp,
    rn: u32,
    rd: u32,
) -> u32 {
    0x1e20_0000
        | (sf as u32) << 31
        | (type_ as u32) << 22
        | (rmode_opcode as u32) << 16
        | (rn & 31) << 5
        | (rd & 31)
}

/// `loadStoreRegisterUnscaledImmediate` (ARM64Assembler.h:4831-4842), GPR form
/// (`ldur`/`stur`). `imm9` is a signed byte displacement (no scaling).
#[inline]
const fn load_store_register_unscaled_immediate(
    size: MemOpSize,
    v: bool,
    opc: MemOp,
    imm9: i32,
    rn: RegisterID,
    rt: RegisterID,
) -> u32 {
    0x3800_0000
        | (size as u32) << 30
        | (v as u32) << 26
        | (opc as u32) << 22
        | ((imm9 & 0x1ff) as u32) << 12
        | x_or_sp(rn) << 5
        | x_or_zr(rt)
}

// ----------------------------------------------------------------------------
// Arm64Encoder — the ARM64Assembler buffer-append surface.
// ----------------------------------------------------------------------------

/// Appends ARM64 instruction words to a borrowed code buffer.
///
/// C++ map: the buffer side of `ARM64Assembler` — `m_buffer` plus `insn`
/// (`m_buffer.putInt`). Borrows `&mut Vec<u8>` so the encoder owns no storage of
/// its own, exactly like `ARM64Assembler` borrows its `AssemblerBuffer`.
pub struct Arm64Encoder<'a> {
    buffer: &'a mut Vec<u8>,
}

impl<'a> Arm64Encoder<'a> {
    /// Wrap a code buffer for emission.
    #[inline]
    pub fn new(buffer: &'a mut Vec<u8>) -> Self {
        Self { buffer }
    }

    /// Current byte length of the emitted code (the buffer's `codeSize`).
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// `ARM64Assembler::insn` -> `AssemblerBuffer::putInt`: append one 32-bit
    /// instruction word little-endian (ARM64 is little-endian).
    #[inline]
    fn insn(&mut self, instruction: u32) {
        self.buffer.extend_from_slice(&instruction.to_le_bytes());
    }

    // ---- Moves -------------------------------------------------------------

    /// `mov<64>(rd, rm)` (ARM64Assembler.h:2375-2382): `add rd, rm, #0` when
    /// either operand is `sp`, else `orr rd, xzr, rm`.
    #[inline]
    pub fn emit_mov(&mut self, rd: RegisterID, rm: RegisterID) {
        if rd.is_sp() || rm.is_sp() {
            self.emit_add_imm12(rd, rm, 0, false);
        } else {
            self.insn(logical_shifted_register(
                Datasize::D64,
                LogicalOp::Orr,
                ShiftType::Lsl,
                false,
                rm,
                0,
                RegisterID::Zr,
                rd,
            ));
        }
    }

    /// `orr<64>(rd, rn, rm)` (ARM64Assembler.h:2708-2711), `LSL #0`.
    #[inline]
    pub fn emit_orr_reg(&mut self, rd: RegisterID, rn: RegisterID, rm: RegisterID) {
        self.insn(logical_shifted_register(
            Datasize::D64,
            LogicalOp::Orr,
            ShiftType::Lsl,
            false,
            rm,
            0,
            rn,
            rd,
        ));
    }

    /// `movz<64>(rd, value, shift)` (ARM64Assembler.h:2544-2549). `shift` is a
    /// byte-position multiple of 16 (0/16/32/48).
    #[inline]
    pub fn emit_movz(&mut self, rd: RegisterID, value: u16, shift: u32) {
        self.insn(move_wide_immediate(
            Datasize::D64,
            MoveWideOp::Z,
            shift >> 4,
            value,
            rd,
        ));
    }

    /// `movk<64>(rd, value, shift)` (ARM64Assembler.h:2528-2533).
    #[inline]
    pub fn emit_movk(&mut self, rd: RegisterID, value: u16, shift: u32) {
        self.insn(move_wide_immediate(
            Datasize::D64,
            MoveWideOp::K,
            shift >> 4,
            value,
            rd,
        ));
    }

    /// `movn<64>(rd, value, shift)` (ARM64Assembler.h:2536-2541).
    #[inline]
    pub fn emit_movn(&mut self, rd: RegisterID, value: u16, shift: u32) {
        self.insn(move_wide_immediate(
            Datasize::D64,
            MoveWideOp::N,
            shift >> 4,
            value,
            rd,
        ));
    }

    /// Materialize a 64-bit immediate into `rd` with a `movz` then `movk`
    /// sequence over the non-zero 16-bit halfwords.
    ///
    /// Mirrors the in-tree baseline pattern
    /// `src/jit/arm64_baseline.rs::p6_arm64_emit_mov_xd_u64` (movz the lowest
    /// non-zero halfword, then movk each higher non-zero halfword; movz #0 for a
    /// zero value). This is the simple correct sequence; JSC's
    /// `MacroAssemblerARM64::moveInternal` additionally optimizes all-ones runs
    /// with `movn`, which is deferred (it never changes correctness, only size).
    pub fn emit_move_immediate64(&mut self, rd: RegisterID, value: u64) {
        let mut emitted_movz = false;
        let mut shift = 0u32;
        while shift < 64 {
            let halfword = ((value >> shift) & 0xffff) as u16;
            if halfword != 0 {
                if !emitted_movz {
                    self.emit_movz(rd, halfword, shift);
                    emitted_movz = true;
                } else {
                    self.emit_movk(rd, halfword, shift);
                }
            }
            shift += 16;
        }
        if !emitted_movz {
            self.emit_movz(rd, 0, 0);
        }
    }

    // ---- Add / Sub ---------------------------------------------------------

    /// `add<64>(rd, rn, imm12, shift)` (ARM64Assembler.h:804-809).
    #[inline]
    pub fn emit_add_imm12(&mut self, rd: RegisterID, rn: RegisterID, imm12: u32, shift12: bool) {
        self.insn(add_subtract_immediate(
            Datasize::D64,
            AddOp::Add,
            SetFlags::DontSetFlags,
            shift12,
            imm12,
            rn,
            rd,
        ));
    }

    /// `sub<64>(rd, rn, imm12, shift)` (ARM64Assembler.h:3013-3018).
    #[inline]
    pub fn emit_sub_imm12(&mut self, rd: RegisterID, rn: RegisterID, imm12: u32, shift12: bool) {
        self.insn(add_subtract_immediate(
            Datasize::D64,
            AddOp::Sub,
            SetFlags::DontSetFlags,
            shift12,
            imm12,
            rn,
            rd,
        ));
    }

    /// `add<64>(rd, rn, rm)` (ARM64Assembler.h:812-833): shifted-register form,
    /// except when `rd`/`rn` is `sp` JSC routes through the extended-register
    /// form (`UXTX #0`), which this mirrors.
    #[inline]
    pub fn emit_add_reg(&mut self, rd: RegisterID, rn: RegisterID, rm: RegisterID) {
        if rd.is_sp() || rn.is_sp() {
            self.insn(add_subtract_extended_register(
                Datasize::D64,
                AddOp::Add,
                SetFlags::DontSetFlags,
                rm,
                ExtendType::Uxtx,
                0,
                rn,
                rd,
            ));
        } else {
            self.insn(add_subtract_shifted_register(
                Datasize::D64,
                AddOp::Add,
                SetFlags::DontSetFlags,
                ShiftType::Lsl,
                rm,
                0,
                rn,
                rd,
            ));
        }
    }

    /// `sub<64>(rd, rn, rm)` (ARM64Assembler.h:3021-3045): shifted-register
    /// form, with the same `sp` extended-register routing as `add`.
    #[inline]
    pub fn emit_sub_reg(&mut self, rd: RegisterID, rn: RegisterID, rm: RegisterID) {
        if rd.is_sp() || rn.is_sp() {
            self.insn(add_subtract_extended_register(
                Datasize::D64,
                AddOp::Sub,
                SetFlags::DontSetFlags,
                rm,
                ExtendType::Uxtx,
                0,
                rn,
                rd,
            ));
        } else {
            self.insn(add_subtract_shifted_register(
                Datasize::D64,
                AddOp::Sub,
                SetFlags::DontSetFlags,
                ShiftType::Lsl,
                rm,
                0,
                rn,
                rd,
            ));
        }
    }

    // ---- Loads / Stores (single register) ----------------------------------

    /// `ldr<64>(rt, rn, pimm)` (ARM64Assembler.h:1277-1281), unsigned-offset
    /// form. `byte_offset` is the unscaled byte displacement and must be a
    /// non-negative multiple of 8 (`encodePositiveImmediate<64>`).
    #[inline]
    pub fn emit_ldr_imm64(&mut self, rt: RegisterID, rn: RegisterID, byte_offset: u32) {
        debug_assert!(byte_offset % 8 == 0, "64-bit ldr offset must be 8-aligned");
        self.insn(load_store_register_unsigned_immediate(
            MemOpSize::Size64,
            false,
            MemOp::Load,
            byte_offset / 8,
            rn,
            rt,
        ));
    }

    /// `str<64>(rt, rn, pimm)` (ARM64Assembler.h:2922-2926), unsigned-offset
    /// form. Same scaling/alignment requirement as [`Self::emit_ldr_imm64`].
    #[inline]
    pub fn emit_str_imm64(&mut self, rt: RegisterID, rn: RegisterID, byte_offset: u32) {
        debug_assert!(byte_offset % 8 == 0, "64-bit str offset must be 8-aligned");
        self.insn(load_store_register_unsigned_immediate(
            MemOpSize::Size64,
            false,
            MemOp::Store,
            byte_offset / 8,
            rn,
            rt,
        ));
    }

    /// `ldr<64>(rt, rn, rm, extend, amount)` (ARM64Assembler.h:1270-1274),
    /// register-offset form for a `BaseIndex`. The `scale` selects the `S`
    /// (scaled-index) bit: a `TimesEight` index on a 64-bit access scales, while
    /// `TimesOne` does not.
    #[inline]
    pub fn emit_ldr_register_offset64(
        &mut self,
        rt: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
        extend: ExtendType,
        scale: Scale,
    ) {
        self.insn(load_store_register_register_offset(
            MemOpSize::Size64,
            false,
            MemOp::Load,
            rm,
            extend,
            scale.log2() != 0,
            rn,
            rt,
        ));
    }

    /// `str<64>(rt, rn, rm, extend, amount)` (ARM64Assembler.h:2915-2919),
    /// register-offset form. See [`Self::emit_ldr_register_offset64`].
    #[inline]
    pub fn emit_str_register_offset64(
        &mut self,
        rt: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
        extend: ExtendType,
        scale: Scale,
    ) {
        self.insn(load_store_register_register_offset(
            MemOpSize::Size64,
            false,
            MemOp::Store,
            rm,
            extend,
            scale.log2() != 0,
            rn,
            rt,
        ));
    }

    // ---- Load/Store Pair (prologue/epilogue) -------------------------------

    /// `stp<64>(rt, rt2, rn, PairPreIndex(simm))` (ARM64Assembler.h:2860-2864).
    /// `byte_offset` is the unscaled (pre-shift) byte displacement.
    #[inline]
    pub fn emit_stp_pre_index64(
        &mut self,
        rt: RegisterID,
        rt2: RegisterID,
        rn: RegisterID,
        byte_offset: i32,
    ) {
        self.insn(load_store_register_pair_pre_index(
            MemPairOpSize::Pair64,
            false,
            MemOp::Store,
            byte_offset,
            rn,
            rt,
            rt2,
        ));
    }

    /// `stp<64>(rt, rt2, rn, PairPostIndex(simm))` (ARM64Assembler.h:2853-2857).
    #[inline]
    pub fn emit_stp_post_index64(
        &mut self,
        rt: RegisterID,
        rt2: RegisterID,
        rn: RegisterID,
        byte_offset: i32,
    ) {
        self.insn(load_store_register_pair_post_index(
            MemPairOpSize::Pair64,
            false,
            MemOp::Store,
            byte_offset,
            rn,
            rt,
            rt2,
        ));
    }

    /// `stp<64>(rt, rt2, rn, simm)` (ARM64Assembler.h:2867-2871), signed-offset
    /// form (no writeback).
    #[inline]
    pub fn emit_stp_offset64(
        &mut self,
        rt: RegisterID,
        rt2: RegisterID,
        rn: RegisterID,
        byte_offset: i32,
    ) {
        self.insn(load_store_register_pair_offset(
            MemPairOpSize::Pair64,
            false,
            MemOp::Store,
            byte_offset,
            rn,
            rt,
            rt2,
        ));
    }

    /// `ldp<64>(rt, rt2, rn, PairPreIndex(simm))` (ARM64Assembler.h:1215-1219).
    #[inline]
    pub fn emit_ldp_pre_index64(
        &mut self,
        rt: RegisterID,
        rt2: RegisterID,
        rn: RegisterID,
        byte_offset: i32,
    ) {
        self.insn(load_store_register_pair_pre_index(
            MemPairOpSize::Pair64,
            false,
            MemOp::Load,
            byte_offset,
            rn,
            rt,
            rt2,
        ));
    }

    /// `ldp<64>(rt, rt2, rn, PairPostIndex(simm))`
    /// (ARM64Assembler.h:1208-1212).
    #[inline]
    pub fn emit_ldp_post_index64(
        &mut self,
        rt: RegisterID,
        rt2: RegisterID,
        rn: RegisterID,
        byte_offset: i32,
    ) {
        self.insn(load_store_register_pair_post_index(
            MemPairOpSize::Pair64,
            false,
            MemOp::Load,
            byte_offset,
            rn,
            rt,
            rt2,
        ));
    }

    /// `ldp<64>(rt, rt2, rn, simm)` (ARM64Assembler.h:1222-1226),
    /// signed-offset form (no writeback).
    #[inline]
    pub fn emit_ldp_offset64(
        &mut self,
        rt: RegisterID,
        rt2: RegisterID,
        rn: RegisterID,
        byte_offset: i32,
    ) {
        self.insn(load_store_register_pair_offset(
            MemPairOpSize::Pair64,
            false,
            MemOp::Load,
            byte_offset,
            rn,
            rt,
            rt2,
        ));
    }

    // ---- Branches ----------------------------------------------------------

    /// `b()` (ARM64Assembler.h:892-895): unconditional immediate branch.
    /// `imm26` is the word (4-byte) offset; `0` is the unlinked placeholder JSC
    /// emits before relocation.
    #[inline]
    pub fn emit_b(&mut self, imm26: i32) {
        self.insn(unconditional_branch_immediate(false, imm26));
    }

    /// `bl()` (ARM64Assembler.h:943-946): unconditional immediate branch with
    /// link (call). `imm26` is the word offset (`0` when unlinked).
    #[inline]
    pub fn emit_bl(&mut self, imm26: i32) {
        self.insn(unconditional_branch_immediate(true, imm26));
    }

    /// `b.cond` via `b_cond(cond, offset)` (ARM64Assembler.h:897-903). `imm19`
    /// is the word (4-byte) offset.
    #[inline]
    pub fn emit_b_cond(&mut self, cond: Condition, imm19: i32) {
        self.insn(conditional_branch_immediate(imm19, cond));
    }

    /// `blr(rn)` (ARM64Assembler.h:948-951): indirect call.
    #[inline]
    pub fn emit_blr(&mut self, rn: RegisterID) {
        self.insn(unconditional_branch_register(BranchType::Call, rn));
    }

    /// `br(rn)` (ARM64Assembler.h:953-956): indirect branch.
    #[inline]
    pub fn emit_br(&mut self, rn: RegisterID) {
        self.insn(unconditional_branch_register(BranchType::Jmp, rn));
    }

    /// `ret(rn = lr)` (ARM64Assembler.h:2734-2737).
    #[inline]
    pub fn emit_ret(&mut self) {
        self.emit_ret_reg(RegisterID::Lr);
    }

    /// `ret(rn)` (ARM64Assembler.h:2734-2737), explicit return register.
    #[inline]
    pub fn emit_ret_reg(&mut self, rn: RegisterID) {
        self.insn(unconditional_branch_register(BranchType::Ret, rn));
    }

    // ---- Misc --------------------------------------------------------------

    /// `nop()` (ARM64Assembler.h:2600-2603).
    #[inline]
    pub fn emit_nop(&mut self) {
        self.insn(nop_pseudo());
    }

    // ------------------------------------------------------------------------
    // Datasize-generic ARM64Assembler public methods consumed by the
    // MacroAssemblerARM64 composite layer (`macro_assembler_arm64.rs`). These
    // mirror ARM64Assembler's `template<int datasize[, SetFlags]>` mnemonics;
    // the datasize/set-flags template parameters become runtime arguments. They
    // append the same instruction words the existing D64-only helpers above do
    // for their fixed datasize, and are byte-tested in the tests module.
    // ------------------------------------------------------------------------

    /// `add<datasize, S>(rd, rn, imm12, shift)` / `sub<datasize, S>(...)`
    /// (ARM64Assembler.h add/sub immediate). `shift12` is the `shift == 12`
    /// boolean. With `SetFlags::S` and `rd == zr` this is `cmp`/`cmn` immediate.
    #[inline]
    pub fn emit_add_sub_imm(
        &mut self,
        sf: Datasize,
        op: AddOp,
        s: SetFlags,
        rd: RegisterID,
        rn: RegisterID,
        imm12: u32,
        shift12: bool,
    ) {
        self.insn(add_subtract_immediate(sf, op, s, shift12, imm12, rn, rd));
    }

    /// `add<datasize, S>(rd, rn, rm)` / `sub<datasize, S>(...)` shifted-register
    /// form (ARM64Assembler.h:812-833 / 3021-3045). When `rd`/`rn` is `sp` JSC
    /// routes through the extended-register form (`UXTX #0`); this mirrors that.
    /// With `SetFlags::S` and `rd == zr` this is `cmp`/`cmn` register.
    #[inline]
    pub fn emit_add_sub_reg(
        &mut self,
        sf: Datasize,
        op: AddOp,
        s: SetFlags,
        rd: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
    ) {
        if rd.is_sp() || rn.is_sp() {
            self.insn(add_subtract_extended_register(
                sf,
                op,
                s,
                rm,
                ExtendType::Uxtx,
                0,
                rn,
                rd,
            ));
        } else {
            self.insn(add_subtract_shifted_register(
                sf,
                op,
                s,
                ShiftType::Lsl,
                rm,
                0,
                rn,
                rd,
            ));
        }
    }

    /// `add<datasize, S>(rd, rn, rm, extend, amount)` extended-register form
    /// (ARM64Assembler.h add/sub extended). Used to fold a scaled `BaseIndex`
    /// index into an address (`add x, x, index, UXTX/UXTW/SXTW #scale`).
    #[inline]
    pub fn emit_add_sub_extended_reg(
        &mut self,
        sf: Datasize,
        op: AddOp,
        s: SetFlags,
        rd: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
        extend: ExtendType,
        amount: u32,
    ) {
        self.insn(add_subtract_extended_register(
            sf, op, s, rm, extend, amount, rn, rd,
        ));
    }

    /// `and_<datasize, S>` / `orr<datasize>` / `eor<datasize>` shifted-register
    /// form, `LSL #0` (ARM64Assembler.h logical register). `opc` selects the
    /// operation; `LogicalOp::Ands` with `rd == zr` is `tst`.
    #[inline]
    pub fn emit_logical_reg(
        &mut self,
        sf: Datasize,
        opc: LogicalOp,
        rd: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
    ) {
        self.insn(logical_shifted_register(
            sf,
            opc,
            ShiftType::Lsl,
            false,
            rm,
            0,
            rn,
            rd,
        ));
    }

    /// `lslv` / `lsrv` / `asrv` `<datasize>(rd, rn, rm)`
    /// (ARM64Assembler.h:1523/1543/886): register-amount shift.
    #[inline]
    pub fn emit_shift_reg(
        &mut self,
        sf: Datasize,
        op: DataOp2Source,
        rd: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
    ) {
        self.insn(data_processing_2_source(sf, rm, op, rn, rd));
    }

    /// `lsl<datasize>(rd, rn, shift)` (ARM64Assembler.h:1510-1514): immediate
    /// left shift via `ubfm(rd, rn, (datasize-shift)&(datasize-1), datasize-1-shift)`.
    #[inline]
    pub fn emit_lsl_imm(&mut self, sf: Datasize, rd: RegisterID, rn: RegisterID, shift: u32) {
        let datasize = datasize_bits(sf);
        let immr = (datasize - shift) & (datasize - 1);
        let imms = datasize - 1 - shift;
        self.insn(bitfield(sf, BitfieldOp::Ubfm, immr, imms, rn, rd));
    }

    /// `lsr<datasize>(rd, rn, shift)` (ARM64Assembler.h:1530-1533): immediate
    /// logical right shift via `ubfm(rd, rn, shift, datasize-1)`.
    #[inline]
    pub fn emit_lsr_imm(&mut self, sf: Datasize, rd: RegisterID, rn: RegisterID, shift: u32) {
        let imms = datasize_bits(sf) - 1;
        self.insn(bitfield(sf, BitfieldOp::Ubfm, shift, imms, rn, rd));
    }

    /// `asr<datasize>(rd, rn, shift)` (ARM64Assembler.h:873-876): immediate
    /// arithmetic right shift via `sbfm(rd, rn, shift, datasize-1)`.
    #[inline]
    pub fn emit_asr_imm(&mut self, sf: Datasize, rd: RegisterID, rn: RegisterID, shift: u32) {
        let imms = datasize_bits(sf) - 1;
        self.insn(bitfield(sf, BitfieldOp::Sbfm, shift, imms, rn, rd));
    }

    /// `mul<datasize>(rd, rn, rm)` (ARM64Assembler.h:2559-2562):
    /// `madd(rd, rn, rm, zr)`.
    #[inline]
    pub fn emit_mul(&mut self, sf: Datasize, rd: RegisterID, rn: RegisterID, rm: RegisterID) {
        self.insn(data_processing_3_source(
            sf,
            DataOp3Source::Madd,
            rm,
            RegisterID::Zr,
            rn,
            rd,
        ));
    }

    /// `smull(rd, rn, rm)` (ARM64Assembler.h:2835-2838): `smaddl(rd, rn, rm, zr)`,
    /// the signed 32x32 -> 64-bit multiply. `rd` is the 64-bit (X) destination;
    /// `rn`/`rm` are the 32-bit (W) source views. Always a 64-bit (`Datasize_64`)
    /// instruction. Used by `MacroAssemblerARM64::branchMul32`'s overflow check.
    #[inline]
    pub fn emit_smull(&mut self, rd: RegisterID, rn: RegisterID, rm: RegisterID) {
        self.insn(data_processing_3_source(
            Datasize::D64,
            DataOp3Source::Smaddl,
            rm,
            RegisterID::Zr,
            rn,
            rd,
        ));
    }

    /// `movz`/`movk`/`movn` `<datasize>(rd, value, shift)` (ARM64Assembler.h
    /// move-wide). `shift` is a byte-position multiple of 16.
    #[inline]
    pub fn emit_move_wide(
        &mut self,
        sf: Datasize,
        op: MoveWideOp,
        rd: RegisterID,
        value: u16,
        shift: u32,
    ) {
        self.insn(move_wide_immediate(sf, op, shift >> 4, value, rd));
    }

    /// `mov<datasize>(rd, rm)` (ARM64Assembler.h:2375-2382): `add rd, rm, #0`
    /// when either operand is `sp`, else `orr rd, xzr, rm`.
    #[inline]
    pub fn emit_mov_reg(&mut self, sf: Datasize, rd: RegisterID, rm: RegisterID) {
        if rd.is_sp() || rm.is_sp() {
            self.emit_add_sub_imm(sf, AddOp::Add, SetFlags::DontSetFlags, rd, rm, 0, false);
        } else {
            self.insn(logical_shifted_register(
                sf,
                LogicalOp::Orr,
                ShiftType::Lsl,
                false,
                rm,
                0,
                RegisterID::Zr,
                rd,
            ));
        }
    }

    /// `fmov<64>(vd, vn)` (ARM64Assembler.h:3368-3372): double-precision FP
    /// register-to-register move.
    #[inline]
    pub fn emit_fmov_double(&mut self, rd: FPRegisterID, rn: FPRegisterID) {
        self.insn(floating_point_data_processing_1_source(
            Datasize::D64,
            FpDataOp1Source::Fmov,
            rn,
            rd,
        ));
    }

    /// `fadd<64>(rd, rn, rm)` (ARM64Assembler.h:3300-3304): `rd = rn + rm`,
    /// double-precision.
    #[inline]
    pub fn emit_fadd_double(&mut self, rd: FPRegisterID, rn: FPRegisterID, rm: FPRegisterID) {
        self.insn(floating_point_data_processing_2_source(
            Datasize::D64,
            rm,
            FpDataOp2Source::FAdd,
            rn,
            rd,
        ));
    }

    /// `fsub<64>(rd, rn, rm)` (ARM64Assembler.h:3306-3310): `rd = rn - rm`.
    #[inline]
    pub fn emit_fsub_double(&mut self, rd: FPRegisterID, rn: FPRegisterID, rm: FPRegisterID) {
        self.insn(floating_point_data_processing_2_source(
            Datasize::D64,
            rm,
            FpDataOp2Source::FSub,
            rn,
            rd,
        ));
    }

    /// `fmul<64>(rd, rn, rm)` (ARM64Assembler.h:3312-3316): `rd = rn * rm`.
    #[inline]
    pub fn emit_fmul_double(&mut self, rd: FPRegisterID, rn: FPRegisterID, rm: FPRegisterID) {
        self.insn(floating_point_data_processing_2_source(
            Datasize::D64,
            rm,
            FpDataOp2Source::FMul,
            rn,
            rd,
        ));
    }

    /// `fdiv<64>(rd, rn, rm)` (ARM64Assembler.h:3318-3322): `rd = rn / rm`.
    #[inline]
    pub fn emit_fdiv_double(&mut self, rd: FPRegisterID, rn: FPRegisterID, rm: FPRegisterID) {
        self.insn(floating_point_data_processing_2_source(
            Datasize::D64,
            rm,
            FpDataOp2Source::FDiv,
            rn,
            rd,
        ));
    }

    /// `scvtf<64, 32>(vd, rn)` (ARM64Assembler.h:3441-3445, `convertInt32ToDouble`):
    /// signed 32-bit integer (W-register source) -> double FP register. `sf == D32`
    /// (the integer source is the 32-bit W view), `type == D64` (double result).
    #[inline]
    pub fn emit_scvtf_int32_to_double(&mut self, vd: FPRegisterID, rn: RegisterID) {
        self.insn(floating_point_integer_conversions(
            Datasize::D32,
            Datasize::D64,
            FpIntConvOp::Scvtf,
            rn.value(),
            vd.value(),
        ));
    }

    /// `fmov<64>(rd, vn)` (ARM64Assembler.h:3380-3384, `moveDoubleTo64`): bit-cast
    /// a double FP register to a 64-bit GPR (`Xd <- Dn`). No numeric conversion —
    /// the raw 64 bits are copied.
    #[inline]
    pub fn emit_fmov_double_to_gpr(&mut self, rd: RegisterID, vn: FPRegisterID) {
        self.insn(floating_point_integer_conversions(
            Datasize::D64,
            Datasize::D64,
            FpIntConvOp::FmovFpToGpr,
            vn.value(),
            rd.value(),
        ));
    }

    /// `fmov<64>(vd, rn)` (ARM64Assembler.h:3374-3378, `move64ToDouble`): bit-cast
    /// a 64-bit GPR to a double FP register (`Dd <- Xn`). No numeric conversion.
    #[inline]
    pub fn emit_fmov_gpr_to_double(&mut self, vd: FPRegisterID, rn: RegisterID) {
        self.insn(floating_point_integer_conversions(
            Datasize::D64,
            Datasize::D64,
            FpIntConvOp::FmovGprToFp,
            rn.value(),
            vd.value(),
        ));
    }

    /// `ldur`/`stur` (ARM64Assembler.h:1472/2996): unscaled signed-imm9
    /// load/store. `op`/`size` select the direction and access width.
    #[inline]
    pub fn emit_load_store_unscaled(
        &mut self,
        size: MemOpSize,
        op: MemOp,
        rt: RegisterID,
        rn: RegisterID,
        simm9: i32,
    ) {
        self.insn(load_store_register_unscaled_immediate(
            size, false, op, simm9, rn, rt,
        ));
    }

    /// `ldr`/`str` (ARM64Assembler.h:1276/2910): unsigned scaled-imm12
    /// load/store. `byte_offset` is the unscaled byte displacement (a
    /// non-negative multiple of the access size), scaled here via
    /// `encodePositiveImmediate`.
    #[inline]
    pub fn emit_load_store_unsigned(
        &mut self,
        size: MemOpSize,
        op: MemOp,
        rt: RegisterID,
        rn: RegisterID,
        byte_offset: u32,
    ) {
        let access = mem_access_bytes(size);
        debug_assert!(
            byte_offset % access == 0,
            "unsigned-immediate offset must be a multiple of the access size"
        );
        self.insn(load_store_register_unsigned_immediate(
            size,
            false,
            op,
            byte_offset / access,
            rn,
            rt,
        ));
    }

    /// `ldr`/`str` (ARM64Assembler.h:1270/2914): register-offset load/store.
    /// `s` is the scaled-index bit (set when the index is shifted by
    /// `log2(accessSize)`).
    #[inline]
    pub fn emit_load_store_register_offset(
        &mut self,
        size: MemOpSize,
        op: MemOp,
        rt: RegisterID,
        rn: RegisterID,
        rm: RegisterID,
        extend: ExtendType,
        s: bool,
    ) {
        self.insn(load_store_register_register_offset(
            size, false, op, rm, extend, s, rn, rt,
        ));
    }
}

/// Bit width of a GPR `Datasize` (used by the immediate-shift bitfield math).
#[inline]
const fn datasize_bits(sf: Datasize) -> u32 {
    match sf {
        Datasize::D64 => 64,
        _ => 32,
    }
}

/// Access size in bytes for a single-register `MemOpSize`.
#[inline]
const fn mem_access_bytes(size: MemOpSize) -> u32 {
    1u32 << (size as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Byte oracle: the known-good prologue/epilogue byte constants from
    // src/jit/arm64_baseline/entry_prologue.rs:17-36. Reproduced here (cited)
    // so this new module stays self-contained and unwired; if the encoder
    // diverges from these bytes the asm the baseline JIT relies on is wrong.
    // ------------------------------------------------------------------------

    /// entry_prologue.rs:17-20 — `stp fp,lr,[sp,#-16]!` ; `mov fp,x1`.
    const RAW_C_ABI_PROLOGUE: [u8; 8] = [
        0xfd, 0x7b, 0xbf, 0xa9, // stp fp, lr, [sp, #-16]!
        0xfd, 0x03, 0x01, 0xaa, // mov fp, x1   (orr fp, xzr, x1)
    ];
    /// entry_prologue.rs:22-25 — `ldp fp,lr,[sp],#16` ; `ret`.
    const RAW_C_ABI_EPILOGUE: [u8; 8] = [
        0xfd, 0x7b, 0xc1, 0xa8, // ldp fp, lr, [sp], #16
        0xc0, 0x03, 0x5f, 0xd6, // ret
    ];
    /// entry_prologue.rs:27-30 — `stp fp,lr,[sp,#-16]!` ; `mov fp,sp`.
    const JSC_GENERATED_PROLOGUE: [u8; 8] = [
        0xfd, 0x7b, 0xbf, 0xa9, // stp fp, lr, [sp, #-16]!
        0xfd, 0x03, 0x00, 0x91, // mov fp, sp   (add fp, sp, #0)
    ];
    /// entry_prologue.rs:32-36 — `mov sp,fp` ; `ldp fp,lr,[sp],#16` ; `ret`.
    const JSC_GENERATED_EPILOGUE: [u8; 12] = [
        0xbf, 0x03, 0x00, 0x91, // mov sp, fp   (add sp, fp, #0)
        0xfd, 0x7b, 0xc1, 0xa8, // ldp fp, lr, [sp], #16
        0xc0, 0x03, 0x5f, 0xd6, // ret
    ];

    fn word(bytes: &[u8], index: usize) -> u32 {
        let s = index * 4;
        u32::from_le_bytes([bytes[s], bytes[s + 1], bytes[s + 2], bytes[s + 3]])
    }

    #[test]
    fn encoder_reproduces_raw_c_abi_prologue_bytes() {
        let mut buf = Vec::new();
        let mut enc = Arm64Encoder::new(&mut buf);
        // stp fp, lr, [sp, #-16]!  /  mov fp, x1
        enc.emit_stp_pre_index64(RegisterID::Fp, RegisterID::Lr, RegisterID::Sp, -16);
        enc.emit_mov(RegisterID::Fp, RegisterID::X1);
        assert_eq!(buf, RAW_C_ABI_PROLOGUE);
        assert_eq!(word(&buf, 0), 0xa9bf_7bfd);
        assert_eq!(word(&buf, 1), 0xaa01_03fd);
    }

    #[test]
    fn encoder_reproduces_raw_c_abi_epilogue_bytes() {
        let mut buf = Vec::new();
        let mut enc = Arm64Encoder::new(&mut buf);
        // ldp fp, lr, [sp], #16  /  ret
        enc.emit_ldp_post_index64(RegisterID::Fp, RegisterID::Lr, RegisterID::Sp, 16);
        enc.emit_ret();
        assert_eq!(buf, RAW_C_ABI_EPILOGUE);
        assert_eq!(word(&buf, 0), 0xa8c1_7bfd);
        assert_eq!(word(&buf, 1), 0xd65f_03c0);
    }

    #[test]
    fn encoder_reproduces_jsc_generated_prologue_bytes() {
        let mut buf = Vec::new();
        let mut enc = Arm64Encoder::new(&mut buf);
        // stp fp, lr, [sp, #-16]!  /  mov fp, sp
        enc.emit_stp_pre_index64(RegisterID::Fp, RegisterID::Lr, RegisterID::Sp, -16);
        enc.emit_mov(RegisterID::Fp, RegisterID::Sp);
        assert_eq!(buf, JSC_GENERATED_PROLOGUE);
        assert_eq!(word(&buf, 1), 0x9100_03fd);
    }

    #[test]
    fn encoder_reproduces_jsc_generated_epilogue_bytes() {
        let mut buf = Vec::new();
        let mut enc = Arm64Encoder::new(&mut buf);
        // mov sp, fp  /  ldp fp, lr, [sp], #16  /  ret
        enc.emit_mov(RegisterID::Sp, RegisterID::Fp);
        enc.emit_ldp_post_index64(RegisterID::Fp, RegisterID::Lr, RegisterID::Sp, 16);
        enc.emit_ret();
        assert_eq!(buf, JSC_GENERATED_EPILOGUE);
        assert_eq!(word(&buf, 0), 0x9100_03bf);
    }

    // ------------------------------------------------------------------------
    // Hand cross-checks against ARM64Assembler.h field layout.
    // ------------------------------------------------------------------------

    fn single(emit: impl FnOnce(&mut Arm64Encoder)) -> u32 {
        let mut buf = Vec::new();
        let mut enc = Arm64Encoder::new(&mut buf);
        emit(&mut enc);
        assert_eq!(buf.len(), 4);
        word(&buf, 0)
    }

    #[test]
    fn nop_is_canonical_hint() {
        // hintPseudo(0) -> system(0,0,3,2,0,0,zr) = 0xd503201f.
        assert_eq!(single(|e| e.emit_nop()), 0xd503_201f);
    }

    #[test]
    fn move_wide_immediate_matches() {
        // movz x0, #0x1234 : 0x12800000|sf|Z<<29|imm<<5 = 0xd2824680.
        assert_eq!(
            single(|e| e.emit_movz(RegisterID::X0, 0x1234, 0)),
            0xd282_4680
        );
        // movk x0, #0xabcd, lsl #16 : 0xf2b579a0.
        assert_eq!(
            single(|e| e.emit_movk(RegisterID::X0, 0xabcd, 16)),
            0xf2b5_79a0
        );
        // movn x0, #0 : 0x92800000.
        assert_eq!(single(|e| e.emit_movn(RegisterID::X0, 0, 0)), 0x9280_0000);
    }

    #[test]
    fn move_immediate64_sequence() {
        // 0x0000_abcd_0000_1234 -> movz x3,#0x1234 then movk x3,#0xabcd,lsl#32.
        let mut buf = Vec::new();
        let mut enc = Arm64Encoder::new(&mut buf);
        enc.emit_move_immediate64(RegisterID::X3, 0x0000_abcd_0000_1234);
        assert_eq!(buf.len(), 8);
        assert_eq!(word(&buf, 0), 0xd282_4683); // movz x3, #0x1234
        assert_eq!(word(&buf, 1), 0xf2d5_79a3); // movk x3, #0xabcd, lsl #32

        // Zero materializes as a single movz #0.
        let mut z = Vec::new();
        Arm64Encoder::new(&mut z).emit_move_immediate64(RegisterID::X0, 0);
        assert_eq!(z.len(), 4);
        assert_eq!(word(&z, 0), 0xd280_0000);
    }

    #[test]
    fn add_sub_immediate_and_register() {
        // add x0, x1, #4    : 0x91001020
        assert_eq!(
            single(|e| e.emit_add_imm12(RegisterID::X0, RegisterID::X1, 4, false)),
            0x9100_1020
        );
        // sub x0, x0, #1    : 0xd1000400
        assert_eq!(
            single(|e| e.emit_sub_imm12(RegisterID::X0, RegisterID::X0, 1, false)),
            0xd100_0400
        );
        // add x0, x1, x2    : 0x8b020020
        assert_eq!(
            single(|e| e.emit_add_reg(RegisterID::X0, RegisterID::X1, RegisterID::X2)),
            0x8b02_0020
        );
        // sub x0, x1, x2    : 0xcb020020
        assert_eq!(
            single(|e| e.emit_sub_reg(RegisterID::X0, RegisterID::X1, RegisterID::X2)),
            0xcb02_0020
        );
    }

    #[test]
    fn data_processing_3_source_mul_and_smull() {
        // mul w0, w1, w2 == madd w0, w1, w2, wzr : 0x1b027c20 (matches the
        // MacroAssemblerArm64 mul32 byte oracle).
        assert_eq!(
            single(|e| e.emit_mul(
                Datasize::D32,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2
            )),
            0x1b02_7c20
        );
        // mul x0, x1, x2 == madd x0, x1, x2, xzr : 0x9b027c20.
        assert_eq!(
            single(|e| e.emit_mul(
                Datasize::D64,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2
            )),
            0x9b02_7c20
        );
        // smull x0, w1, w2 == smaddl x0, w1, w2, xzr (DataOp_SMADDL=2 -> op31 bit
        // 21 set) : 0x9b227c20.
        assert_eq!(
            single(|e| e.emit_smull(RegisterID::X0, RegisterID::X1, RegisterID::X2)),
            0x9b22_7c20
        );
    }

    #[test]
    fn load_store_unsigned_immediate() {
        // ldr x0, [x1, #8]  : imm12 = 8/8 = 1 -> 0xf9400420
        assert_eq!(
            single(|e| e.emit_ldr_imm64(RegisterID::X0, RegisterID::X1, 8)),
            0xf940_0420
        );
        // str x2, [x1, #16] : imm12 = 16/8 = 2 -> 0xf9000822
        assert_eq!(
            single(|e| e.emit_str_imm64(RegisterID::X2, RegisterID::X1, 16)),
            0xf900_0822
        );
        // Cross-check against the in-tree baseline encoder constant
        // (arm64_baseline.rs:2406 expects 0xf9400ba9 = ldr x9, [x29, #16]).
        assert_eq!(
            single(|e| e.emit_ldr_imm64(RegisterID::X9, RegisterID::Fp, 16)),
            0xf940_0ba9
        );
    }

    #[test]
    fn load_store_register_offset_baseindex() {
        // str x0, [x1, x2, lsl #3] : size=64, opc=STORE, option=UXTX, S=1.
        // 0x38200800 | 0xC0000000 | (x2<<16) | (UXTX<<13) | (1<<12) | (x1<<5)
        //   = 0xf8226820? recompute: base 0x38200800 + size 0xC0000000 =
        //   0xf8200800; |2<<16=0xf8220800; |3<<13=0xf8226800; |1<<12=0xf8227800;
        //   |1<<5=0xf8227820.
        assert_eq!(
            single(|e| e.emit_str_register_offset64(
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
                ExtendType::Uxtx,
                Scale::TimesEight,
            )),
            0xf822_7820
        );
        // ldr x0, [x1, x2] (TimesOne => S=0) : 0xf8626820? base 0xf8200800
        //   |opc LOAD 1<<22=0x400000 -> 0xf8600800; |x2<<16=0xf8620800;
        //   |UXTX<<13=0xf8626800; |S=0; |x1<<5=0xf8626820; rt=0.
        assert_eq!(
            single(|e| e.emit_ldr_register_offset64(
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
                ExtendType::Uxtx,
                Scale::TimesOne,
            )),
            0xf862_6820
        );
    }

    #[test]
    fn pair_offset_form_no_writeback() {
        // stp x29, x30, [sp, #16] (signed offset) : 0xa9015bfd.
        // base 0x29000000 | size 0x80000000 | imm7 (16/8=2)<<15=0x10000 |
        //   x30<<10=0x7800 | sp<<5=0x3e0 | x29=0x1d -> 0xa9015bfd? compute:
        //   0xa9000000|0x10000=0xa9010000;|0x7800=0xa9017800;|0x3e0=0xa90 17be0;
        //   |0x1d=0xa9017bfd.
        assert_eq!(
            single(|e| e.emit_stp_offset64(RegisterID::Fp, RegisterID::Lr, RegisterID::Sp, 16)),
            0xa901_7bfd
        );
        // ldp x29, x30, [sp, #16] : opc LOAD adds 0x400000 -> 0xa9417bfd.
        assert_eq!(
            single(|e| e.emit_ldp_offset64(RegisterID::Fp, RegisterID::Lr, RegisterID::Sp, 16)),
            0xa941_7bfd
        );
    }

    #[test]
    fn branch_family() {
        // b #0 (unlinked) : 0x14000000 ; bl #0 : 0x94000000.
        assert_eq!(single(|e| e.emit_b(0)), 0x1400_0000);
        assert_eq!(single(|e| e.emit_bl(0)), 0x9400_0000);
        // b #(2 words forward) : imm26 = 2 -> 0x14000002.
        assert_eq!(single(|e| e.emit_b(2)), 0x1400_0002);
        // b.eq #0 : 0x54000000 ; b.ne #(1 word) : 0x54000021.
        assert_eq!(single(|e| e.emit_b_cond(Condition::Eq, 0)), 0x5400_0000);
        assert_eq!(single(|e| e.emit_b_cond(Condition::Ne, 1)), 0x5400_0021);
        // blr x0 : 0xd63f0000 ; br x2 : 0xd61f0040 ; ret : 0xd65f03c0.
        assert_eq!(single(|e| e.emit_blr(RegisterID::X0)), 0xd63f_0000);
        assert_eq!(single(|e| e.emit_br(RegisterID::X2)), 0xd61f_0040);
        assert_eq!(single(|e| e.emit_ret()), 0xd65f_03c0);
    }

    // ------------------------------------------------------------------------
    // Datasize-generic helpers (the MacroAssembler-facing surface). Every word
    // below is hand-encoded against the ARM64 ARM and cross-checked with a
    // disassembler-style derivation.
    // ------------------------------------------------------------------------

    #[test]
    fn datasize_generic_add_sub_logical() {
        // add w0, w1, w2 : 0x0b020020 ; sub w0, w1, w2 : 0x4b020020.
        assert_eq!(
            single(|e| e.emit_add_sub_reg(
                Datasize::D32,
                AddOp::Add,
                SetFlags::DontSetFlags,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x0b02_0020
        );
        assert_eq!(
            single(|e| e.emit_add_sub_reg(
                Datasize::D32,
                AddOp::Sub,
                SetFlags::DontSetFlags,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x4b02_0020
        );
        // add w0, w1, #4 : 0x11001020.
        assert_eq!(
            single(|e| e.emit_add_sub_imm(
                Datasize::D32,
                AddOp::Add,
                SetFlags::DontSetFlags,
                RegisterID::X0,
                RegisterID::X1,
                4,
                false,
            )),
            0x1100_1020
        );
        // cmp w0, w1 (subs wzr, w0, w1) : 0x6b01001f.
        assert_eq!(
            single(|e| e.emit_add_sub_reg(
                Datasize::D32,
                AddOp::Sub,
                SetFlags::S,
                RegisterID::Zr,
                RegisterID::X0,
                RegisterID::X1,
            )),
            0x6b01_001f
        );
        // cmp w0, #4 (subs wzr, w0, #4) : 0x7100101f.
        assert_eq!(
            single(|e| e.emit_add_sub_imm(
                Datasize::D32,
                AddOp::Sub,
                SetFlags::S,
                RegisterID::Zr,
                RegisterID::X0,
                4,
                false,
            )),
            0x7100_101f
        );
        // and w0, w1, w2 : 0x0a020020 ; orr : 0x2a020020 ; eor : 0x4a020020.
        assert_eq!(
            single(|e| e.emit_logical_reg(
                Datasize::D32,
                LogicalOp::And,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x0a02_0020
        );
        assert_eq!(
            single(|e| e.emit_logical_reg(
                Datasize::D32,
                LogicalOp::Orr,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x2a02_0020
        );
        assert_eq!(
            single(|e| e.emit_logical_reg(
                Datasize::D32,
                LogicalOp::Eor,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x4a02_0020
        );
        // tst w0, w1 (ands wzr, w0, w1) : 0x6a01001f.
        assert_eq!(
            single(|e| e.emit_logical_reg(
                Datasize::D32,
                LogicalOp::Ands,
                RegisterID::Zr,
                RegisterID::X0,
                RegisterID::X1,
            )),
            0x6a01_001f
        );
    }

    #[test]
    fn datasize_generic_shifts_and_mul() {
        // lsl w0, w1, w2 : 0x1ac22020 ; lsr : 0x1ac22420 ; asr : 0x1ac22820.
        assert_eq!(
            single(|e| e.emit_shift_reg(
                Datasize::D32,
                DataOp2Source::Lslv,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x1ac2_2020
        );
        assert_eq!(
            single(|e| e.emit_shift_reg(
                Datasize::D32,
                DataOp2Source::Lsrv,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x1ac2_2420
        );
        assert_eq!(
            single(|e| e.emit_shift_reg(
                Datasize::D32,
                DataOp2Source::Asrv,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
            )),
            0x1ac2_2820
        );
        // lsl w0, w1, #1 : 0x531f7820 ; lsr w0, w1, #1 : 0x53017c20 ;
        // asr w0, w1, #1 : 0x13017c20.
        assert_eq!(
            single(|e| e.emit_lsl_imm(Datasize::D32, RegisterID::X0, RegisterID::X1, 1)),
            0x531f_7820
        );
        assert_eq!(
            single(|e| e.emit_lsr_imm(Datasize::D32, RegisterID::X0, RegisterID::X1, 1)),
            0x5301_7c20
        );
        assert_eq!(
            single(|e| e.emit_asr_imm(Datasize::D32, RegisterID::X0, RegisterID::X1, 1)),
            0x1301_7c20
        );
        // mul w0, w1, w2 (madd w0,w1,w2,wzr) : 0x1b027c20.
        assert_eq!(
            single(|e| e.emit_mul(
                Datasize::D32,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2
            )),
            0x1b02_7c20
        );
    }

    #[test]
    fn datasize_generic_moves_and_fp() {
        // movz w0, #0x1234 : 0x52824680 ; mov w0, w1 (orr w0, wzr, w1) : 0x2a0103e0.
        assert_eq!(
            single(|e| e.emit_move_wide(Datasize::D32, MoveWideOp::Z, RegisterID::X0, 0x1234, 0)),
            0x5282_4680
        );
        assert_eq!(
            single(|e| e.emit_mov_reg(Datasize::D32, RegisterID::X0, RegisterID::X1)),
            0x2a01_03e0
        );
        // fmov d0, d1 : 0x1e604020.
        assert_eq!(
            single(|e| e.emit_fmov_double(FPRegisterID::Q0, FPRegisterID::Q1)),
            0x1e60_4020
        );
    }

    // ------------------------------------------------------------------------
    // FP scalar-double arithmetic + FP<->integer conversions (the baseline
    // double fast-path primitives). Byte oracle: real assembled words for the
    // double forms (`<64>`), cross-checked against the ARM ARM encodings.
    // ------------------------------------------------------------------------
    #[test]
    fn fp_scalar_double_arith() {
        // fadd d0, d1, d2 : 0x1e622820 ; fsub d0, d1, d2 : 0x1e623820 ;
        // fmul d0, d1, d2 : 0x1e620820 ; fdiv d0, d1, d2 : 0x1e621820.
        assert_eq!(
            single(|e| e.emit_fadd_double(FPRegisterID::Q0, FPRegisterID::Q1, FPRegisterID::Q2)),
            0x1e62_2820
        );
        assert_eq!(
            single(|e| e.emit_fsub_double(FPRegisterID::Q0, FPRegisterID::Q1, FPRegisterID::Q2)),
            0x1e62_3820
        );
        assert_eq!(
            single(|e| e.emit_fmul_double(FPRegisterID::Q0, FPRegisterID::Q1, FPRegisterID::Q2)),
            0x1e62_0820
        );
        assert_eq!(
            single(|e| e.emit_fdiv_double(FPRegisterID::Q0, FPRegisterID::Q1, FPRegisterID::Q2)),
            0x1e62_1820
        );
        // Field-placement check: fadd d3, d4, d5 : 0x1e652883.
        assert_eq!(
            single(|e| e.emit_fadd_double(FPRegisterID::Q3, FPRegisterID::Q4, FPRegisterID::Q5)),
            0x1e65_2883
        );
    }

    #[test]
    fn fp_integer_conversions() {
        // scvtf d0, w1 : 0x1e620020 (signed W -> double).
        assert_eq!(
            single(|e| e.emit_scvtf_int32_to_double(FPRegisterID::Q0, RegisterID::X1)),
            0x1e62_0020
        );
        // fmov x0, d1 : 0x9e660020 (moveDoubleTo64, Xd <- Dn bit-cast).
        assert_eq!(
            single(|e| e.emit_fmov_double_to_gpr(RegisterID::X0, FPRegisterID::Q1)),
            0x9e66_0020
        );
        // fmov d0, x1 : 0x9e670020 (move64ToDouble, Dd <- Xn bit-cast).
        assert_eq!(
            single(|e| e.emit_fmov_gpr_to_double(FPRegisterID::Q0, RegisterID::X1)),
            0x9e67_0020
        );
    }

    #[test]
    fn datasize_generic_memory_forms() {
        // ldur x0, [x1, #-8] : 0xf85f8020 ; stur w0, [x1, #-4] : 0xb81fc020.
        assert_eq!(
            single(|e| e.emit_load_store_unscaled(
                MemOpSize::Size64,
                MemOp::Load,
                RegisterID::X0,
                RegisterID::X1,
                -8,
            )),
            0xf85f_8020
        );
        assert_eq!(
            single(|e| e.emit_load_store_unscaled(
                MemOpSize::Size32,
                MemOp::Store,
                RegisterID::X0,
                RegisterID::X1,
                -4,
            )),
            0xb81f_c020
        );
        // ldr w0, [x1, #8] : imm12 = 8/4 = 2 -> 0xb9400820.
        assert_eq!(
            single(|e| e.emit_load_store_unsigned(
                MemOpSize::Size32,
                MemOp::Load,
                RegisterID::X0,
                RegisterID::X1,
                8,
            )),
            0xb940_0820
        );
        // ldrb w0, [x1, #1] : imm12 = 1 -> 0x39400420.
        assert_eq!(
            single(|e| e.emit_load_store_unsigned(
                MemOpSize::Size8Or128,
                MemOp::Load,
                RegisterID::X0,
                RegisterID::X1,
                1,
            )),
            0x3940_0420
        );
        // ldr w0, [x1, x2, lsl #2] : size=32, S=1, UXTX -> 0xb8627820.
        assert_eq!(
            single(|e| e.emit_load_store_register_offset(
                MemOpSize::Size32,
                MemOp::Load,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
                ExtendType::Uxtx,
                true,
            )),
            0xb862_7820
        );
        // ldrb w0, [x1, x2] : size=8, S=0, UXTX -> 0x38626820.
        assert_eq!(
            single(|e| e.emit_load_store_register_offset(
                MemOpSize::Size8Or128,
                MemOp::Load,
                RegisterID::X0,
                RegisterID::X1,
                RegisterID::X2,
                ExtendType::Uxtx,
                false,
            )),
            0x3862_6820
        );
    }
}
