use crate::syntax::arena::{ParserArena, ParserIdentifier};
use crate::syntax::lexer::{LexDeferred, LexerError};
use crate::syntax::semantic::EarlyError;
use crate::syntax::source::{Diagnostic, DiagnosticSink, SourceCode, SourceSpan};

/// Parser grammar and code-kind mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseMode {
    Program,
    Eval,
    FunctionBody,
    Module,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptMode {
    Classic,
    Module,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinMode {
    Normal,
    Builtin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceParseMode {
    NormalFunction,
    GeneratorBody,
    GeneratorWrapperFunction,
    GeneratorWrapperMethod,
    Getter,
    Setter,
    Method,
    ArrowFunction,
    AsyncFunctionBody,
    AsyncFunction,
    AsyncMethod,
    AsyncArrowFunction,
    AsyncArrowFunctionBody,
    Program,
    ModuleAnalyze,
    ModuleEvaluate,
    AsyncGeneratorBody,
    AsyncGeneratorWrapperFunction,
    AsyncGeneratorWrapperMethod,
    ClassFieldInitializer,
    ClassStaticBlock,
}

impl SourceParseMode {
    pub fn is_function(self) -> bool {
        !matches!(
            self,
            Self::Program | Self::ModuleAnalyze | Self::ModuleEvaluate
        )
    }

    pub fn is_module(self) -> bool {
        matches!(self, Self::ModuleAnalyze | Self::ModuleEvaluate)
    }
}

/// Shared tree-construction boundary for AST building and syntax checking.
pub trait TreeBuilder {
    type Output;

    fn finish(self) -> Self::Output;
}

/// AST-building front end. The concrete builder will allocate typed AST nodes
/// in `ParserArena` and preserve semantic metadata for bytecode generation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AstBuilderConfig {
    pub needs_free_variable_info: bool,
    pub can_use_function_cache: bool,
}

/// Syntax-only front end. It mirrors JSC's `SyntaxChecker`: no AST allocation,
/// but enough result categories to validate grammar, regexp syntax, and early
/// error surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyntaxCheckerConfig {
    pub validate_regexp_literals: bool,
}

/// Recursive-descent parser owner.
///
/// The parser owns token state, scopes, labels, declarations, and the selected
/// builder. Tokens and AST handles are tied to source and arena lifetimes by
/// future implementation details; this skeleton avoids exporting raw node
/// storage.
#[derive(Debug)]
pub struct Parser<'src, 'arena, B> {
    arena: &'arena mut ParserArena,
    builder: B,
    session: ParserSession<'src>,
    state: ParserState,
}

impl<'src, 'arena, B: TreeBuilder> Parser<'src, 'arena, B> {
    pub fn new(arena: &'arena mut ParserArena, builder: B, session: ParserSession<'src>) -> Self {
        Self {
            arena,
            builder,
            state: ParserState::new(session.config),
            session,
        }
    }

    pub fn with_mode(
        arena: &'arena mut ParserArena,
        builder: B,
        source: &'src SourceCode,
        mode: ParseMode,
    ) -> Self {
        Self::new(
            arena,
            builder,
            ParserSession::new(source, ParserConfig::for_mode(mode)),
        )
    }

    pub fn arena(&self) -> &ParserArena {
        self.arena
    }

    pub fn mode(&self) -> ParseMode {
        self.session.config.mode
    }

    pub fn session(&self) -> &ParserSession<'src> {
        &self.session
    }

    pub fn state(&self) -> &ParserState {
        &self.state
    }

    /// Parser entrypoint boundary.
    ///
    /// This deliberately does not parse JavaScript or construct an AST. It
    /// returns a typed deferred error so the scaffold can compile without
    /// installing an EOF-only parser path.
    pub fn parse(self) -> Result<B::Output, ParserError> {
        Err(ParserError::deferred(ParsePhase::Entry))
    }

    pub fn into_builder(self) -> B {
        self.builder
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParserSession<'src> {
    pub source: &'src SourceCode,
    pub config: ParserConfig,
}

