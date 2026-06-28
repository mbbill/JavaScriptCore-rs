//! ARM64 link records and the in-place branch/call relocation pass.
//!
//! Faithful port of the per-architecture linking/patching layer of
//! `Source/JavaScriptCore/assembler/ARM64Assembler.h`:
//!
//! - `JumpType` / `JumpLinkType` (ARM64Assembler.h:323-342) with their
//!   size-in-the-high-bits `JUMP_ENUM_WITH_SIZE` encoding.
//! - `BranchType` (ARM64Assembler.h:344-348).
//! - `LinkRecord` (ARM64Assembler.h:355-477): the `from`/`to`/`type`/`condition`/
//!   `branchType` tuple the assembler accumulates as it emits jumps and calls
//!   whose targets are not yet known.
//! - `computeJumpType` (ARM64Assembler.h:4062-4131): pick the concrete link form
//!   (direct one-instruction vs indirect two-instruction) from the link-time
//!   distance.
//! - `link` / `linkJumpOrCall` / `linkConditionalBranch` (ARM64Assembler.h:
//!   4139-4310): patch the `b` / `bl` / `b.cond` instruction word *in place* in
//!   the code buffer with the resolved relative displacement.
//!
//! JSC deliberately keeps this layer per-architecture inside `ARM64Assembler`
//! rather than in the shared `AbstractMacroAssembler` — see the mcts_mem
//! `assembler/buffer-label-linking` move dated 2009-07-22
//! (`link-repatch-buffer-nested-in-abstract-macro-assembler`): per-arch
//! `linkCall`/`relinkCall` cannot live in the base class. So a dedicated ARM64
//! module here is the faithful structure, not a Rust-only split.
//!
//! This is PURE-SAFE byte computation: it resolves label offsets and rewrites
//! immediate fields inside an in-memory `&mut [u8]` code buffer. It does NOT
//! touch executable memory, `mprotect`, or the icache — the W^X / repatch
//! execution boundary (JSC's `relink*` / `RepatchBuffer`) is a separate,
//! orchestrator-owned step and is out of scope here. The module stays UNWIRED
//! dead code until the baseline JIT is rewired to emit through it.
#![allow(dead_code)]

use super::arm64_encoder::{
    conditional_branch_immediate, unconditional_branch_immediate, Condition,
};
use super::registers::RegisterID;

// ----------------------------------------------------------------------------
// JumpType / JumpLinkType — faithful mirror of ARM64Assembler.h:321-342.
//
// JSC packs the linked byte-size of each jump form into the high nibble of the
// enum value via `JUMP_ENUM_WITH_SIZE(index, value) = (value << 4) | index`, so
// `JUMP_ENUM_SIZE(j) = j >> 4` recovers the size in bytes without a table
// lookup (mcts_mem buffer-label-linking move 2011-07-07: encoding sizes in the
// enum lets the compiler constant-fold all linking arithmetic).
// ----------------------------------------------------------------------------

/// `ARM64Assembler::JumpType` (ARM64Assembler.h:323-332). Discriminants are the
/// `JUMP_ENUM_WITH_SIZE(index, value)` packed bytes (`(value << 4) | index`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum JumpType {
    /// index 0, size 0.
    JumpFixed = (0 << 4) | 0,
    /// index 1, size 1 word.
    JumpNoCondition = (4 << 4) | 1,
    /// index 2, size 2 words.
    JumpCondition = (8 << 4) | 2,
    /// index 3, size 2 words.
    JumpCompareAndBranch = (8 << 4) | 3,
    /// index 4, size 2 words.
    JumpTestBit = (8 << 4) | 4,
    /// index 5, size 1 word.
    JumpNoConditionFixedSize = (4 << 4) | 5,
    /// index 6, size 2 words.
    JumpConditionFixedSize = (8 << 4) | 6,
    /// index 7, size 2 words.
    JumpCompareAndBranchFixedSize = (8 << 4) | 7,
    /// index 8, size 2 words.
    JumpTestBitFixedSize = (8 << 4) | 8,
}

