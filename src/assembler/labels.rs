//! AbstractMacroAssembler label / jump / call offset-token model.
//!
//! Faithful port of the token classes in
//! `Source/JavaScriptCore/assembler/AbstractMacroAssembler.h:434-820`: `Label`
//! (436-464), `Call` (569-630), `Jump` (632-771), `PatchableJump` (773-792),
//! and `JumpList` (794-820). These are plain `Copy` offset tokens captured while
//! the assembler emits code; each wraps an [`AssemblerLabel`] (a code-buffer
//! byte offset, AssemblerBuffer.h:67-109) and holds NO executable address. A
//! `Jump`/`Call` is resolved only at link time, when it is turned into an
//! [`Arm64LinkRecord`] and the relative displacement is patched into the
//! instruction word (see [`super::link_records`]).
//!
//! On ARM64 a `Jump` additionally carries the `JumpType`/`Condition`/
//! compare-register metadata the link pass needs (AbstractMacroAssembler.h:
//! 654-770) — JSC stores this on the token rather than in the instruction stream
//! (see the `Fixme` at AbstractMacroAssembler.h:647). This port mirrors the
//! ARM64 layout only; the `CPU(ARM_THUMB2)` / generic layouts are not ported.
//!
//! Pure-safe value types: no allocation beyond `JumpList`'s vector, no
//! executable memory. UNWIRED dead code until the baseline JIT emits through it.
#![allow(dead_code)]

use super::arm64_encoder::Condition;
use super::link_records::{Arm64LinkRecord, JumpType};
use super::registers::RegisterID;
use super::AssemblerLabel;

/// `AbstractMacroAssembler::Label` (AbstractMacroAssembler.h:438-464): a point
/// in the instruction stream usable as a jump destination. Wraps the captured
/// [`AssemblerLabel`] buffer offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Label {
    label: AssemblerLabel,
}

impl Default for Label {
    /// `Label() = default` (AbstractMacroAssembler.h:447) default-constructs an
    /// `AssemblerLabel()`, which is the *unset* sentinel (AssemblerBuffer.h:68).
    /// This must override `AssemblerLabel`'s `#[derive(Default)]` of `0` (a valid
    /// offset that the descriptor/digest layer relies on) — see the divergence
    /// note on [`AssemblerLabel`].
    #[inline]
    fn default() -> Self {
        Self {
            label: AssemblerLabel::UNSET,
        }
    }
}

impl Label {
    /// Construct from a captured assembler label (mirrors the `masm` ctor at
    /// AbstractMacroAssembler.h:453, which stores `m_assembler.label()`).
    #[inline]
    pub fn new(label: AssemblerLabel) -> Self {
        Self { label }
    }

    /// `isSet()` (AbstractMacroAssembler.h:461).
    #[inline]
    pub fn is_set(&self) -> bool {
        self.label.is_set()
    }

    /// The underlying buffer offset token.
    #[inline]
    pub fn label(&self) -> AssemblerLabel {
        self.label
    }
}

/// `AbstractMacroAssembler::Call::Flags` (AbstractMacroAssembler.h:579-586): the
/// bit set describing how a call site may be linked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallFlags(u8);

impl CallFlags {
    pub const NONE: CallFlags = CallFlags(0x0);
    pub const LINKABLE: CallFlags = CallFlags(0x1);
    pub const NEAR: CallFlags = CallFlags(0x2);
    pub const TAIL: CallFlags = CallFlags(0x4);
    pub const LINKABLE_NEAR: CallFlags = CallFlags(0x1 | 0x2);
    pub const LINKABLE_NEAR_TAIL: CallFlags = CallFlags(0x1 | 0x2 | 0x4);

    /// `isFlagSet(flag)` (AbstractMacroAssembler.h:599-602).
    #[inline]
    pub const fn is_set(self, flag: CallFlags) -> bool {
        self.0 & flag.0 != 0
    }
}

/// `AbstractMacroAssembler::Call` (AbstractMacroAssembler.h:575-630): a planted
/// call instruction to be linked to its destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Call {
    label: AssemblerLabel,
    flags: CallFlags,
}

impl Call {
    /// `Call(jmp, flags)` (AbstractMacroAssembler.h:593-597).
    #[inline]
    pub fn new(label: AssemblerLabel, flags: CallFlags) -> Self {
        Self { label, flags }
    }

