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

/// Small exact-character candidate set used before consulting the bitmap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoyerMooreFastCandidateSet {
    pub characters: Vec<char>,
    pub is_valid: bool,
    pub maximum_size: u8,
}

/// Cached Boyer-Moore bitmap owned by generated code for possible reuse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoyerMooreBitmapDescriptor {
    pub map_size: u16,
    pub set_bits: u16,
    pub fast_candidates: BoyerMooreFastCandidateSet,
    pub can_be_reused: bool,
}

/// Inlineability metadata published by a Yarr code block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrJitInlineStats {
    pub instruction_count: u32,
    pub stack_size: u32,
    pub needs_temp_register_2: bool,
    pub can_inline: bool,
}

/// Plan for compiling Yarr bytecode to generated code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrJitPlan {
    pub id: YarrJitPlanId,
    pub pattern: BytecodePatternId,
    pub tier: YarrJitTier,
    pub boundary: Option<CallBoundaryId>,
    pub boyer_moore: Option<BoyerMooreDescriptor>,
    pub reusable_boyer_moore_maps: Vec<BoyerMooreBitmapDescriptor>,
    pub failure: Option<YarrJitFailureReason>,
}

/// Generated Yarr code descriptor.
/// Generated code owns executable memory and Boyer-Moore map storage; regexp
/// runtime may replace the artifact but must not mutate published entrypoints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrJitArtifact {
    pub plan: YarrJitPlanId,
    pub code: Option<JitCodeArtifact>,
    pub entry_code: Option<JitCodeId>,
    pub slow_path_boundary: Option<CallBoundaryId>,
    pub patchpoints: Vec<PatchpointDescriptor>,
    pub inline_stats: Option<YarrJitInlineStats>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrFallbackTarget {
    RunJit,
    BytecodeInterpreter,
    ParsedPatternInterpreter,
    RejectPattern,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrDiagnosticKind {
    JitDisabledByPolicy,
    UnsupportedPattern,
    ExecutableMemoryUnavailable,
    InterpreterOnlyTier,
    MissingGeneratedCode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrFallbackDiagnosticRecord {
    pub plan: YarrJitPlanId,
    pub pattern: BytecodePatternId,
    pub requested_tier: YarrJitTier,
    pub target: YarrFallbackTarget,
    pub kind: YarrDiagnosticKind,
    pub failure: Option<YarrJitFailureReason>,
    pub can_retry_with_policy_change: bool,
}

pub fn describe_yarr_jit_fallback(plan: &YarrJitPlan) -> YarrFallbackDiagnosticRecord {
    let (target, kind, can_retry_with_policy_change) = match plan.failure {
        None if plan.tier == YarrJitTier::InterpreterOnly => (
            YarrFallbackTarget::BytecodeInterpreter,
            YarrDiagnosticKind::InterpreterOnlyTier,
            false,
        ),
        None => (
            YarrFallbackTarget::RunJit,
            YarrDiagnosticKind::MissingGeneratedCode,
            false,
        ),
        Some(YarrJitFailureReason::PolicyDisabled) => (
            YarrFallbackTarget::BytecodeInterpreter,
            YarrDiagnosticKind::JitDisabledByPolicy,
            true,
        ),
        Some(YarrJitFailureReason::ExecutableMemoryAllocationFailure) => (
            YarrFallbackTarget::BytecodeInterpreter,
            YarrDiagnosticKind::ExecutableMemoryUnavailable,
            false,
        ),
        Some(YarrJitFailureReason::DecodeSurrogatePair)
        | Some(YarrJitFailureReason::BackReference)
        | Some(YarrJitFailureReason::Lookbehind)
        | Some(YarrJitFailureReason::VariableCountedParenthesisWithNonZeroMinimum)
        | Some(YarrJitFailureReason::ParenthesizedSubpattern)
        | Some(YarrJitFailureReason::ParenthesisNestedTooDeep)
        | Some(YarrJitFailureReason::OffsetTooLarge)
        | Some(YarrJitFailureReason::UnsupportedUnicodeSet) => (
            YarrFallbackTarget::BytecodeInterpreter,
            YarrDiagnosticKind::UnsupportedPattern,
            false,
        ),
    };

    YarrFallbackDiagnosticRecord {
        plan: plan.id,
        pattern: plan.pattern,
        requested_tier: plan.tier,
        target,
        kind,
        failure: plan.failure,
        can_retry_with_policy_change,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yarr_jit_fallback_reports_policy_retry() {
        let plan = YarrJitPlan {
            id: YarrJitPlanId(1),
            pattern: BytecodePatternId(2),
            tier: YarrJitTier::Jit,
            boundary: None,
            boyer_moore: None,
            reusable_boyer_moore_maps: Vec::new(),
            failure: Some(YarrJitFailureReason::PolicyDisabled),
        };

        let record = describe_yarr_jit_fallback(&plan);

        assert_eq!(record.target, YarrFallbackTarget::BytecodeInterpreter);
        assert_eq!(record.kind, YarrDiagnosticKind::JitDisabledByPolicy);
        assert!(record.can_retry_with_policy_change);
    }

    #[test]
    fn yarr_jit_fallback_reports_unsupported_pattern() {
        let plan = YarrJitPlan {
            id: YarrJitPlanId(3),
            pattern: BytecodePatternId(4),
            tier: YarrJitTier::JitWithBoyerMoore,
            boundary: None,
            boyer_moore: None,
            reusable_boyer_moore_maps: Vec::new(),
            failure: Some(YarrJitFailureReason::Lookbehind),
        };

        let record = describe_yarr_jit_fallback(&plan);

        assert_eq!(record.target, YarrFallbackTarget::BytecodeInterpreter);
        assert_eq!(record.kind, YarrDiagnosticKind::UnsupportedPattern);
        assert!(!record.can_retry_with_policy_change);
    }
}