impl JumpType {
    /// `JUMP_ENUM_SIZE(jump)` (ARM64Assembler.h:322): linked size in bytes.
    #[inline]
    pub const fn size(self) -> i32 {
        (self as u8 as i32) >> 4
    }
}

/// `ARM64Assembler::JumpLinkType` (ARM64Assembler.h:333-342). Same
/// `JUMP_ENUM_WITH_SIZE` packing as [`JumpType`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum JumpLinkType {
    /// index 0, size 0.
    LinkInvalid = (0 << 4) | 0,
    /// index 1, size 1 word.
    LinkJumpNoCondition = (4 << 4) | 1,
    /// index 2, size 1 word — direct one-instruction `b.cond`.
    LinkJumpConditionDirect = (4 << 4) | 2,
    /// index 3, size 2 words — inverted `b.cond` over an unconditional `b`.
    LinkJumpCondition = (8 << 4) | 3,
    /// index 4, size 2 words.
    LinkJumpCompareAndBranch = (8 << 4) | 4,
    /// index 5, size 1 word.
    LinkJumpCompareAndBranchDirect = (4 << 4) | 5,
    /// index 6, size 2 words.
    LinkJumpTestBit = (8 << 4) | 6,
    /// index 7, size 1 word.
    LinkJumpTestBitDirect = (4 << 4) | 7,
}

impl JumpLinkType {
    /// `JUMP_ENUM_SIZE(jump)` (ARM64Assembler.h:322): linked size in bytes.
    #[inline]
    pub const fn size(self) -> i32 {
        (self as u8 as i32) >> 4
    }
}

/// `ARM64Assembler::BranchType` (ARM64Assembler.h:344-348).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BranchType {
    /// `b` — unconditional branch.
    Jmp = 0,
    /// `bl` — branch with link (near call).
    Call = 1,
    /// `ret` — never a relocation target.
    Ret = 2,
}

/// `ARM64Assembler::ThunkOrNot` (ARM64Assembler.h:350-353). Distinguishes a
/// link to JIT code in the same buffer from a link to a fixed thunk address;
/// only changes the `computeJumpType` distance bias.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThunkOrNot {
    NotThunk,
    Thunk,
}

// ----------------------------------------------------------------------------
// Range / field helpers.
// ----------------------------------------------------------------------------

/// `WTF::isInt<bits>` (AssemblerCommon.h:57-63): does `value` fit in a signed
/// `BITS`-bit field? Faithful `(t << shift) >> shift == t` form; Rust's `>>` on
/// a signed integer is arithmetic, matching C++'s signed shift.
#[inline]
const fn is_int<const BITS: u32>(value: i64) -> bool {
    let shift = 64 - BITS;
    (value << shift) >> shift == value
}

/// `jumpSizeDelta` (ARM64Assembler.h:4053): bytes saved by compacting `jumpType`
/// to `jumpLinkType`. Used only to bias the thunk distance in `computeJumpType`.
#[inline]
const fn jump_size_delta(jump_type: JumpType, link_type: JumpLinkType) -> i32 {
    jump_type.size() - link_type.size()
}

// ----------------------------------------------------------------------------
// Errors. JSC `RELEASE_ASSERT`s these invariants (the JIT pre-guarantees them);
// this safe port surfaces them as values so the relocation pass never panics.
// ----------------------------------------------------------------------------

/// A relocation that could not be applied in place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arm64LinkError {
    /// A `from`/`to` byte offset is not 4-byte aligned. ARM64 instructions are
    /// fixed 4-byte words (JSC asserts `is4ByteAligned`, ARM64Assembler.h:4080).
    Misaligned { at: u32 },
    /// The patch site `[at, at+4)` lies outside the code buffer.
    OutOfBounds { at: u32, len: usize },
    /// The resolved word displacement does not fit the branch immediate field.
    /// JSC's fallback is the multi-instruction expansion / jump island
    /// (ARM64Assembler.h:4244-4253) — a separate deferred step; here it is a
    /// flagged error rather than a silent expansion.
    OutOfRange { offset_words: i64, field_bits: u32 },
    /// The link form needs the multi-instruction expansion path (far/indirect
    /// conditional, compare-and-branch, or test-bit). The in-place single-word
    /// patch core does not emit those yet (flagged: serial coupling).
    ExpansionUnsupported(JumpLinkType),
    /// `BranchType::Ret` is not a relocation target (ARM64Assembler.h:4152).
    NotRelocatable,
}

