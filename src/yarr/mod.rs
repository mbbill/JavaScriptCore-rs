//! Yarr regular-expression engine contracts.
//!
//! This module reserves regexp parsing, bytecode/JIT compilation, match state,
//! Unicode handling, and runtime integration surfaces without implementing a
//! regexp engine.

#![forbid(unsafe_code)]

pub(crate) mod bytecode;
pub(crate) mod jit;
pub(crate) mod matching;
pub(crate) mod parse;
pub(crate) mod unicode;

pub use bytecode::{
    BytecodeAlternative, BytecodePattern, BytecodePatternId, BytecodeTerm, BytecodeTermId,
    BytecodeTermKind, Quantifier, QuantifierKind, YarrBacktrackFrame, YarrBytecodeProgram,
};
pub use jit::{
    BoyerMooreDescriptor, YarrJitArtifact, YarrJitFailureReason, YarrJitPlan, YarrJitPlanId,
    YarrJitTier,
};
pub use matching::{
    MatchDirection, MatchFrom, MatchInput, MatchRange, MatchResult, MatchState, MatchStatus,
    YarrMatchContext,
};
pub use parse::{
    CharacterClassDescriptor, CharacterClassSetOperation, CharacterClassWidth, CompileMode,
    PatternAlternative, PatternAssertion, PatternDisjunction, PatternTerm, PatternTermKind,
    RegexFlags, YarrErrorCode, YarrPattern, YarrPatternId,
};
pub use unicode::{
    BuiltInCharacterClassId, CharacterRange, UnicodeCanonicalizationMode, UnicodeClassDescriptor,
    UnicodePropertyLookup, UnicodePropertyName,
};
