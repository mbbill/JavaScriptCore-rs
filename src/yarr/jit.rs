//! Yarr JIT planning descriptors.
//!
//! The real Yarr JIT will own assembler emission and entry thunks. This module
//! only records suitability, failure reasons, and generated-code metadata.

use crate::jit::{CallBoundaryId, JitCodeArtifact, JitCodeId, PatchpointDescriptor};
use crate::yarr::{BuiltInCharacterClassId, BytecodePatternId, CharacterRange};

/// Stable identity for a Yarr JIT plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct YarrJitPlanId(pub u64);

/// Yarr execution tier selected for a pattern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrJitTier {
    InterpreterOnly,
    OneShot,
    Jit,
    JitWithBoyerMoore,
}

/// Why a pattern cannot use the Yarr JIT.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrJitFailureReason {
    DecodeSurrogatePair,
    BackReference,
    Lookbehind,
    VariableCountedParenthesisWithNonZeroMinimum,
    ParenthesizedSubpattern,
    ParenthesisNestedTooDeep,
    ExecutableMemoryAllocationFailure,
    OffsetTooLarge,
    UnsupportedUnicodeSet,
    PolicyDisabled,
}

/// Boyer-Moore prefilter metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoyerMooreDescriptor {
    pub character_class: Option<BuiltInCharacterClassId>,
    pub ranges: Vec<CharacterRange>,
    pub map_size: u16,
    pub is_all_set: bool,
}

/// Plan for compiling Yarr bytecode to generated code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrJitPlan {
    pub id: YarrJitPlanId,
    pub pattern: BytecodePatternId,
    pub tier: YarrJitTier,
    pub boundary: Option<CallBoundaryId>,
    pub boyer_moore: Option<BoyerMooreDescriptor>,
    pub failure: Option<YarrJitFailureReason>,
}

/// Generated Yarr code descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrJitArtifact {
    pub plan: YarrJitPlanId,
    pub code: Option<JitCodeArtifact>,
    pub entry_code: Option<JitCodeId>,
    pub slow_path_boundary: Option<CallBoundaryId>,
    pub patchpoints: Vec<PatchpointDescriptor>,
}