// ----------------------------------------------------------------------------
// LinkRecord — faithful mirror of ARM64Assembler.h:355-477.
// ----------------------------------------------------------------------------

/// `ARM64Assembler::LinkRecord` (ARM64Assembler.h:355-477): one accumulated
/// jump/call whose target is resolved at link time. `from`/`to` are byte
/// offsets into the code buffer (JSC stores `intptr_t` buffer offsets via
/// `AssemblerLabel::offset()`).
///
/// Divergence (commented per contract): JSC's ARM64E build pointer-authenticates
/// `m_to` with a key derived from the assembler address (ARM64Assembler.h:
/// 360-365). This port targets the non-ARM64E layout (the `#else` branch:
/// `m_to = to` raw), so the PAC tagging is intentionally omitted. The union /
/// bitfield packing (`RealTypes`/`CopyTypes`, ARM64Assembler.h:455-476) is a C++
/// copy-perf micro-optimization with no Rust equivalent and is not mirrored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Arm64LinkRecord {
    from: i64,
    to: i64,
    type_: JumpType,
    link_type: JumpLinkType,
    condition: Condition,
    branch_type: BranchType,
    is_64bit: bool,
    bit_number: u32,
    compare_register: Option<RegisterID>,
    is_thunk: ThunkOrNot,
}

impl Arm64LinkRecord {
    /// Call form: `LinkRecord(assembler, from, to, isThunk)`
    /// (ARM64Assembler.h:357-368). `m_branchType = BranchType_CALL`, `m_type`
    /// defaults to `JumpNoCondition`.
    #[inline]
    pub fn new_call(from: i64, to: i64) -> Self {
        Self {
            from,
            to,
            type_: JumpType::JumpNoCondition,
            link_type: JumpLinkType::LinkInvalid,
            condition: Condition::Invalid,
            branch_type: BranchType::Call,
            is_64bit: false,
            bit_number: 0,
            compare_register: None,
            is_thunk: ThunkOrNot::NotThunk,
        }
    }

    /// Jump form: `LinkRecord(assembler, from, to, type, condition, isThunk)`
    /// (ARM64Assembler.h:370-382). Covers both the unconditional `b`
    /// (`JumpNoCondition`, `condition = Invalid`) and the conditional `b.cond`
    /// (`JumpCondition`) that `MacroAssemblerARM64::jump`/`makeBranch` emit
    /// (MacroAssemblerARM64.h:5389-5394, 7501-7508). `m_branchType` stays
    /// `BranchType_JMP`.
    #[inline]
    pub fn new_jump(from: i64, to: i64, type_: JumpType, condition: Condition) -> Self {
        Self {
            from,
            to,
            type_,
            link_type: JumpLinkType::LinkInvalid,
            condition,
            branch_type: BranchType::Jmp,
            is_64bit: false,
            bit_number: 0,
            compare_register: None,
            is_thunk: ThunkOrNot::NotThunk,
        }
    }

    /// Mark this record as targeting a fixed thunk address rather than in-buffer
    /// JIT code (ARM64Assembler.h `ThunkOrNot::Thunk`); only biases the distance
    /// in [`Self::compute_jump_type`].
    #[inline]
    pub fn as_thunk(mut self) -> Self {
        self.is_thunk = ThunkOrNot::Thunk;
        self
    }

    /// `from()` (ARM64Assembler.h:425): branch instruction byte offset.
    #[inline]
    pub fn from(&self) -> i64 {
        self.from
    }

    /// `to()` (ARM64Assembler.h:435-443): target byte offset (untagged, see the
    /// ARM64E divergence note on the struct).
    #[inline]
    pub fn to(&self) -> i64 {
        self.to
    }

