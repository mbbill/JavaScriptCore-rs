//! Syntax-front-end contracts for the Rust JavaScriptCore rewrite.
//!
//! This module is a design skeleton. It names the source, lexer, parser, AST,
//! arena, and early semantic boundaries without implementing JavaScript
//! parsing or semantic behavior.

pub(crate) mod arena;
pub(crate) mod ast;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod semantic;
pub(crate) mod source;
pub(crate) mod token;

pub use arena::{
    ArenaGeneration, AstRef, IdentifierArena, IdentifierSource, NodeArenaKind, NodeId, ParserArena,
    ParserIdentifier, WellKnownIdentifier,
};
pub use ast::{
    AssignmentContext, AstPropertyKey, AstRoot, BinaryOperator as AstBinaryOperator,
    ClassElementKind, Expr, FunctionMetadata, FunctionSyntaxMode, LiteralKind, ModuleItem,
    NodeHeader, Pattern, PrivateBrandRequirement, ScopeNode, ScopeNodeKind, Stmt, SuperBinding,
    SyntaxKind,
};
pub use lexer::{
    KeywordPolicy, LexDeferred, LexGoal, LexRequest, LexResult, LexStrictness, Lexer, LexerError,
    LexerFlags, LexerSnapshot, LexerState, RawStringMode, RegExpLexContext, TemplateLexContext,
};
pub use parser::{
    AstBuilderConfig, BuiltinMode, CollectingDiagnostics, ErrorRecovery, ModuleGoal, ParseMode,
    ParsePhase, Parser, ParserConfig, ParserDiagnosticSink, ParserError, ParserErrorKind,
    ParserFeatureSet, ParserSession, ScriptMode, SourceParseMode, StrictModePolicy,
    SyntaxCheckerConfig, TreeBuilder,
};
pub use semantic::{
    CodeFeatures, Declaration, DeclarationKind, EarlyError, EarlyErrorKind, EarlySemanticInfo,
    ModuleAnalysis, ModuleImport, ModuleRequest, PrivateNameDeclaration, PrivateNameKind, Scope,
    ScopeId, ScopeKind, SemanticModel, VariableEnvironment,
};
pub use source::{
    Diagnostic, DiagnosticKind, DiagnosticSeverity, DiagnosticSink, LineColumn, SourceBoundary,
    SourceCode, SourceEncoding, SourceOrigin, SourcePosition, SourceProvider,
    SourceProviderSourceType, SourceSpan, SourceTaint, SourceText,
};
pub use token::{
    ContextualKeyword, IdentifierTokenKind, Keyword, LexicalErrorKind, NumericLiteralKind,
    NumericRadix, Punctuator, TemplateTokenKind, Token, TokenData, TokenFlags, TokenKind,
    TokenLocation,
};