impl<'src> ParserSession<'src> {
    pub fn new(source: &'src SourceCode, config: ParserConfig) -> Self {
        Self { source, config }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserConfig {
    pub mode: ParseMode,
    pub script_mode: ScriptMode,
    pub builtin_mode: BuiltinMode,
    pub source_parse_mode: SourceParseMode,
    pub strict: StrictModePolicy,
    pub module_goal: ModuleGoal,
    pub recovery: ErrorRecovery,
    pub features: ParserFeatureSet,
}

impl ParserConfig {
    pub fn for_mode(mode: ParseMode) -> Self {
        Self {
            mode,
            script_mode: if matches!(mode, ParseMode::Module) {
                ScriptMode::Module
            } else {
                ScriptMode::Classic
            },
            builtin_mode: BuiltinMode::Normal,
            source_parse_mode: match mode {
                ParseMode::Program | ParseMode::Eval => SourceParseMode::Program,
                ParseMode::FunctionBody => SourceParseMode::NormalFunction,
                ParseMode::Module => SourceParseMode::ModuleEvaluate,
            },
            strict: if matches!(mode, ParseMode::Module) {
                StrictModePolicy::AlwaysStrict
            } else {
                StrictModePolicy::DirectivePrologue
            },
            module_goal: ModuleGoal::Evaluate,
            recovery: ErrorRecovery::StopAtFirstError,
            features: ParserFeatureSet::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrictModePolicy {
    Sloppy,
    DirectivePrologue,
    AlwaysStrict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleGoal {
    Analyze,
    Evaluate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorRecovery {
    StopAtFirstError,
    CollectAndContinue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParserFeatureSet {
    pub allow_top_level_await: bool,
    pub allow_import_attributes: bool,
    pub allow_defer_imports: bool,
    pub allow_using_declarations: bool,
    pub preserve_function_cache_metadata: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParserState {
    pub config: ParserConfig,
    pub strict: bool,
    pub labels: Vec<LabelFrame>,
    pub scope_depth: u32,
    pub function_depth: u32,
    pub loop_depth: u32,
    pub switch_depth: u32,
    pub pending_early_errors: Vec<EarlyError>,
}

impl ParserState {
    fn new(config: ParserConfig) -> Self {
        Self {
            strict: matches!(config.strict, StrictModePolicy::AlwaysStrict),
            config,
            labels: Vec::new(),
            scope_depth: 0,
            function_depth: 0,
            loop_depth: 0,
            switch_depth: 0,
            pending_early_errors: Vec::new(),
        }
    }

    pub fn checkpoint(&self) -> ParserCheckpoint {
        ParserCheckpoint {
            strict: self.strict,
            scope_depth: self.scope_depth,
            function_depth: self.function_depth,
            loop_depth: self.loop_depth,
            switch_depth: self.switch_depth,
            label_count: self.labels.len().try_into().unwrap_or(u32::MAX),
            early_error_count: self
                .pending_early_errors
                .len()
                .try_into()
                .unwrap_or(u32::MAX),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserCheckpoint {
    pub strict: bool,
    pub scope_depth: u32,
    pub function_depth: u32,
    pub loop_depth: u32,
    pub switch_depth: u32,
    pub label_count: u32,
    pub early_error_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LabelFrame {
    pub name: ParserIdentifier,
    pub is_loop: bool,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParserError {
    pub span: Option<SourceSpan>,
    pub kind: ParserErrorKind,
}

impl ParserError {
    pub fn deferred(phase: ParsePhase) -> Self {
        Self {
            span: None,
            kind: ParserErrorKind::Deferred(phase),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParserErrorKind {
    Lexer(LexerError),
    LexDeferred(LexDeferred),
    Syntax(String),
    Early(EarlyError),
    Deferred(ParsePhase),
    RecoveryLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParsePhase {
    Entry,
    DirectivePrologue,
    StatementList,
    Expression,
    Pattern,
    Function,
    Class,
    ModuleItems,
    EarlyErrors,
}

pub trait ParserDiagnosticSink: DiagnosticSink {
    fn parser_error(&mut self, error: ParserError);
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollectingDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub parser_errors: Vec<ParserError>,
}

impl DiagnosticSink for CollectingDiagnostics {
    fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

impl ParserDiagnosticSink for CollectingDiagnostics {
    fn parser_error(&mut self, error: ParserError) {
        self.parser_errors.push(error);
    }
}