    /// `type()` (ARM64Assembler.h:444).
    #[inline]
    pub fn jump_type(&self) -> JumpType {
        self.type_
    }

    /// `linkType()` (ARM64Assembler.h:445): the concrete form, valid only after
    /// [`Self::compute_jump_type`].
    #[inline]
    pub fn link_type(&self) -> JumpLinkType {
        self.link_type
    }

    /// `branchType()` (ARM64Assembler.h:446).
    #[inline]
    pub fn branch_type(&self) -> BranchType {
        self.branch_type
    }

    /// `condition()` (ARM64Assembler.h:448).
    #[inline]
    pub fn condition(&self) -> Condition {
        self.condition
    }

    #[inline]
    fn is_thunk(&self) -> bool {
        matches!(self.is_thunk, ThunkOrNot::Thunk)
    }

    /// `computeJumpType(record, from, to)` (ARM64Assembler.h:4062-4131): choose
    /// the concrete [`JumpLinkType`] from the link-time byte distance and record
    /// it via `setLinkType`. `from`/`to` are byte offsets (JSC passes the
    /// instruction-stream pointers; the relative distance is identical).
    pub fn compute_jump_type(&mut self) -> JumpLinkType {
        let link_type = match self.type_ {
            JumpType::JumpFixed => JumpLinkType::LinkInvalid,
            JumpType::JumpNoConditionFixedSize => JumpLinkType::LinkJumpNoCondition,
            JumpType::JumpConditionFixedSize => JumpLinkType::LinkJumpCondition,
            JumpType::JumpCompareAndBranchFixedSize => JumpLinkType::LinkJumpCompareAndBranch,
            JumpType::JumpTestBitFixedSize => JumpLinkType::LinkJumpTestBit,
            JumpType::JumpNoCondition => JumpLinkType::LinkJumpNoCondition,
            JumpType::JumpCondition => {
                let mut relative = self.to - self.from;
                if self.is_thunk() {
                    relative +=
                        jump_size_delta(self.type_, JumpLinkType::LinkJumpConditionDirect) as i64;
                }
                if is_int::<21>(relative) {
                    JumpLinkType::LinkJumpConditionDirect
                } else {
                    JumpLinkType::LinkJumpCondition
                }
            }
            JumpType::JumpCompareAndBranch => {
                let mut relative = self.to - self.from;
                if self.is_thunk() {
                    relative +=
                        jump_size_delta(self.type_, JumpLinkType::LinkJumpCompareAndBranchDirect)
                            as i64;
                }
                if is_int::<21>(relative) {
                    JumpLinkType::LinkJumpCompareAndBranchDirect
                } else {
                    JumpLinkType::LinkJumpCompareAndBranch
                }
            }
            JumpType::JumpTestBit => {
                let mut relative = self.to - self.from;
                if self.is_thunk() {
                    relative +=
                        jump_size_delta(self.type_, JumpLinkType::LinkJumpTestBitDirect) as i64;
                }
                if is_int::<14>(relative) {
                    JumpLinkType::LinkJumpTestBitDirect
                } else {
                    JumpLinkType::LinkJumpTestBit
                }
            }
        };
        self.link_type = link_type;
        link_type
    }

