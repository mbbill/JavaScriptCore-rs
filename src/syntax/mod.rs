//! Syntax-front-end contracts for the Rust JavaScriptCore rewrite.
//!
//! This module owns source, lexing, parser, AST arena, and early semantic
//! boundaries. Parsing starts with a conservative recursive-descent subset and
//! keeps unsupported grammar as typed syntax errors instead of manufacturing a
//! fake tree.

pub(crate) mod arena;
pub(crate) mod ast;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod semantic;
pub(crate) mod source;
pub(crate) mod token;

pub use arena::{
    ArenaGeneration, AstRef, AstRefValidationFinding, AstRefValidationReport, IdentifierArena,
    IdentifierSource, NodeArenaKind, NodeId, ParserArena, ParserIdentifier, WellKnownIdentifier,
};
pub use ast::{
    AssignmentContext, AstPropertyKey, AstRoot, BinaryOperator as AstBinaryOperator,
    ClassElementKind, Expr, FunctionDecl, FunctionMetadata, FunctionSyntaxMode, LiteralKind,
    ModuleItem, NodeHeader, ObjectLiteralExpr, ObjectLiteralProperty, Pattern,
    PrivateBrandRequirement, ScopeNode, ScopeNodeKind, Stmt, SuperBinding, SyntaxKind,
};
pub use lexer::{
    KeywordPolicy, LexDeferred, LexGoal, LexRequest, LexResult, LexStrictness, Lexer, LexerError,
    LexerFlags, LexerSnapshot, LexerState, RawStringMode, RegExpLexContext, TemplateLexContext,
};
pub use parser::{
    AstBuilder, AstBuilderConfig, BuiltinMode, CollectingDiagnostics, DeclarationDefaultContext,
    DeclarationResultFlags, DerivedContextKind, DestructuringKind, ErrorRecovery, EvalContextKind,
    FunctionBodyKind, FunctionNameRequirement, FunctionParsePhase, ModuleGoal, ParseMode,
    ParsePhase, ParsedAst, ParsedSyntax, Parser, ParserConfig, ParserDiagnosticSink, ParserError,
    ParserErrorKind, ParserFeatureDescriptor, ParserFeatureSet, ParserGrammarContext,
    ParserImplementationVisibility, ParserModeDescriptor, ParserModeTable,
    ParserModeTableMutationAuthority, ParserModeTableOwner, ParserSavePoint, ParserSession,
    ScriptMode, SourceParseMode, SourceParseModeDescriptor, SourceParseModeSet, StrictModePolicy,
    SyntaxCheckResult, SyntaxChecker, SyntaxCheckerConfig, TreeBuilder,
};
pub use semantic::{
    analyze_root, analyze_scope_node, ClassElementError, ClassElementSemanticKind,
    ClassElementSemanticName, ClassElementSemanticRecord, CodeFeatures, Declaration,
    DeclarationKind, EarlyError, EarlyErrorKind, EarlySemanticInfo, EnvironmentSemanticFlags,
    EnvironmentSemanticKind, EnvironmentSemanticRecord, EvalParseSemanticMetadata,
    EvalSemanticContext, FunctionParseSemanticMetadata, ModuleAnalysis, ModuleExportBinding,
    ModuleImport, ModuleParseSemanticMetadata, ModuleRequest, ModuleScopeData, ParseSemanticGoal,
    ParseSemanticMetadata, PrivateNameDeclaration, PrivateNameError, PrivateNameKind,
    PrivateNameReference, PrivateNameReferenceKind, Scope, ScopeBoundaryFlag, ScopeKind,
    SemanticModel, SemanticScopeId, SemanticStrictness, SemanticValidationFinding,
    SemanticValidationReport, VariableEnvironment,
};
pub use source::{
    summarize_source_for_tooling, Diagnostic, DiagnosticKind, DiagnosticSeverity, DiagnosticSink,
    LineColumn, SourceBoundary, SourceCode, SourceEncoding, SourceOrigin, SourcePosition,
    SourceProvider, SourceProviderSourceType, SourceSpan, SourceTaint, SourceText,
    SourceToolingMapEntry, SourceToolingSummary, SourceValidationFinding, SourceValidationReport,
};
pub use token::{
    ContextualKeyword, IdentifierTokenKind, Keyword, KeywordDescriptor, KeywordStrictness,
    LexicalErrorKind, NumericLiteralKind, NumericRadix, Punctuator, PunctuatorDescriptor,
    StaticTokenTable, TemplateTokenKind, Token, TokenData, TokenFlags, TokenKind, TokenLocation,
    TokenTableMutationAuthority, TokenTableOwner,
};
