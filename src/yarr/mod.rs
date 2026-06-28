//! Yarr regular-expression engine contracts.
//!
//! This module owns regexp parsing, bytecode/JIT compilation, match state,
//! Unicode handling, and runtime integration surfaces.

#![forbid(unsafe_code)]

pub(crate) mod bytecode;
pub(crate) mod execution;
pub(crate) mod jit;
pub(crate) mod matching;
pub(crate) mod parse;
pub(crate) mod simple_exec;
pub(crate) mod unicode;

pub use bytecode::{
    assemble_yarr_bytecode_plan, bytecode_term_schema, bytecode_term_schema_table,
    validate_byte_disjunction, validate_bytecode_pattern, validate_bytecode_term,
    validate_yarr_bytecode_program, ByteDisjunction, BytecodeAlternative, BytecodeAlternativeJump,
    BytecodeCharacterClassCache, BytecodeInputCheck, BytecodeOffsetVectorLayout, BytecodePattern,
    BytecodePatternBuilder, BytecodePatternId, BytecodeSchemaOwner, BytecodeSubpatternRange,
    BytecodeTerm, BytecodeTermBuilder, BytecodeTermId, BytecodeTermKind, BytecodeTermPayloadKind,
    BytecodeTermSchemaDescriptor, Quantifier, QuantifierKind, YarrBacktrackFrame,
    YarrBytecodeAssemblyError, YarrBytecodeProgram, YarrBytecodeProgramBuilder,
    YarrBytecodeValidationError,
};
pub use execution::{
    describe_regexp_program_invocation, describe_regexp_program_result, execute_regexp_bytecode,
    RegExpExecutionBoundaryError, RegExpProgramBody, RegExpProgramInvocationDescriptor,
    RegExpProgramInvocationRecord, RegExpProgramResultDescriptor, RegExpProgramResultRecord,
    RegExpRootBoundaryKind, RegExpRootBoundaryRecord,
};
pub use jit::{
    describe_yarr_jit_fallback, BoyerMooreBitmapDescriptor, BoyerMooreDescriptor,
    BoyerMooreFastCandidateSet, YarrDiagnosticKind, YarrFallbackDiagnosticRecord,
    YarrFallbackTarget, YarrJitArtifact, YarrJitFailureReason, YarrJitInlineStats, YarrJitPlan,
    YarrJitPlanId, YarrJitTier,
};
pub use matching::{
    describe_match_result_semantics, describe_match_state_semantics, interpret_bytecode,
    interpret_bytecode_with_limit, JSRegExpResult, MatchDirection, MatchFrom, MatchInput,
    MatchRange, MatchResult, MatchResultSemanticDescriptor, MatchSemanticError,
    MatchStackLimitSource, MatchState, MatchStateSemanticDescriptor, MatchStatus,
    MatchingContextHolderDescriptor, YarrInterpretOutcome, YarrMatchContext, YARR_MATCH_LIMIT,
    YARR_OFFSET_NO_MATCH,
};
pub use parse::{
    compile_mode_for_flags, describe_regex_flag_semantics, describe_yarr_parse_semantics,
    parse_regex_flags, plan_yarr_parse, validate_regex_flag_semantics,
    CharacterClassConstructionState, CharacterClassDescriptor, CharacterClassSetOperation,
    CharacterClassWidth, CompileMode, CreateDisjunctionPurpose, DotStarEnclosureAnchors,
    NamedCaptureGroupState, ParseEscapeMode, ParserTokenKind, PatternAlternative, PatternAssertion,
    PatternDisjunction, PatternParenthesesDescriptor, PatternTerm, PatternTermKind,
    RegExpParseSemanticDescriptor, RegexFlagKind, RegexFlagSemanticDescriptor,
    RegexFlagSemanticError, RegexFlags, RegexModifierFlagKind, UnicodeParseContext, YarrErrorCode,
    YarrParseError, YarrParsePlan, YarrParsePlanAtom, YarrParsePlanAtomKind, YarrPattern,
    YarrPatternId, YarrSyntaxDelegate,
};
pub use simple_exec::{
    execute_simple_yarr, YarrSimpleExecError, YarrSimpleMatch, YarrSimpleMatchRange,
};
pub use unicode::{
    built_in_character_class_descriptor, canonicalization_mode_for_flags,
    canonicalize_character_class_descriptor, character_class_contains,
    describe_character_class_semantics, describe_unicode_property_semantics,
    describe_yarr_canonicalization_semantics, unicode_property_class_descriptor,
    validate_owned_yarr_unicode_registry, validate_unicode_class_descriptor,
    validate_yarr_unicode_registry, yarr_unicode_registry, BuiltInCharacterClassDescriptor,
    BuiltInCharacterClassId, CanonicalizationRangeDescriptor, CanonicalizationRangeKind,
    CanonicalizationTableMode, CharacterClassSemanticDescriptor, CharacterRange,
    OwnedYarrUnicodeRegistry, UnicodeCanonicalizationMode, UnicodeClassDescriptor,
    UnicodeClassDescriptorBuilder, UnicodePropertyClassDescriptor, UnicodePropertyClassKind,
    UnicodePropertyLookup, UnicodePropertyName, UnicodePropertySemanticDescriptor,
    YarrCanonicalizationSemanticDescriptor, YarrUnicodeRegistry, YarrUnicodeRegistryAuthority,
    YarrUnicodeRegistryBuilder, YarrUnicodeSchemaOwner, YarrUnicodeValidationError,
    YARR_UNICODE_REGISTRY,
};