    /// `link(record, from, fromInstruction, to)` (ARM64Assembler.h:4139-4180):
    /// compute the link form and patch the branch word(s) in `code` in place.
    ///
    /// In this no-compaction core, `from == fromInstruction == self.from` (the
    /// branch's own byte offset), so a single offset drives both the
    /// displacement math and the write site. JSC's compaction pass
    /// (`LinkBuffer::copyCompactAndLinkCode`) splits `from` (post-shift write
    /// site) from `fromInstruction` (original distance site); that code-shifting
    /// layer is deferred and flagged as a serial coupling.
    pub fn link(&mut self, code: &mut [u8]) -> Result<(), Arm64LinkError> {
        let link_type = self.compute_jump_type();
        match link_type {
            JumpLinkType::LinkJumpNoCondition => match self.branch_type {
                BranchType::Jmp => link_jump_or_call(code, self.from, self.to, false),
                BranchType::Call => link_jump_or_call(code, self.from, self.to, true),
                BranchType::Ret => Err(Arm64LinkError::NotRelocatable),
            },
            JumpLinkType::LinkJumpConditionDirect => {
                link_conditional_branch_direct(code, self.from, self.to, self.condition)
            }
            // The two-instruction indirect forms and the compare-and-branch /
            // test-bit families need the expansion path JSC implements in
            // linkConditionalBranch<IndirectBranch> / linkCompareAndBranch /
            // linkTestAndBranch (ARM64Assembler.h:4260-4337). Deferred.
            other => Err(Arm64LinkError::ExpansionUnsupported(other)),
        }
    }
}

// ----------------------------------------------------------------------------
// In-place patch primitives — faithful to ARM64Assembler.h:4226-4310.
// ----------------------------------------------------------------------------

#[inline]
fn check_aligned(byte_offset: i64) -> Result<(), Arm64LinkError> {
    if byte_offset & 3 != 0 {
        return Err(Arm64LinkError::Misaligned {
            at: byte_offset as u32,
        });
    }
    Ok(())
}

/// Read the little-endian 32-bit instruction word at `byte_offset`.
fn read_word_le(code: &[u8], byte_offset: i64) -> Result<u32, Arm64LinkError> {
    if byte_offset < 0 || (byte_offset as usize) + 4 > code.len() {
        return Err(Arm64LinkError::OutOfBounds {
            at: byte_offset as u32,
            len: code.len(),
        });
    }
    let i = byte_offset as usize;
    Ok(u32::from_le_bytes([
        code[i],
        code[i + 1],
        code[i + 2],
        code[i + 3],
    ]))
}

/// Write `word` little-endian at `byte_offset`. Mirrors JSC's
/// `machineCodeCopy<repatch>(from, &insn, sizeof(int))` (ARM64Assembler.h:4257)
/// onto unprotected buffer bytes (the safe, non-executable path).
fn write_word_le(code: &mut [u8], byte_offset: i64, word: u32) -> Result<(), Arm64LinkError> {
    if byte_offset < 0 || (byte_offset as usize) + 4 > code.len() {
        return Err(Arm64LinkError::OutOfBounds {
            at: byte_offset as u32,
            len: code.len(),
        });
    }
    let i = byte_offset as usize;
    code[i..i + 4].copy_from_slice(&word.to_le_bytes());
    Ok(())
}

/// `linkJumpOrCall<type>(from, fromInstruction, to)` (ARM64Assembler.h:
/// 4226-4258): rewrite the `b`/`bl` word with `imm26 = (to - from) >> 2`.
/// `is_call` is the `BranchType_CALL` link bit (`bl`).
pub fn link_jump_or_call(
    code: &mut [u8],
    from: i64,
    to: i64,
    is_call: bool,
) -> Result<(), Arm64LinkError> {
    check_aligned(from)?;
    check_aligned(to)?;
    // JSC: offset = (to - fromInstruction) >> 2; with from == fromInstruction.
    let offset = (to - from) >> 2;
    if !is_int::<26>(offset) {
        return Err(Arm64LinkError::OutOfRange {
            offset_words: offset,
            field_bits: 26,
        });
    }
    let insn = unconditional_branch_immediate(is_call, offset as i32);
    write_word_le(code, from, insn)
}