    /// `fromTailJump(jump)` (AbstractMacroAssembler.h:604-607).
    #[inline]
    pub fn from_tail_jump(jump: Jump) -> Self {
        Self {
            label: jump.label,
            flags: CallFlags::LINKABLE,
        }
    }

    #[inline]
    pub fn is_flag_set(&self, flag: CallFlags) -> bool {
        self.flags.is_set(flag)
    }

    #[inline]
    pub fn label(&self) -> AssemblerLabel {
        self.label
    }

    /// Build the link record for a near call to `target` (byte offset). Mirrors
    /// JSC's `linkNearCall` path: a `bl` whose displacement is resolved at link
    /// time (ARM64Assembler.h:3854-3858). `from` is this call's byte offset.
    #[inline]
    pub fn to_link_record(&self, target: AssemblerLabel) -> Arm64LinkRecord {
        Arm64LinkRecord::new_call(self.label.offset() as i64, target.offset() as i64)
    }
}

/// `AbstractMacroAssembler::Jump` (AbstractMacroAssembler.h:632-771), ARM64
/// layout (654-680, 764-770).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Jump {
    label: AssemblerLabel,
    type_: JumpType,
    condition: Condition,
    is_64bit: bool,
    bit_number: u32,
    compare_register: Option<RegisterID>,
}

impl Jump {
    /// `Jump(jmp, type, condition)` (AbstractMacroAssembler.h:655-660): the
    /// common direct/conditional branch. `compare_register`/`bit_number` are the
    /// compare-and-branch / test-bit extras (defaulted off here).
    #[inline]
    pub fn new(label: AssemblerLabel, type_: JumpType, condition: Condition) -> Self {
        Self {
            label,
            type_,
            condition,
            is_64bit: false,
            bit_number: 0,
            compare_register: None,
        }
    }

    /// `label()` (AbstractMacroAssembler.h:688-693): the destination view of this
    /// jump's own site.
    #[inline]
    pub fn label(&self) -> Label {
        Label { label: self.label }
    }

    #[inline]
    pub fn jump_type(&self) -> JumpType {
        self.type_
    }

    #[inline]
    pub fn condition(&self) -> Condition {
        self.condition
    }

    /// `isSet()` (AbstractMacroAssembler.h:757).
    #[inline]
    pub fn is_set(&self) -> bool {
        self.label.is_set()
    }

    /// `Jump::linkTo(label, masm)` (AbstractMacroAssembler.h:718-737): resolve
    /// this jump to a known destination by producing the [`Arm64LinkRecord`] the
    /// assembler would append (ARM64Assembler.h:3799-3804 `linkJump`). The
    /// compare-and-branch / test-bit ctors (AbstractMacroAssembler.h:662-680)
    /// are not modelled here; this covers the `b`/`b.cond` forms.
    #[inline]
    pub fn to_link_record(&self, target: Label) -> Arm64LinkRecord {
        debug_assert!(
            self.compare_register.is_none(),
            "compare-and-branch / test-bit link records are deferred"
        );
        Arm64LinkRecord::new_jump(
            self.label.offset() as i64,
            target.label.offset() as i64,
            self.type_,
            self.condition,
        )
    }
}

/// `AbstractMacroAssembler::PatchableJump` (AbstractMacroAssembler.h:773-792): a
/// jump whose site may later be re-patched. Wraps a [`Jump`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchableJump {
    jump: Jump,
}

impl PatchableJump {
    /// `explicit PatchableJump(Jump)` (AbstractMacroAssembler.h:778-781).
    #[inline]
    pub fn new(jump: Jump) -> Self {
        Self { jump }
    }

    /// `operator Jump&()` (AbstractMacroAssembler.h:783).
    #[inline]
    pub fn jump(&self) -> Jump {
        self.jump
    }
}

/// `AbstractMacroAssembler::JumpList` (AbstractMacroAssembler.h:794-820): a set
/// of jumps all linked to the same destination. The `Vector<Jump, 2>` inline
/// capacity is a C++ allocation optimization with no observable Rust analogue;
/// a plain `Vec` is the faithful behavior.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JumpList {
    jumps: Vec<Jump>,
}