/// `linkConditionalBranch<DirectBranch>(condition, from, fromInstruction, to)`
/// (ARM64Assembler.h:4287-4310, the `useDirect`/`DirectBranch` arm): rewrite the
/// single `b.cond` word with `imm19 = (to - from) >> 2`.
///
/// When the displacement does not fit `imm19`, JSC falls back to the indirect
/// two-instruction form (inverted `b.cond` skipping an unconditional `b`); that
/// expansion is deferred, so here an out-of-range displacement is a flagged
/// [`Arm64LinkError::OutOfRange`] rather than a silent expansion.
pub fn link_conditional_branch_direct(
    code: &mut [u8],
    from: i64,
    to: i64,
    condition: Condition,
) -> Result<(), Arm64LinkError> {
    check_aligned(from)?;
    check_aligned(to)?;
    let offset = (to - from) >> 2;
    // JSC: bool useDirect = isInt<19>(offset).
    if !is_int::<19>(offset) {
        return Err(Arm64LinkError::OutOfRange {
            offset_words: offset,
            field_bits: 19,
        });
    }
    let insn = conditional_branch_immediate(offset as i32, condition);
    write_word_le(code, from, insn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::arm64_encoder::Arm64Encoder;

    fn word(bytes: &[u8], index: usize) -> u32 {
        let s = index * 4;
        u32::from_le_bytes([bytes[s], bytes[s + 1], bytes[s + 2], bytes[s + 3]])
    }

    // ------------------------------------------------------------------------
    // Enum packing: the JUMP_ENUM_WITH_SIZE byte sizes (ARM64Assembler.h:
    // 321-342) must round-trip through `size()`.
    // ------------------------------------------------------------------------
    #[test]
    fn jump_enum_sizes_match_jsc() {
        assert_eq!(JumpType::JumpFixed.size(), 0);
        assert_eq!(JumpType::JumpNoCondition.size(), 4);
        assert_eq!(JumpType::JumpCondition.size(), 8);
        assert_eq!(JumpType::JumpNoConditionFixedSize.size(), 4);
        assert_eq!(JumpType::JumpConditionFixedSize.size(), 8);
        assert_eq!(JumpLinkType::LinkInvalid.size(), 0);
        assert_eq!(JumpLinkType::LinkJumpNoCondition.size(), 4);
        assert_eq!(JumpLinkType::LinkJumpConditionDirect.size(), 4);
        assert_eq!(JumpLinkType::LinkJumpCondition.size(), 8);
        // index nibble is preserved distinctly per variant (no collisions).
        assert_eq!(JumpType::JumpNoCondition as u8, (4 << 4) | 1);
        assert_eq!(JumpLinkType::LinkJumpConditionDirect as u8, (4 << 4) | 2);
    }

    #[test]
    fn is_int_matches_signed_field_bounds() {
        // 26-bit signed word field: [-2^25, 2^25).
        assert!(is_int::<26>((1 << 25) - 1));
        assert!(is_int::<26>(-(1 << 25)));
        assert!(!is_int::<26>(1 << 25));
        assert!(!is_int::<26>(-(1 << 25) - 1));
        // 19-bit signed word field: [-2^18, 2^18).
        assert!(is_int::<19>((1 << 18) - 1));
        assert!(is_int::<19>(-(1 << 18)));
        assert!(!is_int::<19>(1 << 18));
    }

    // ------------------------------------------------------------------------
    // PROOF 1 — forward conditional branch (b.eq) to a label defined later.
    //
    // Emit `b.eq #0` (unlinked placeholder) at offset 0, then a NOP filler, then
    // a `ret` at the label (offset 8). Register the LinkRecord, run the link
    // pass, and assert imm19 == (to - from)>>2 exactly.
    //
    // Encoding pinned to ARM64Assembler.h:4542-4550 (conditionalBranchImmediate:
    // imm19 << 5) and the DirectBranch write at ARM64Assembler.h:4299.
    // Hand cross-check: b.eq .+8 = 0x5400_0000 | (2 << 5) | 0 = 0x5400_0040.
    // ------------------------------------------------------------------------
    #[test]
    fn forward_conditional_branch_patches_imm19_byte_exact() {
        let mut code = Vec::new();
        {
            let mut enc = Arm64Encoder::new(&mut code);
            enc.emit_b_cond(Condition::Eq, 0); // offset 0: b.eq #0 (unlinked)
            enc.emit_nop(); // offset 4: filler
            enc.emit_ret(); // offset 8: label / ret
        }
        assert_eq!(word(&code, 0), 0x5400_0000, "unlinked b.eq placeholder");

        let mut record = Arm64LinkRecord::new_jump(0, 8, JumpType::JumpCondition, Condition::Eq);
        assert_eq!(
            record.compute_jump_type(),
            JumpLinkType::LinkJumpConditionDirect
        );
        record.link(&mut code).expect("link b.eq");

        // imm19 = (8 - 0) >> 2 = 2  =>  word = 0x5400_0040.
        let patched = word(&code, 0);
        assert_eq!(patched, 0x5400_0040);
        let imm19 = ((patched >> 5) & 0x0007_ffff) as i32;
        assert_eq!(imm19, 2, "imm19 must encode (target_byte - branch_byte)>>2");
        // Untouched neighbours.
        assert_eq!(word(&code, 1), 0xd503_201f, "nop preserved");
        assert_eq!(word(&code, 2), 0xd65f_03c0, "ret preserved");
    }

    // ------------------------------------------------------------------------
    // PROOF 2 — bl (near call) relocation, imm26.
    //
    // Encoding pinned to ARM64Assembler.h:4879-4883 (unconditionalBranchImmediate
    // with link bit) and the linkJumpOrCall write at ARM64Assembler.h:4255.
    // Hand cross-check: bl .+12 = 0x1400_0000 | (1<<31) | 3 = 0x9400_0003.
    // ------------------------------------------------------------------------
    #[test]
    fn forward_call_patches_imm26_byte_exact() {
        let mut code = Vec::new();
        {
            let mut enc = Arm64Encoder::new(&mut code);
            enc.emit_bl(0); // offset 0: bl #0 (unlinked)
            enc.emit_nop(); // offset 4
            enc.emit_nop(); // offset 8
            enc.emit_ret(); // offset 12: call target
        }
        assert_eq!(word(&code, 0), 0x9400_0000, "unlinked bl placeholder");

        let mut record = Arm64LinkRecord::new_call(0, 12);
        assert_eq!(
            record.compute_jump_type(),
            JumpLinkType::LinkJumpNoCondition
        );
        record.link(&mut code).expect("link bl");

        // imm26 = (12 - 0) >> 2 = 3  =>  word = 0x9400_0003.
        let patched = word(&code, 0);
        assert_eq!(patched, 0x9400_0003);
        assert_eq!((patched & 0x03ff_ffff) as i32, 3, "imm26 == 3 words");
        assert_eq!(patched >> 31, 1, "link bit set for bl");
    }

    // ------------------------------------------------------------------------
    // PROOF 3 — backward branches encode a two's-complement negative offset.
    //
    // Unconditional `b` from offset 8 back to offset 0: imm26 = -2.
    //   b .-8  = 0x1400_0000 | (-2 & 0x03ff_ffff) = 0x17ff_fffe.
    // Conditional `b.eq` from offset 4 back to offset 0: imm19 = -1.
    //   b.eq .-4 = 0x5400_0000 | ((-1 & 0x7ffff) << 5) = 0x54ff_ffe0.
    // ------------------------------------------------------------------------
    #[test]
    fn backward_branches_use_twos_complement_offsets() {
        // Unconditional b backward.
        let mut code = vec![0u8; 12];
        {
            let mut enc = Arm64Encoder::new(&mut code);
            enc.emit_ret(); // offset 0: target (label)
            enc.emit_nop(); // offset 4
            enc.emit_b(0); // offset 8: b #0 (unlinked)
        }
        let mut b = Arm64LinkRecord::new_jump(8, 0, JumpType::JumpNoCondition, Condition::Invalid);
        b.link(&mut code).expect("link backward b");
        let patched = word(&code, 2);
        assert_eq!(patched, 0x17ff_fffe, "b .-8");
        // Sign-extend imm26 and confirm it is -2 words.
        let imm26 = ((patched & 0x03ff_ffff) << 6) as i32 >> 6;
        assert_eq!(imm26, -2);

        // Conditional b.eq backward.
        let mut code2 = vec![0u8; 8];
        {
            let mut enc = Arm64Encoder::new(&mut code2);
            enc.emit_ret(); // offset 0: target
            enc.emit_b_cond(Condition::Eq, 0); // offset 4: b.eq #0
        }
        let mut bc = Arm64LinkRecord::new_jump(4, 0, JumpType::JumpCondition, Condition::Eq);
        assert_eq!(
            bc.compute_jump_type(),
            JumpLinkType::LinkJumpConditionDirect
        );
        bc.link(&mut code2).expect("link backward b.eq");
        let patched2 = word(&code2, 1);
        assert_eq!(patched2, 0x54ff_ffe0, "b.eq .-4");
        let imm19 = (((patched2 >> 5) & 0x0007_ffff) << 13) as i32 >> 13;
        assert_eq!(imm19, -1);
    }

    // ------------------------------------------------------------------------
    // PROOF 4 — out-of-range displacements are flagged, not silently truncated.
    //
    // b.cond direct fits isInt<19> word offset (ARM64Assembler.h:4294); just past
    // it forces the (deferred) indirect expansion, surfaced here as OutOfRange.
    // b/bl fit isInt<26> (ARM64Assembler.h:4244); just past it would need a jump
    // island.
    // ------------------------------------------------------------------------
    #[test]
    fn out_of_range_displacements_are_flagged() {
        // b.cond: max in-range word offset = 2^18 - 1 -> byte distance 4*(2^18-1).
        let in_range_to = 4 * ((1i64 << 18) - 1);
        let mut code = vec![0u8; (in_range_to as usize) + 8];
        // Just past the field: word offset 2^18 -> byte distance 2^20.
        let out_to = 4 * (1i64 << 18);
        // In-range succeeds.
        assert_eq!(
            link_conditional_branch_direct(&mut code, 0, in_range_to, Condition::Eq),
            Ok(())
        );
        // Out-of-range is flagged with the field width.
        assert_eq!(
            link_conditional_branch_direct(&mut code, 0, out_to, Condition::Eq),
            Err(Arm64LinkError::OutOfRange {
                offset_words: 1 << 18,
                field_bits: 19,
            })
        );

        // b/bl: word offset 2^25 is just out of the 26-bit signed field.
        let mut code2 = vec![0u8; 8];
        let far = 4 * (1i64 << 25);
        assert_eq!(
            link_jump_or_call(&mut code2, 0, far, false),
            Err(Arm64LinkError::OutOfRange {
                offset_words: 1 << 25,
                field_bits: 26,
            })
        );
    }

    // ------------------------------------------------------------------------
    // Guard rails: misalignment and out-of-bounds are values, never panics.
    // ------------------------------------------------------------------------
    #[test]
    fn misaligned_and_out_of_bounds_are_errors() {
        let mut code = vec![0u8; 8];
        assert_eq!(
            link_jump_or_call(&mut code, 2, 0, false),
            Err(Arm64LinkError::Misaligned { at: 2 })
        );
        // from == 8 leaves no room for a 4-byte word in an 8-byte buffer.
        assert_eq!(
            link_jump_or_call(&mut code, 8, 0, false),
            Err(Arm64LinkError::OutOfBounds { at: 8, len: 8 })
        );
    }

    #[test]
    fn far_conditional_selects_indirect_then_defers() {
        // A conditional jump whose distance exceeds isInt<21> bytes picks the
        // indirect form (ARM64Assembler.h:4091); the in-place core does not yet
        // emit the two-instruction expansion, so it is flagged.
        let to = 4 * (1i64 << 18); // word offset 2^18 -> out of imm19.
        let mut code = vec![0u8; (to as usize) + 8];
        let mut record = Arm64LinkRecord::new_jump(0, to, JumpType::JumpCondition, Condition::Eq);
        assert_eq!(record.compute_jump_type(), JumpLinkType::LinkJumpCondition);
        assert_eq!(
            record.link(&mut code),
            Err(Arm64LinkError::ExpansionUnsupported(
                JumpLinkType::LinkJumpCondition
            ))
        );
    }
}