impl JumpList {
    #[inline]
    pub fn new() -> Self {
        Self { jumps: Vec::new() }
    }

    /// `JumpList(Jump)` (AbstractMacroAssembler.h:804-808): append only if set.
    #[inline]
    pub fn from_jump(jump: Jump) -> Self {
        let mut list = Self::new();
        if jump.is_set() {
            list.append(jump);
        }
        list
    }

    /// `append(Jump)`.
    #[inline]
    pub fn append(&mut self, jump: Jump) {
        self.jumps.push(jump);
    }

    /// `append(JumpList)`: absorb another list's jumps.
    #[inline]
    pub fn append_list(&mut self, other: &JumpList) {
        self.jumps.extend_from_slice(&other.jumps);
    }

    #[inline]
    pub fn jumps(&self) -> &[Jump] {
        &self.jumps
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.jumps.is_empty()
    }

    /// Resolve every jump in the list to the same `target`, producing one link
    /// record per jump (mirrors `JumpList::link`, which links each contained
    /// `Jump` to the common destination).
    #[inline]
    pub fn to_link_records(&self, target: Label) -> Vec<Arm64LinkRecord> {
        self.jumps
            .iter()
            .map(|jump| jump.to_link_record(target))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::link_records::{BranchType, JumpLinkType};
    use super::*;

    #[test]
    fn label_tracks_set_state() {
        assert!(!Label::default().is_set());
        let label = Label::new(AssemblerLabel(8));
        assert!(label.is_set());
        assert_eq!(label.label(), AssemblerLabel(8));
        // The unset sentinel mirrors AssemblerLabel()'s u32::MAX default.
        assert!(!Label::new(AssemblerLabel::UNSET).is_set());
    }

    #[test]
    fn call_flags_match_jsc_bit_layout() {
        assert!(CallFlags::LINKABLE_NEAR.is_set(CallFlags::LINKABLE));
        assert!(CallFlags::LINKABLE_NEAR.is_set(CallFlags::NEAR));
        assert!(!CallFlags::LINKABLE_NEAR.is_set(CallFlags::TAIL));
        assert!(CallFlags::LINKABLE_NEAR_TAIL.is_set(CallFlags::TAIL));
        assert!(!CallFlags::NONE.is_set(CallFlags::LINKABLE));
    }

    #[test]
    fn jump_lowers_to_conditional_link_record() {
        // A forward b.eq token at offset 0 to a label at offset 8 lowers to the
        // LinkRecord the assembler would append, and resolves to the direct form.
        let jump = Jump::new(AssemblerLabel(0), JumpType::JumpCondition, Condition::Eq);
        let mut record = jump.to_link_record(Label::new(AssemblerLabel(8)));
        assert_eq!(record.from(), 0);
        assert_eq!(record.to(), 8);
        assert_eq!(record.jump_type(), JumpType::JumpCondition);
        assert_eq!(record.condition(), Condition::Eq);
        assert_eq!(record.branch_type(), BranchType::Jmp);
        assert_eq!(
            record.compute_jump_type(),
            JumpLinkType::LinkJumpConditionDirect
        );
    }

    #[test]
    fn call_lowers_to_call_link_record() {
        let call = Call::new(AssemblerLabel(4), CallFlags::LINKABLE_NEAR);
        let record = call.to_link_record(AssemblerLabel(16));
        assert_eq!(record.from(), 4);
        assert_eq!(record.to(), 16);
        assert_eq!(record.branch_type(), BranchType::Call);
    }

    #[test]
    fn jump_list_collects_and_resolves() {
        let mut list = JumpList::from_jump(Jump::new(
            AssemblerLabel(0),
            JumpType::JumpNoCondition,
            Condition::Invalid,
        ));
        list.append(Jump::new(
            AssemblerLabel(4),
            JumpType::JumpCondition,
            Condition::Ne,
        ));
        // An unset jump is dropped by from_jump, matching JSC.
        assert!(JumpList::from_jump(Jump::new(
            AssemblerLabel::UNSET,
            JumpType::JumpNoCondition,
            Condition::Invalid,
        ))
        .is_empty());

        let records = list.to_link_records(Label::new(AssemblerLabel(64)));
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.to() == 64));
        assert_eq!(records[0].from(), 0);
        assert_eq!(records[1].from(), 4);
    }
}
