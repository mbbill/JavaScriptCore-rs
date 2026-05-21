use crate::syntax::arena::{
    AstRef, IdentifierSource, ParserArena, ParserIdentifier, WellKnownIdentifier,
};
use crate::syntax::ast::{
    ArrayLiteralElement, ArrayLiteralExpr, AssignmentContext, AssignmentExpr, AssignmentOperator,
    AstPropertyKey, AstRoot, BinaryExpr, BinaryOperator as AstBinaryOperator, CallExpr,
    ClassElement, ClassElementKind, ClassElementName, ClassExpr, ConditionalExpr, ControlKind,
    ControlStmt, DeclarationStmt, DeclarationSyntaxKind, DoWhileStmt, Expr, ForInit, ForOfBinding,
    ForOfStmt, ForStmt, FunctionDecl, FunctionMetadata, FunctionParameter, FunctionSyntaxMode,
    IfStmt, LiteralExpr, LiteralKind, MemberExpr, MemberKind, ModuleItem, NameExpr, NameKind,
    NewExpr, NumberLiteralValue, ObjectLiteralExpr, ObjectLiteralProperty,
    ObjectLiteralPropertyKind, Pattern, ScopeBlock, ScopeNode, ScopeNodeKind, Stmt, SwitchCase,
    SwitchStmt, TemplateExpr, TemplatePart, TryStmt, UnaryExpr, UnaryOperator, WhileStmt,
};
use crate::syntax::lexer::{
    KeywordPolicy, LexDeferred, LexGoal, LexRequest, LexResult, Lexer, LexerError, RawStringMode,
    TemplateLexContext,
};
use crate::syntax::semantic::{EarlyError, EarlySemanticInfo, ModuleAnalysis, SemanticScopeId};
use crate::syntax::source::{Diagnostic, DiagnosticSink, SourceCode, SourceSpan};
use crate::syntax::token::{
    ContextualKeyword, Keyword, NumericLiteralKind, Punctuator, TemplateTokenKind, Token,
    TokenData, TokenKind,
};

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

type DeclarationBindingList = (Vec<AstRef<Pattern>>, Vec<Option<AstRef<Expr>>>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedFunctionMetadata {
    name: Option<ParserIdentifier>,
    metadata: AstRef<FunctionMetadata>,
    close_span: SourceSpan,
}

/// Host-facing implementation visibility carried while parsing builtins.
///
/// JSC threads `ImplementationVisibility` through parser scopes so builtin
/// syntax can name private implementation details without making them part of
/// ordinary source semantics. Mutation belongs to parser scope management.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParserImplementationVisibility {
    #[default]
    Public,
    Private,
    Hidden,
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

/// Compact SourceParseMode mask mirroring JSC's `SourceParseModeSet`.
///
/// This is configuration data only. Parser mode classification should stay in
/// this syntax layer; bytecode and executable modules receive resolved modes in
/// their own contracts rather than recomputing parser policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceParseModeSet {
    pub bits: u32,
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

/// Derived-class context inherited by nested function parsing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DerivedContextKind {
    #[default]
    None,
    DerivedConstructor,
    DerivedMethod,
}

/// Eval source context preserved for source cache keys and executable flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EvalContextKind {
    #[default]
    None,
    FunctionEval,
    InstanceFieldEval,
}

/// Shared tree-construction boundary for AST building and syntax checking.
pub trait TreeBuilder {
    type Output;

    fn finish(self, parsed: ParsedSyntax) -> Self::Output;
}

/// Parser result shared by AST-building and syntax-only front ends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedSyntax {
    pub root: AstRoot,
    pub mode: ParseMode,
    pub span: SourceSpan,
    pub token_count: u32,
}

/// AST-building front end. The concrete builder will allocate typed AST nodes
/// in `ParserArena` and preserve semantic metadata for bytecode generation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AstBuilderConfig {
    pub needs_free_variable_info: bool,
    pub can_use_function_cache: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AstBuilder {
    pub config: AstBuilderConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedAst {
    pub root: AstRoot,
    pub mode: ParseMode,
    pub span: SourceSpan,
    pub token_count: u32,
    pub needs_free_variable_info: bool,
}

impl TreeBuilder for AstBuilder {
    type Output = ParsedAst;

    fn finish(self, parsed: ParsedSyntax) -> Self::Output {
        ParsedAst {
            root: parsed.root,
            mode: parsed.mode,
            span: parsed.span,
            token_count: parsed.token_count,
            needs_free_variable_info: self.config.needs_free_variable_info,
        }
    }
}

/// Syntax-only front end. It mirrors JSC's `SyntaxChecker`: no AST allocation,
/// but enough result categories to validate grammar, regexp syntax, and early
/// error surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyntaxCheckerConfig {
    pub validate_regexp_literals: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyntaxChecker {
    pub config: SyntaxCheckerConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxCheckResult {
    pub mode: ParseMode,
    pub span: SourceSpan,
    pub token_count: u32,
    pub root_kind: ScopeNodeKind,
}

impl TreeBuilder for SyntaxChecker {
    type Output = SyntaxCheckResult;

    fn finish(self, parsed: ParsedSyntax) -> Self::Output {
        let root_kind = match parsed.root {
            AstRoot::Script(_) => ScopeNodeKind::Script,
            AstRoot::Module(_) => ScopeNodeKind::Module,
            AstRoot::Function(_) => ScopeNodeKind::Function,
        };
        SyntaxCheckResult {
            mode: parsed.mode,
            span: parsed.span,
            token_count: parsed.token_count,
            root_kind,
        }
    }
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

    /// Parse source into an arena-owned syntax product.
    ///
    /// This is intentionally a conservative recursive-descent entrypoint. It
    /// builds real AST nodes for statement lists, declarations, control
    /// statements, calls, member expressions, assignment, and binary
    /// expressions, while unsupported grammar still reports typed syntax
    /// errors instead of silently manufacturing a fake tree.
    pub fn parse(mut self) -> Result<B::Output, ParserError> {
        let tokens = self.lex_all_tokens()?;
        let token_count = tokens
            .iter()
            .filter(|token| token.kind != TokenKind::EndOfFile)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        let mut cursor = TokenCursor::new(&tokens);
        let statements = self.parse_statement_list(&mut cursor, None)?;
        cursor.expect_eof()?;
        let root = self.finish_root(statements);
        Ok(self.builder.finish(ParsedSyntax {
            root,
            mode: self.session.config.mode,
            span: self.session.source.range(),
            token_count,
        }))
    }

    pub fn into_builder(self) -> B {
        self.builder
    }

    fn lex_all_tokens(&mut self) -> Result<Vec<Token>, ParserError> {
        let mut lexer = Lexer::<()>::new(self.session.source, self.arena.identifiers_mut());
        let request = LexRequest {
            goal: LexGoal::Div,
            strict: if self.state.strict {
                crate::syntax::lexer::LexStrictness::Strict
            } else {
                crate::syntax::lexer::LexStrictness::Sloppy
            },
            keyword_policy: KeywordPolicy::Classify,
            allow_html_comment_tokens: self.session.config.features.allow_html_comments,
        };
        let mut tokens = Vec::new();
        let mut previous = None;
        let mut template_stack = Vec::<TemplateLexingContext>::new();
        loop {
            let request = LexRequest {
                goal: if regexp_literal_allowed_after(previous) {
                    LexGoal::RegExp
                } else {
                    LexGoal::Div
                },
                ..request
            };
            match lexer.next_token(request) {
                LexResult::Ready(token) => {
                    let is_eof = token.kind == TokenKind::EndOfFile;
                    let resume_template =
                        update_template_lexing_context(&mut template_stack, token.kind);
                    if !is_eof {
                        previous = previous_kind_after_lexed_token(token.kind);
                    }
                    tokens.push(token);
                    if is_eof {
                        return Ok(tokens);
                    }
                    if resume_template {
                        let token = match lexer.template_literal(TemplateLexContext {
                            raw_strings: RawStringMode::Build,
                            expression_depth: 1,
                        }) {
                            LexResult::Ready(token) => token,
                            LexResult::Error(error) => {
                                return Err(ParserError {
                                    span: Some(error.span),
                                    kind: ParserErrorKind::Lexer(error),
                                });
                            }
                            LexResult::Deferred(deferred) => {
                                return Err(ParserError {
                                    span: Some(crate::syntax::source::SourceSpan::at(
                                        deferred.cursor,
                                    )),
                                    kind: ParserErrorKind::LexDeferred(deferred),
                                });
                            }
                        };
                        update_template_lexing_context(&mut template_stack, token.kind);
                        previous = previous_kind_after_lexed_token(token.kind);
                        tokens.push(token);
                    }
                }
                LexResult::Error(error) => {
                    return Err(ParserError {
                        span: Some(error.span),
                        kind: ParserErrorKind::Lexer(error),
                    });
                }
                LexResult::Deferred(deferred) => {
                    return Err(ParserError {
                        span: Some(crate::syntax::source::SourceSpan::at(deferred.cursor)),
                        kind: ParserErrorKind::LexDeferred(deferred),
                    });
                }
            }
        }
    }

    fn parse_statement_list(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        terminator: Option<Punctuator>,
    ) -> Result<Vec<AstRef<Stmt>>, ParserError> {
        let mut statements = Vec::new();
        while !cursor.is_eof() {
            if let Some(punctuator) = terminator {
                if cursor.at_punctuator(punctuator) {
                    break;
                }
            }
            statements.push(self.parse_statement(cursor)?);
        }
        Ok(statements)
    }

    fn parse_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let token = cursor.current().clone();
        match token.kind {
            TokenKind::Punctuator(Punctuator::Semicolon) => {
                cursor.bump();
                Ok(self.arena.alloc_statement(Stmt::Empty(token.span())))
            }
            TokenKind::Punctuator(Punctuator::OpenBrace) => self.parse_block_statement(cursor),
            TokenKind::Keyword(Keyword::Var) => {
                self.parse_declaration_statement(cursor, DeclarationSyntaxKind::Var)
            }
            TokenKind::Keyword(Keyword::Const) => {
                self.parse_declaration_statement(cursor, DeclarationSyntaxKind::Const)
            }
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Let)) => {
                self.parse_declaration_statement(cursor, DeclarationSyntaxKind::Let)
            }
            TokenKind::Keyword(Keyword::Function) => self.parse_function_declaration(cursor),
            TokenKind::Keyword(Keyword::Class) => self.parse_class_declaration(cursor),
            TokenKind::Keyword(Keyword::Return) => self.parse_return_statement(cursor),
            TokenKind::Keyword(Keyword::If) => self.parse_if_statement(cursor),
            TokenKind::Keyword(Keyword::Do) => self.parse_do_while_statement(cursor),
            TokenKind::Keyword(Keyword::While) => self.parse_while_statement(cursor),
            TokenKind::Keyword(Keyword::Switch) => self.parse_switch_statement(cursor),
            TokenKind::Keyword(Keyword::For) => self.parse_for_statement(cursor),
            TokenKind::Keyword(Keyword::Try) => self.parse_try_statement(cursor),
            TokenKind::Keyword(Keyword::Throw) => self.parse_throw_statement(cursor),
            TokenKind::Keyword(Keyword::Break) => {
                self.parse_jump_statement(cursor, JumpStatementKind::Break)
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.parse_jump_statement(cursor, JumpStatementKind::Continue)
            }
            TokenKind::EndOfFile => syntax_error(&token, "unexpected end of input in statement"),
            _ => {
                let expr = self.parse_expression(cursor)?;
                self.consume_statement_terminator(cursor)?;
                Ok(self.arena.alloc_statement(Stmt::Expression(expr)))
            }
        }
    }

    fn parse_block_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let open = cursor.expect_punctuator(Punctuator::OpenBrace)?;
        let statements = self.parse_statement_list(cursor, Some(Punctuator::CloseBrace))?;
        let close = cursor.expect_punctuator(Punctuator::CloseBrace)?;
        let span = join_spans(open.span(), close.span());
        let scope = ScopeNode {
            id: SemanticScopeId(self.state.scope_depth.saturating_add(1)),
            kind: ScopeNodeKind::Eval,
            span,
            statements,
            semantics: EarlySemanticInfo {
                strict: self.state.strict,
                ..EarlySemanticInfo::default()
            },
            module: None,
        };
        let scope = self.arena.alloc_scope_node(scope);
        Ok(self
            .arena
            .alloc_statement(Stmt::Block(ScopeBlock { span, scope })))
    }

    fn parse_declaration_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        kind: DeclarationSyntaxKind,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.bump().span();
        let (bindings, initializers) = self.parse_declaration_bindings(cursor)?;
        let end = self.consume_statement_terminator(cursor)?.unwrap_or(start);
        let span = join_spans(start, end);
        Ok(self
            .arena
            .alloc_statement(Stmt::Declaration(DeclarationStmt {
                span,
                kind,
                bindings,
                initializers,
            })))
    }

    fn parse_declaration_bindings(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<DeclarationBindingList, ParserError> {
        let mut bindings = Vec::new();
        let mut initializers = Vec::new();
        loop {
            let binding = self.parse_binding_pattern(cursor)?;
            bindings.push(binding);
            let initializer = if cursor.consume_punctuator(Punctuator::Equal).is_some() {
                Some(self.parse_expression(cursor)?)
            } else {
                None
            };
            initializers.push(initializer);
            if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                break;
            }
        }
        Ok((bindings, initializers))
    }

    fn parse_binding_pattern(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Pattern>, ParserError> {
        if cursor.at_punctuator(Punctuator::OpenBracket) {
            self.parse_array_binding_pattern(cursor)
        } else if cursor.at_punctuator(Punctuator::OpenBrace) {
            self.parse_object_binding_pattern(cursor)
        } else {
            self.parse_binding_identifier(cursor)
                .map(Pattern::Binding)
                .map(|pattern| self.arena.alloc_pattern(pattern))
        }
    }

    fn parse_array_binding_pattern(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Pattern>, ParserError> {
        cursor.expect_punctuator(Punctuator::OpenBracket)?;
        let mut index = 0_usize;
        let mut elements = Vec::new();
        while !cursor.at_punctuator(Punctuator::CloseBracket) {
            if cursor.consume_punctuator(Punctuator::Comma).is_some() {
                index = index.saturating_add(1);
                continue;
            }
            if let Some(rest) = cursor.consume_punctuator(Punctuator::DotDotDot) {
                let pattern_ref = self.parse_binding_pattern(cursor)?;
                let pattern = self
                    .arena
                    .pattern(pattern_ref)
                    .cloned()
                    .ok_or(ParserError {
                        span: Some(rest.span()),
                        kind: ParserErrorKind::Syntax("missing rest binding pattern".into()),
                    })?;
                let end = cursor.previous_span();
                elements.push(crate::syntax::ast::PatternElement {
                    span: join_spans(rest.span(), end),
                    index,
                    pattern: Pattern::Rest(Box::new(pattern)),
                    default_value: None,
                });
                if !cursor.at_punctuator(Punctuator::CloseBracket) {
                    return syntax_error(cursor.current(), "rest binding pattern must be last");
                }
                break;
            }
            let start = cursor.current().span();
            let pattern = self.parse_binding_pattern(cursor)?;
            let default_value = if cursor.consume_punctuator(Punctuator::Equal).is_some() {
                Some(self.parse_assignment_expression(cursor)?)
            } else {
                None
            };
            let end = default_value
                .map(|value| self.expr_span(value))
                .unwrap_or_else(|| cursor.previous_span());
            elements.push(crate::syntax::ast::PatternElement {
                span: join_spans(start, end),
                index,
                pattern: self.arena.pattern(pattern).cloned().ok_or(ParserError {
                    span: Some(start),
                    kind: ParserErrorKind::Syntax("missing binding pattern".into()),
                })?,
                default_value,
            });
            index = index.saturating_add(1);
            if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                break;
            }
        }
        cursor.expect_punctuator(Punctuator::CloseBracket)?;
        Ok(self.arena.alloc_pattern(Pattern::Array(elements)))
    }

    fn parse_object_binding_pattern(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Pattern>, ParserError> {
        cursor.expect_punctuator(Punctuator::OpenBrace)?;
        let mut properties = Vec::new();
        while !cursor.at_punctuator(Punctuator::CloseBrace) {
            if let Some(rest) = cursor.consume_punctuator(Punctuator::DotDotDot) {
                let name = self.parse_binding_identifier(cursor)?;
                let end = cursor.previous_span();
                properties.push(crate::syntax::ast::PatternProperty {
                    span: join_spans(rest.span(), end),
                    key: AstPropertyKey::Identifier(name),
                    pattern: Pattern::Rest(Box::new(Pattern::Binding(name))),
                    default_value: None,
                });
                if !cursor.at_punctuator(Punctuator::CloseBrace) {
                    return syntax_error(cursor.current(), "rest binding pattern must be last");
                }
                break;
            }
            let start = cursor.current().span();
            let key = self.parse_object_literal_property_key(cursor)?;
            let (pattern, default_value) = if cursor.consume_punctuator(Punctuator::Colon).is_some()
            {
                let pattern = self.parse_binding_pattern(cursor)?;
                let default_value = if cursor.consume_punctuator(Punctuator::Equal).is_some() {
                    Some(self.parse_assignment_expression(cursor)?)
                } else {
                    None
                };
                let pattern = self.arena.pattern(pattern).cloned().ok_or(ParserError {
                    span: Some(start),
                    kind: ParserErrorKind::Syntax("missing binding pattern".into()),
                })?;
                (pattern, default_value)
            } else {
                let AstPropertyKey::Identifier(name) = key else {
                    return syntax_error(
                        cursor.current(),
                        "object binding shorthand requires an identifier",
                    );
                };
                let default_value = if cursor.consume_punctuator(Punctuator::Equal).is_some() {
                    Some(self.parse_assignment_expression(cursor)?)
                } else {
                    None
                };
                (Pattern::Binding(name), default_value)
            };
            let end = default_value
                .map(|value| self.expr_span(value))
                .unwrap_or_else(|| cursor.previous_span());
            properties.push(crate::syntax::ast::PatternProperty {
                span: join_spans(start, end),
                key,
                pattern,
                default_value,
            });
            if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                break;
            }
        }
        cursor.expect_punctuator(Punctuator::CloseBrace)?;
        Ok(self.arena.alloc_pattern(Pattern::Object(properties)))
    }

    fn parse_function_declaration(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Function)?.span();
        let parsed = self.parse_function_metadata(cursor, true)?;
        let Some(name) = parsed.name else {
            return Err(ParserError {
                span: Some(start),
                kind: ParserErrorKind::Syntax("function declaration requires a name".into()),
            });
        };
        Ok(self
            .arena
            .alloc_statement(Stmt::FunctionDeclaration(FunctionDecl {
                span: join_spans(start, parsed.close_span),
                name,
                metadata: parsed.metadata,
            })))
    }

    fn parse_function_metadata(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        require_name: bool,
    ) -> Result<ParsedFunctionMetadata, ParserError> {
        let name_token = cursor.current().clone();
        let name = if require_name || !cursor.at_punctuator(Punctuator::OpenParen) {
            Some(self.parse_binding_identifier(cursor)?)
        } else {
            None
        };
        self.parse_function_metadata_after_name(
            cursor,
            name,
            name.map(|_| name_token.span()),
            FunctionSyntaxMode::Normal,
        )
    }

    fn parse_function_metadata_after_name(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        name: Option<ParserIdentifier>,
        name_span: Option<SourceSpan>,
        mode: FunctionSyntaxMode,
    ) -> Result<ParsedFunctionMetadata, ParserError> {
        self.arena.identifiers_mut().reserve_identifier_text(
            IdentifierSource::WellKnown(WellKnownIdentifier::Prototype),
            "prototype".into(),
        );
        cursor.expect_punctuator(Punctuator::OpenParen)?;
        let mut parameters = Vec::new();
        if !cursor.at_punctuator(Punctuator::CloseParen) {
            loop {
                let start = cursor.current().span();
                let (pattern, default_value) = if let Some(rest) =
                    cursor.consume_punctuator(Punctuator::DotDotDot)
                {
                    let pattern_ref = self.parse_binding_pattern(cursor)?;
                    let pattern = self
                        .arena
                        .pattern(pattern_ref)
                        .cloned()
                        .ok_or(ParserError {
                            span: Some(rest.span()),
                            kind: ParserErrorKind::Syntax("missing rest parameter".into()),
                        })?;
                    let rest_pattern = self.arena.alloc_pattern(Pattern::Rest(Box::new(pattern)));
                    if !cursor.at_punctuator(Punctuator::CloseParen) {
                        return syntax_error(cursor.current(), "rest parameter must be last");
                    }
                    (rest_pattern, None)
                } else {
                    let pattern = self.parse_binding_pattern(cursor)?;
                    let default_value = if cursor.consume_punctuator(Punctuator::Equal).is_some() {
                        Some(self.parse_assignment_expression(cursor)?)
                    } else {
                        None
                    };
                    (pattern, default_value)
                };
                if default_value.is_some() && cursor.at_punctuator(Punctuator::DotDotDot) {
                    return syntax_error(cursor.current(), "rest parameter must be last");
                }
                let end = default_value
                    .map(|value| self.expr_span(value))
                    .unwrap_or_else(|| cursor.previous_span());
                parameters.push(FunctionParameter {
                    span: join_spans(start, end),
                    pattern,
                    default_value,
                });
                if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                    break;
                }
                if matches!(self.arena.pattern(pattern), Some(Pattern::Rest(_))) {
                    return syntax_error(cursor.current(), "rest parameter must be last");
                }
            }
        }
        cursor.expect_punctuator(Punctuator::CloseParen)?;
        let open = cursor.expect_punctuator(Punctuator::OpenBrace)?;
        let statements = self.parse_statement_list(cursor, Some(Punctuator::CloseBrace))?;
        let close = cursor.expect_punctuator(Punctuator::CloseBrace)?;
        let body_span = join_spans(open.span(), close.span());
        let strict = self.state.strict
            || matches!(
                mode,
                FunctionSyntaxMode::Method
                    | FunctionSyntaxMode::Getter
                    | FunctionSyntaxMode::Setter
            );
        let scope = self.arena.alloc_scope_node(ScopeNode {
            id: SemanticScopeId(self.state.scope_depth.saturating_add(1)),
            kind: ScopeNodeKind::Function,
            span: body_span,
            statements,
            semantics: EarlySemanticInfo {
                strict,
                ..EarlySemanticInfo::default()
            },
            module: None,
        });
        let metadata = self.arena.alloc_function_metadata(FunctionMetadata {
            name_span,
            name,
            mode,
            body_span,
            body: scope,
            parameter_count: parameters.len().try_into().unwrap_or(u32::MAX),
            parameters,
            strict,
            contains_direct_eval: false,
            super_binding: crate::syntax::ast::SuperBinding::NotNeeded,
            private_brand: crate::syntax::ast::PrivateBrandRequirement::None,
        });
        Ok(ParsedFunctionMetadata {
            name,
            metadata,
            close_span: close.span(),
        })
    }

    fn parse_class_declaration(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let class = self.parse_class_expression(cursor, true)?;
        let Some(name) = self
            .arena
            .expression(class)
            .and_then(|expression| match expression {
                Expr::Class(class) => class.name,
                _ => None,
            })
        else {
            return syntax_error(cursor.current(), "class declaration requires a name");
        };
        let binding = self.arena.alloc_pattern(Pattern::Binding(name));
        let span = self.expr_span(class);
        Ok(self
            .arena
            .alloc_statement(Stmt::Declaration(DeclarationStmt {
                span,
                kind: DeclarationSyntaxKind::Class,
                bindings: vec![binding],
                initializers: vec![Some(class)],
            })))
    }

    fn parse_class_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        require_name: bool,
    ) -> Result<AstRef<Expr>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Class)?.span();
        let name = if require_name {
            Some(self.parse_binding_identifier(cursor)?)
        } else if cursor.at_punctuator(Punctuator::OpenBrace)
            || matches!(cursor.current().kind, TokenKind::Keyword(Keyword::Extends))
        {
            None
        } else {
            Some(self.parse_binding_identifier(cursor)?)
        };
        let heritage = if cursor.consume_keyword(Keyword::Extends).is_some() {
            Some(self.parse_expression(cursor)?)
        } else {
            None
        };
        self.arena.identifiers_mut().reserve_identifier_text(
            IdentifierSource::WellKnown(WellKnownIdentifier::Prototype),
            "prototype".into(),
        );
        let open = cursor.expect_punctuator(Punctuator::OpenBrace)?;
        let mut elements = Vec::new();
        while !cursor.at_punctuator(Punctuator::CloseBrace) {
            if cursor.consume_punctuator(Punctuator::Semicolon).is_some() {
                continue;
            }
            elements.push(self.parse_class_element(cursor)?);
        }
        let close = cursor.expect_punctuator(Punctuator::CloseBrace)?;
        if !elements.iter().any(|element| {
            !element.is_static
                && matches!(
                    element.name,
                        ClassElementName::Public(name)
                        if self.arena.identifiers().identifier_text(name) == Some("constructor")
                )
                && element.kind == ClassElementKind::Method
        }) {
            let constructor = self.synthesize_default_class_constructor(close.span());
            elements.insert(0, constructor);
        }
        let _ = open;
        Ok(self.arena.alloc_expression(Expr::Class(ClassExpr {
            span: join_spans(start, close.span()),
            name,
            heritage,
            elements,
        })))
    }

    fn parse_class_element(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<ClassElement, ParserError> {
        let start = cursor.current().span();
        let is_static = if self.current_class_static_modifier(cursor) {
            cursor.bump();
            true
        } else {
            false
        };
        if let Some((kind, mode)) = self.current_class_accessor(cursor) {
            cursor.bump();
            let name_start = cursor.current().span();
            let name = self.parse_class_element_name(cursor)?;
            let metadata_name = match name {
                ClassElementName::Public(name) => Some(name),
                ClassElementName::Private(_) | ClassElementName::Computed(_) => None,
            };
            let parsed = self.parse_function_metadata_after_name(
                cursor,
                metadata_name,
                Some(name_start),
                mode,
            )?;
            let parameter_count = self
                .arena
                .function_metadata(parsed.metadata)
                .map(|metadata| metadata.parameter_count)
                .unwrap_or(u32::MAX);
            match kind {
                ClassElementKind::Getter if parameter_count != 0 => {
                    return Err(ParserError {
                        span: Some(name_start),
                        kind: ParserErrorKind::Syntax("getter must not have parameters".into()),
                    });
                }
                ClassElementKind::Setter if parameter_count != 1 => {
                    return Err(ParserError {
                        span: Some(name_start),
                        kind: ParserErrorKind::Syntax(
                            "setter must have exactly one parameter".into(),
                        ),
                    });
                }
                _ => {}
            }
            return Ok(ClassElement {
                span: join_spans(start, parsed.close_span),
                name,
                kind,
                is_static,
                is_synthesized_default_constructor: false,
                initializer: None,
                metadata: Some(parsed.metadata),
            });
        }

        let name_start = cursor.current().span();
        let name = self.parse_class_element_name(cursor)?;
        if !cursor.at_punctuator(Punctuator::OpenParen) {
            let initializer = if cursor.consume_punctuator(Punctuator::Equal).is_some() {
                Some(self.parse_expression(cursor)?)
            } else {
                None
            };
            let end = if let Some(semicolon) = cursor.consume_punctuator(Punctuator::Semicolon) {
                semicolon.span()
            } else {
                cursor.previous_span()
            };
            return Ok(ClassElement {
                span: join_spans(start, end),
                name,
                kind: ClassElementKind::Field,
                is_static,
                is_synthesized_default_constructor: false,
                initializer,
                metadata: if !is_static {
                    initializer.map(|initializer| {
                        self.synthesize_class_field_initializer(start, end, initializer)
                    })
                } else {
                    None
                },
            });
        }
        let metadata_name = match name {
            ClassElementName::Public(name) => Some(name),
            ClassElementName::Private(_) | ClassElementName::Computed(_) => None,
        };
        let parsed = self.parse_function_metadata_after_name(
            cursor,
            metadata_name,
            Some(name_start),
            FunctionSyntaxMode::Method,
        )?;
        Ok(ClassElement {
            span: join_spans(start, parsed.close_span),
            name,
            kind: ClassElementKind::Method,
            is_static,
            is_synthesized_default_constructor: false,
            initializer: None,
            metadata: Some(parsed.metadata),
        })
    }

    fn current_class_accessor(
        &self,
        cursor: &TokenCursor<'_>,
    ) -> Option<(ClassElementKind, FunctionSyntaxMode)> {
        let kind = match cursor.current().kind {
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Get)) => {
                (ClassElementKind::Getter, FunctionSyntaxMode::Getter)
            }
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Set)) => {
                (ClassElementKind::Setter, FunctionSyntaxMode::Setter)
            }
            _ => return None,
        };

        if matches!(
            cursor.peek(1).kind,
            TokenKind::Punctuator(Punctuator::OpenBracket)
        ) || (matches!(
            cursor.peek(1).kind,
            TokenKind::StringLiteral | TokenKind::NumericLiteral(_)
        ) || identifier_from_token(cursor.peek(1)).is_some())
            && matches!(
                cursor.peek(2).kind,
                TokenKind::Punctuator(Punctuator::OpenParen)
            )
        {
            Some(kind)
        } else {
            None
        }
    }

    fn parse_class_element_name(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<ClassElementName, ParserError> {
        if cursor.consume_punctuator(Punctuator::OpenBracket).is_some() {
            let expression = self.parse_expression(cursor)?;
            cursor.expect_punctuator(Punctuator::CloseBracket)?;
            return Ok(ClassElementName::Computed(expression));
        }
        self.parse_binding_identifier(cursor)
            .map(ClassElementName::Public)
    }

    fn current_class_static_modifier(&self, cursor: &TokenCursor<'_>) -> bool {
        if !matches!(
            cursor.current().kind,
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Static))
        ) {
            return false;
        }

        !matches!(
            cursor.peek(1).kind,
            TokenKind::Punctuator(
                Punctuator::OpenParen
                    | Punctuator::Equal
                    | Punctuator::Semicolon
                    | Punctuator::CloseBrace
            )
        )
    }

    fn synthesize_default_class_constructor(&mut self, span: SourceSpan) -> ClassElement {
        let name = self.arena.identifiers_mut().reserve_identifier_text(
            IdentifierSource::WellKnown(WellKnownIdentifier::Constructor),
            "constructor".into(),
        );
        let body_span = SourceSpan::at(span.start);
        let body = self.arena.alloc_scope_node(ScopeNode {
            id: SemanticScopeId(self.state.scope_depth.saturating_add(1)),
            kind: ScopeNodeKind::Function,
            span: body_span,
            statements: Vec::new(),
            semantics: EarlySemanticInfo {
                strict: true,
                ..EarlySemanticInfo::default()
            },
            module: None,
        });
        let metadata = self.arena.alloc_function_metadata(FunctionMetadata {
            name_span: Some(body_span),
            name: Some(name),
            mode: FunctionSyntaxMode::Method,
            body_span,
            body,
            parameters: Vec::new(),
            parameter_count: 0,
            strict: true,
            contains_direct_eval: false,
            super_binding: crate::syntax::ast::SuperBinding::NotNeeded,
            private_brand: crate::syntax::ast::PrivateBrandRequirement::None,
        });
        ClassElement {
            span: body_span,
            name: ClassElementName::Public(name),
            kind: ClassElementKind::Method,
            is_static: false,
            is_synthesized_default_constructor: true,
            initializer: None,
            metadata: Some(metadata),
        }
    }

    fn synthesize_class_field_initializer(
        &mut self,
        start: SourceSpan,
        end: SourceSpan,
        initializer: AstRef<Expr>,
    ) -> AstRef<FunctionMetadata> {
        let span = join_spans(start, end);
        let return_statement = self.arena.alloc_statement(Stmt::Control(ControlStmt {
            span,
            kind: ControlKind::Return(Some(initializer)),
        }));
        let body = self.arena.alloc_scope_node(ScopeNode {
            id: SemanticScopeId(self.state.scope_depth.saturating_add(1)),
            kind: ScopeNodeKind::Function,
            span,
            statements: vec![return_statement],
            semantics: EarlySemanticInfo {
                strict: true,
                ..EarlySemanticInfo::default()
            },
            module: None,
        });
        self.arena.alloc_function_metadata(FunctionMetadata {
            name_span: None,
            name: None,
            mode: FunctionSyntaxMode::ClassFieldInitializer,
            body_span: span,
            body,
            parameters: Vec::new(),
            parameter_count: 0,
            strict: true,
            contains_direct_eval: false,
            super_binding: crate::syntax::ast::SuperBinding::NotNeeded,
            private_brand: crate::syntax::ast::PrivateBrandRequirement::None,
        })
    }

    fn parse_if_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::If)?.span();
        cursor.expect_punctuator(Punctuator::OpenParen)?;
        let condition = self.parse_expression(cursor)?;
        cursor.expect_punctuator(Punctuator::CloseParen)?;
        let consequent = self.parse_statement(cursor)?;
        let alternate = if cursor.consume_keyword(Keyword::Else).is_some() {
            Some(self.parse_statement(cursor)?)
        } else {
            None
        };
        let span = join_spans(start, self.stmt_span(alternate.unwrap_or(consequent)));
        Ok(self.arena.alloc_statement(Stmt::If(IfStmt {
            span,
            condition,
            consequent,
            alternate,
        })))
    }

    fn parse_do_while_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Do)?.span();
        let body = self.parse_statement(cursor)?;
        cursor.expect_keyword(Keyword::While)?;
        cursor.expect_punctuator(Punctuator::OpenParen)?;
        let condition = self.parse_expression(cursor)?;
        let close = cursor.expect_punctuator(Punctuator::CloseParen)?.span();
        let end = self.consume_statement_terminator(cursor)?.unwrap_or(close);
        Ok(self.arena.alloc_statement(Stmt::DoWhile(DoWhileStmt {
            span: join_spans(start, end),
            body,
            condition,
        })))
    }

    fn parse_while_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::While)?.span();
        cursor.expect_punctuator(Punctuator::OpenParen)?;
        let condition = self.parse_expression(cursor)?;
        cursor.expect_punctuator(Punctuator::CloseParen)?;
        let body = self.parse_statement(cursor)?;
        let span = join_spans(start, self.stmt_span(body));
        Ok(self.arena.alloc_statement(Stmt::While(WhileStmt {
            span,
            condition,
            body,
        })))
    }

    fn parse_switch_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Switch)?.span();
        cursor.expect_punctuator(Punctuator::OpenParen)?;
        let discriminant = self.parse_expression(cursor)?;
        cursor.expect_punctuator(Punctuator::CloseParen)?;
        cursor.expect_punctuator(Punctuator::OpenBrace)?;

        self.state.switch_depth = self.state.switch_depth.saturating_add(1);
        let mut cases = Vec::new();
        let mut saw_default = false;
        while !cursor.at_punctuator(Punctuator::CloseBrace) {
            if cursor.is_eof() {
                return syntax_error(cursor.current(), "unterminated switch statement");
            }

            let clause_token = cursor.current().clone();
            let clause_start = clause_token.span();
            let test = if cursor.consume_keyword(Keyword::Case).is_some() {
                Some(self.parse_expression(cursor)?)
            } else if cursor.consume_keyword(Keyword::Default).is_some() {
                if saw_default {
                    return syntax_error(&clause_token, "duplicate default clause in switch");
                }
                saw_default = true;
                None
            } else {
                return syntax_error(cursor.current(), "expected switch case or default");
            };
            let colon = cursor.expect_punctuator(Punctuator::Colon)?.span();

            let mut statements = Vec::new();
            while !cursor.is_eof()
                && !cursor.at_punctuator(Punctuator::CloseBrace)
                && !matches!(
                    cursor.current().kind,
                    TokenKind::Keyword(Keyword::Case | Keyword::Default)
                )
            {
                statements.push(self.parse_statement(cursor)?);
            }
            let end = statements
                .last()
                .map(|statement| self.stmt_span(*statement))
                .unwrap_or(colon);
            cases.push(SwitchCase {
                span: join_spans(clause_start, end),
                test,
                statements,
            });
        }
        let close = cursor.expect_punctuator(Punctuator::CloseBrace)?.span();
        self.state.switch_depth = self.state.switch_depth.saturating_sub(1);

        Ok(self.arena.alloc_statement(Stmt::Switch(SwitchStmt {
            span: join_spans(start, close),
            discriminant,
            cases,
        })))
    }

    fn parse_for_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::For)?.span();
        cursor.expect_punctuator(Punctuator::OpenParen)?;
        let init = if cursor.consume_punctuator(Punctuator::Semicolon).is_some() {
            None
        } else if let Some(kind) = cursor.current_declaration_keyword() {
            let declaration_start = cursor.bump().span();
            if let Some(name) = identifier_from_token(cursor.current()) {
                if cursor.peek_contextual_keyword(1, ContextualKeyword::Of) {
                    cursor.bump();
                    cursor.expect_contextual_keyword(ContextualKeyword::Of)?;
                    let iterable = self.parse_expression(cursor)?;
                    cursor.expect_punctuator(Punctuator::CloseParen)?;
                    let body = self.parse_statement(cursor)?;
                    return Ok(self.arena.alloc_statement(Stmt::ForOf(ForOfStmt {
                        span: join_spans(start, self.stmt_span(body)),
                        binding: ForOfBinding::Declaration { kind, name },
                        iterable,
                        body,
                    })));
                }
            }
            let (bindings, initializers) = self.parse_declaration_bindings(cursor)?;
            let semicolon = cursor.expect_punctuator(Punctuator::Semicolon)?.span();
            Some(ForInit::Declaration(DeclarationStmt {
                span: join_spans(declaration_start, semicolon),
                kind,
                bindings,
                initializers,
            }))
        } else if let Some(name) = identifier_from_token(cursor.current()) {
            if cursor.peek_contextual_keyword(1, ContextualKeyword::Of) {
                cursor.bump();
                cursor.expect_contextual_keyword(ContextualKeyword::Of)?;
                let iterable = self.parse_expression(cursor)?;
                cursor.expect_punctuator(Punctuator::CloseParen)?;
                let body = self.parse_statement(cursor)?;
                return Ok(self.arena.alloc_statement(Stmt::ForOf(ForOfStmt {
                    span: join_spans(start, self.stmt_span(body)),
                    binding: ForOfBinding::Assignment(name),
                    iterable,
                    body,
                })));
            }
            let expression = self.parse_expression(cursor)?;
            cursor.expect_punctuator(Punctuator::Semicolon)?;
            Some(ForInit::Expression(expression))
        } else {
            let expression = self.parse_expression(cursor)?;
            cursor.expect_punctuator(Punctuator::Semicolon)?;
            Some(ForInit::Expression(expression))
        };
        let condition = if cursor.at_punctuator(Punctuator::Semicolon) {
            None
        } else {
            Some(self.parse_expression(cursor)?)
        };
        cursor.expect_punctuator(Punctuator::Semicolon)?;
        let update = if cursor.at_punctuator(Punctuator::CloseParen) {
            None
        } else {
            Some(self.parse_expression(cursor)?)
        };
        cursor.expect_punctuator(Punctuator::CloseParen)?;
        let body = self.parse_statement(cursor)?;
        Ok(self.arena.alloc_statement(Stmt::For(ForStmt {
            span: join_spans(start, self.stmt_span(body)),
            init,
            condition,
            update,
            body,
        })))
    }

    fn parse_return_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Return)?.span();
        let value = if cursor.statement_is_terminated() {
            None
        } else {
            Some(self.parse_expression(cursor)?)
        };
        let end = self.consume_statement_terminator(cursor)?.unwrap_or(start);
        Ok(self.arena.alloc_statement(Stmt::Control(ControlStmt {
            span: join_spans(start, end),
            kind: ControlKind::Return(value),
        })))
    }

    fn parse_throw_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Throw)?.span();
        if cursor.statement_is_terminated() {
            return syntax_error(cursor.current(), "throw requires an expression");
        }
        let value = self.parse_expression(cursor)?;
        let end = self.consume_statement_terminator(cursor)?.unwrap_or(start);
        Ok(self.arena.alloc_statement(Stmt::Control(ControlStmt {
            span: join_spans(start, end),
            kind: ControlKind::Throw(value),
        })))
    }

    fn parse_try_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.expect_keyword(Keyword::Try)?.span();
        let body = self.parse_block_statement(cursor)?;
        let mut catch = None;
        let mut finally = None;
        if cursor.consume_keyword(Keyword::Catch).is_some() {
            let catch_start = cursor.previous_span();
            let binding = if cursor.consume_punctuator(Punctuator::OpenParen).is_some() {
                let binding = self.parse_binding_identifier(cursor)?;
                cursor.expect_punctuator(Punctuator::CloseParen)?;
                Some(binding)
            } else {
                None
            };
            let body = self.parse_block_statement(cursor)?;
            catch = Some(crate::syntax::ast::CatchClause {
                span: join_spans(catch_start, self.stmt_span(body)),
                binding,
                body,
            });
        }
        if cursor.consume_keyword(Keyword::Finally).is_some() {
            finally = Some(self.parse_block_statement(cursor)?);
        }
        if catch.is_none() && finally.is_none() {
            return syntax_error(cursor.current(), "try requires catch or finally");
        }
        let end = finally
            .map(|statement| self.stmt_span(statement))
            .or_else(|| catch.map(|clause| clause.span))
            .unwrap_or_else(|| self.stmt_span(body));
        Ok(self.arena.alloc_statement(Stmt::Try(TryStmt {
            span: join_spans(start, end),
            body,
            catch,
            finally,
        })))
    }

    fn parse_jump_statement(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        kind: JumpStatementKind,
    ) -> Result<AstRef<Stmt>, ParserError> {
        let start = cursor.bump().span();
        let label = if cursor.statement_is_terminated() {
            None
        } else {
            Some(self.parse_binding_identifier(cursor)?)
        };
        let end = self.consume_statement_terminator(cursor)?.unwrap_or(start);
        let kind = match kind {
            JumpStatementKind::Break => ControlKind::Break(label),
            JumpStatementKind::Continue => ControlKind::Continue(label),
        };
        Ok(self.arena.alloc_statement(Stmt::Control(ControlStmt {
            span: join_spans(start, end),
            kind,
        })))
    }

    fn parse_binding_identifier(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<ParserIdentifier, ParserError> {
        let token = cursor.current().clone();
        if let Some(identifier) = identifier_from_token(&token) {
            cursor.bump();
            Ok(identifier)
        } else {
            syntax_error(&token, "expected binding identifier")
        }
    }

    fn parse_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        self.parse_assignment_expression(cursor)
    }

    fn parse_assignment_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let left = self.parse_conditional_expression(cursor)?;
        let Some(op) = cursor.current_assignment_operator() else {
            return Ok(left);
        };
        cursor.bump();
        let value = self.parse_assignment_expression(cursor)?;
        let target = self.arena.alloc_pattern(Pattern::AssignmentTarget(left));
        let span = join_spans(self.expr_span(left), self.expr_span(value));
        Ok(self
            .arena
            .alloc_expression(Expr::Assignment(AssignmentExpr {
                span,
                op,
                target,
                value,
                context: AssignmentContext::Expression,
            })))
    }

    fn parse_conditional_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let test = self.parse_binary_expression(cursor, 0)?;
        if cursor.consume_punctuator(Punctuator::Question).is_none() {
            return Ok(test);
        }
        let consequent = self.parse_assignment_expression(cursor)?;
        cursor.expect_punctuator(Punctuator::Colon)?;
        let alternate = self.parse_assignment_expression(cursor)?;
        let span = join_spans(self.expr_span(test), self.expr_span(alternate));
        Ok(self
            .arena
            .alloc_expression(Expr::Conditional(ConditionalExpr {
                span,
                test,
                consequent,
                alternate,
            })))
    }

    fn parse_binary_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        min_precedence: u8,
    ) -> Result<AstRef<Expr>, ParserError> {
        let mut left = self.parse_unary_expression(cursor)?;
        while let Some((op, precedence, right_associative)) = cursor.current_binary_operator() {
            if precedence < min_precedence {
                break;
            }
            cursor.bump();
            let next_precedence = if right_associative {
                precedence
            } else {
                precedence.saturating_add(1)
            };
            let right = self.parse_binary_expression(cursor, next_precedence)?;
            let span = join_spans(self.expr_span(left), self.expr_span(right));
            left = self.arena.alloc_expression(Expr::Binary(BinaryExpr {
                span,
                op,
                left,
                right,
            }));
        }
        Ok(left)
    }

    fn parse_unary_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let token = cursor.current().clone();
        let op = match token.kind {
            TokenKind::Punctuator(Punctuator::PlusPlus) => Some(UnaryOperator::PreIncrement),
            TokenKind::Punctuator(Punctuator::MinusMinus) => Some(UnaryOperator::PreDecrement),
            TokenKind::Punctuator(Punctuator::Plus) => Some(UnaryOperator::Plus),
            TokenKind::Punctuator(Punctuator::Minus) => Some(UnaryOperator::Minus),
            TokenKind::Punctuator(Punctuator::Exclamation) => Some(UnaryOperator::LogicalNot),
            TokenKind::Punctuator(Punctuator::Tilde) => Some(UnaryOperator::BitNot),
            TokenKind::Keyword(Keyword::Typeof) => Some(UnaryOperator::Typeof),
            TokenKind::Keyword(Keyword::Void) => Some(UnaryOperator::Void),
            TokenKind::Keyword(Keyword::Delete) => Some(UnaryOperator::Delete),
            _ => None,
        };
        let Some(op) = op else {
            return self.parse_postfix_expression(cursor);
        };
        cursor.bump();
        let argument = self.parse_unary_expression(cursor)?;
        let span = join_spans(token.span(), self.expr_span(argument));
        Ok(self
            .arena
            .alloc_expression(Expr::Unary(UnaryExpr { span, op, argument })))
    }

    fn parse_postfix_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let expr = self.parse_primary_expression(cursor)?;
        let expr = self.parse_postfix_suffixes(cursor, expr, true)?;
        if cursor.current().flags.has_line_terminator_before {
            return Ok(expr);
        }
        let token = cursor.current().clone();
        let op = match token.kind {
            TokenKind::Punctuator(Punctuator::PlusPlus) => Some(UnaryOperator::PostIncrement),
            TokenKind::Punctuator(Punctuator::MinusMinus) => Some(UnaryOperator::PostDecrement),
            _ => None,
        };
        let Some(op) = op else {
            return Ok(expr);
        };
        cursor.bump();
        let span = join_spans(self.expr_span(expr), token.span());
        Ok(self.arena.alloc_expression(Expr::Unary(UnaryExpr {
            span,
            op,
            argument: expr,
        })))
    }

    fn parse_postfix_suffixes(
        &mut self,
        cursor: &mut TokenCursor<'_>,
        mut expr: AstRef<Expr>,
        allow_call: bool,
    ) -> Result<AstRef<Expr>, ParserError> {
        loop {
            if allow_call && cursor.consume_punctuator(Punctuator::OpenParen).is_some() {
                let (arguments, close) = self.parse_argument_list_after_open_paren(cursor)?;
                let span = join_spans(self.expr_span(expr), close.span());
                expr = self.arena.alloc_expression(Expr::Call(CallExpr {
                    span,
                    callee: expr,
                    arguments,
                    optional: false,
                    tail_position: false,
                }));
            } else if cursor.consume_punctuator(Punctuator::Dot).is_some() {
                let name = self.parse_binding_identifier(cursor)?;
                let span = join_spans(self.expr_span(expr), cursor.previous_span());
                expr = self.arena.alloc_expression(Expr::Member(MemberExpr {
                    span,
                    base: expr,
                    member: MemberKind::Dot(name),
                    optional: false,
                }));
            } else if cursor.consume_punctuator(Punctuator::OpenBracket).is_some() {
                let index = self.parse_expression(cursor)?;
                let close = cursor.expect_punctuator(Punctuator::CloseBracket)?;
                let span = join_spans(self.expr_span(expr), close.span());
                expr = self.arena.alloc_expression(Expr::Member(MemberExpr {
                    span,
                    base: expr,
                    member: MemberKind::Bracket(index),
                    optional: false,
                }));
            } else if matches!(cursor.current().kind, TokenKind::TemplateLiteral(_)) {
                return syntax_error(
                    cursor.current(),
                    "tagged template literals are not supported",
                );
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_argument_list_after_open_paren(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<(Vec<AstRef<Expr>>, Token), ParserError> {
        let mut arguments = Vec::new();
        if !cursor.at_punctuator(Punctuator::CloseParen) {
            loop {
                arguments.push(self.parse_expression(cursor)?);
                if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                    break;
                }
                if cursor.at_punctuator(Punctuator::CloseParen) {
                    break;
                }
            }
        }
        let close = cursor.expect_punctuator(Punctuator::CloseParen)?.clone();
        Ok((arguments, close))
    }

    fn parse_new_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let start = cursor.expect_keyword(Keyword::New)?.span();
        let callee = self.parse_primary_expression(cursor)?;
        let callee = self.parse_postfix_suffixes(cursor, callee, false)?;
        let (arguments, end) = if cursor.consume_punctuator(Punctuator::OpenParen).is_some() {
            let (arguments, close) = self.parse_argument_list_after_open_paren(cursor)?;
            (arguments, close.span())
        } else {
            (Vec::new(), self.expr_span(callee))
        };
        Ok(self.arena.alloc_expression(Expr::New(NewExpr {
            span: join_spans(start, end),
            callee,
            arguments,
        })))
    }

    fn parse_primary_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let token = cursor.current().clone();
        match token.kind {
            TokenKind::NumericLiteral(kind) => {
                cursor.bump();
                let literal = match kind {
                    NumericLiteralKind::Integer | NumericLiteralKind::Double => {
                        LiteralKind::Number {
                            value: self.parse_number_literal_value(&token, kind)?,
                        }
                    }
                    NumericLiteralKind::BigInt { .. } => LiteralKind::BigInt {
                        text: self.parse_bigint_literal_text(&token)?,
                    },
                };
                Ok(self.arena.alloc_expression(Expr::Literal(LiteralExpr {
                    span: token.span(),
                    kind: literal,
                })))
            }
            TokenKind::StringLiteral => {
                cursor.bump();
                let text = self.parse_string_literal_text(&token)?;
                Ok(self.arena.alloc_expression(Expr::Literal(LiteralExpr {
                    span: token.span(),
                    kind: LiteralKind::String { text },
                })))
            }
            TokenKind::RegExpLiteral => {
                cursor.bump();
                let (pattern, flags) = self.parse_regexp_literal_parts(&token)?;
                Ok(self.arena.alloc_expression(Expr::Literal(LiteralExpr {
                    span: token.span(),
                    kind: LiteralKind::RegExp { pattern, flags },
                })))
            }
            TokenKind::Keyword(Keyword::Null) => {
                cursor.bump();
                Ok(self.arena.alloc_expression(Expr::Literal(LiteralExpr {
                    span: token.span(),
                    kind: LiteralKind::Null,
                })))
            }
            TokenKind::Keyword(Keyword::True) => {
                cursor.bump();
                Ok(self.arena.alloc_expression(Expr::Literal(LiteralExpr {
                    span: token.span(),
                    kind: LiteralKind::Boolean(true),
                })))
            }
            TokenKind::Keyword(Keyword::False) => {
                cursor.bump();
                Ok(self.arena.alloc_expression(Expr::Literal(LiteralExpr {
                    span: token.span(),
                    kind: LiteralKind::Boolean(false),
                })))
            }
            TokenKind::Keyword(Keyword::This) => {
                cursor.bump();
                Ok(self.arena.alloc_expression(Expr::Name(NameExpr {
                    span: token.span(),
                    name: identifier_from_token(&token).unwrap_or(ParserIdentifier(u32::MAX)),
                    kind: NameKind::This,
                })))
            }
            TokenKind::Keyword(Keyword::Super) => {
                cursor.bump();
                Ok(self.arena.alloc_expression(Expr::Name(NameExpr {
                    span: token.span(),
                    name: identifier_from_token(&token).unwrap_or(ParserIdentifier(u32::MAX)),
                    kind: NameKind::Super,
                })))
            }
            TokenKind::Keyword(Keyword::New) => self.parse_new_expression(cursor),
            TokenKind::Keyword(Keyword::Function) => {
                cursor.bump();
                let parsed = self.parse_function_metadata(cursor, false)?;
                Ok(self.arena.alloc_expression(Expr::Function(parsed.metadata)))
            }
            TokenKind::Keyword(Keyword::Class) => self.parse_class_expression(cursor, false),
            TokenKind::Identifier(_) | TokenKind::Keyword(Keyword::Contextual(_)) => {
                let name = identifier_from_token(&token).ok_or_else(|| ParserError {
                    span: Some(token.span()),
                    kind: ParserErrorKind::Syntax("identifier token is missing symbol".into()),
                })?;
                cursor.bump();
                Ok(self.arena.alloc_expression(Expr::Name(NameExpr {
                    span: token.span(),
                    name,
                    kind: NameKind::Resolve,
                })))
            }
            TokenKind::Punctuator(Punctuator::OpenParen) => {
                cursor.bump();
                let expr = self.parse_expression(cursor)?;
                cursor.expect_punctuator(Punctuator::CloseParen)?;
                Ok(expr)
            }
            TokenKind::TemplateLiteral(_) => self.parse_template_expression(cursor),
            TokenKind::Punctuator(Punctuator::OpenBrace) => self.parse_object_literal(cursor),
            TokenKind::Punctuator(Punctuator::OpenBracket) => self.parse_array_literal(cursor),
            _ => syntax_error(&token, "expected expression"),
        }
    }

    fn parse_template_expression(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let first = cursor.current().clone();
        let TokenKind::TemplateLiteral(kind) = first.kind else {
            return syntax_error(&first, "expected template literal");
        };
        match kind {
            TemplateTokenKind::NoSubstitution => {
                cursor.bump();
                let quasi = self.parse_template_part(&first, kind)?;
                Ok(self.arena.alloc_expression(Expr::Template(TemplateExpr {
                    span: first.span(),
                    tag: None,
                    quasis: vec![quasi],
                    expressions: Vec::new(),
                })))
            }
            TemplateTokenKind::Head => {
                cursor.bump();
                let mut quasis = vec![self.parse_template_part(&first, kind)?];
                let mut expressions = Vec::new();
                loop {
                    expressions.push(self.parse_expression(cursor)?);
                    cursor.expect_punctuator(Punctuator::CloseBrace)?;
                    let quasi_token = cursor.current().clone();
                    let TokenKind::TemplateLiteral(quasi_kind) = quasi_token.kind else {
                        return syntax_error(&quasi_token, "expected template continuation");
                    };
                    match quasi_kind {
                        TemplateTokenKind::Middle => {
                            cursor.bump();
                            quasis.push(self.parse_template_part(&quasi_token, quasi_kind)?);
                        }
                        TemplateTokenKind::Tail => {
                            cursor.bump();
                            quasis.push(self.parse_template_part(&quasi_token, quasi_kind)?);
                            let span = join_spans(first.span(), quasi_token.span());
                            return Ok(self.arena.alloc_expression(Expr::Template(TemplateExpr {
                                span,
                                tag: None,
                                quasis,
                                expressions,
                            })));
                        }
                        TemplateTokenKind::Head | TemplateTokenKind::NoSubstitution => {
                            return syntax_error(&quasi_token, "expected template middle or tail");
                        }
                    }
                }
            }
            TemplateTokenKind::Middle | TemplateTokenKind::Tail => {
                syntax_error(&first, "unexpected template continuation")
            }
        }
    }

    fn parse_template_part(
        &mut self,
        token: &Token,
        kind: TemplateTokenKind,
    ) -> Result<TemplatePart, ParserError> {
        let content_span = template_content_span(token, kind)?;
        let raw_text = self.source_ascii(content_span).ok_or_else(|| ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax(
                "core parser template literals currently require ascii source text".into(),
            ),
        })?;
        let cooked_text = cook_template_text(token, &raw_text)?;
        let identifiers = self.arena.identifiers_mut();
        let raw = identifiers.reserve_identifier_text(IdentifierSource::RawString, raw_text);
        let cooked =
            identifiers.reserve_identifier_text(IdentifierSource::CookedString, cooked_text);
        Ok(TemplatePart {
            cooked: Some(cooked),
            raw,
            span: token.span(),
        })
    }

    fn parse_object_literal(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let open = cursor.expect_punctuator(Punctuator::OpenBrace)?;
        let mut properties = Vec::new();
        if !cursor.at_punctuator(Punctuator::CloseBrace) {
            loop {
                let key_start = cursor.current().span();
                let (kind, key, value) =
                    if cursor.consume_punctuator(Punctuator::DotDotDot).is_some() {
                        let value = self.parse_assignment_expression(cursor)?;
                        (
                            ObjectLiteralPropertyKind::Spread,
                            AstPropertyKey::Computed(value),
                            value,
                        )
                    } else if let Some((kind, mode)) = self.current_object_literal_accessor(cursor)
                    {
                        cursor.bump();
                        let key_start = cursor.current().span();
                        let key = self.parse_object_literal_property_key(cursor)?;
                        let metadata_name = match key {
                            AstPropertyKey::Identifier(name)
                            | AstPropertyKey::String(name)
                            | AstPropertyKey::Number(name) => Some(name),
                            AstPropertyKey::Private(_) | AstPropertyKey::Computed(_) => None,
                        };
                        let parsed = self.parse_function_metadata_after_name(
                            cursor,
                            metadata_name,
                            Some(key_start),
                            mode,
                        )?;
                        let parameter_count = self
                            .arena
                            .function_metadata(parsed.metadata)
                            .map(|metadata| metadata.parameter_count)
                            .unwrap_or(u32::MAX);
                        match kind {
                            ObjectLiteralPropertyKind::Getter if parameter_count != 0 => {
                                return Err(ParserError {
                                    span: Some(key_start),
                                    kind: ParserErrorKind::Syntax(
                                        "getter must not have parameters".into(),
                                    ),
                                });
                            }
                            ObjectLiteralPropertyKind::Setter if parameter_count != 1 => {
                                return Err(ParserError {
                                    span: Some(key_start),
                                    kind: ParserErrorKind::Syntax(
                                        "setter must have exactly one parameter".into(),
                                    ),
                                });
                            }
                            _ => {}
                        }
                        let value = self.arena.alloc_expression(Expr::Function(parsed.metadata));
                        (kind, key, value)
                    } else {
                        let key = self.parse_object_literal_property_key(cursor)?;
                        let value = if cursor.consume_punctuator(Punctuator::Colon).is_some() {
                            self.parse_expression(cursor)?
                        } else {
                            let AstPropertyKey::Identifier(name) = key else {
                                return syntax_error(
                                    cursor.current(),
                                    "object literal shorthand requires an identifier",
                                );
                            };
                            self.arena.alloc_expression(Expr::Name(NameExpr {
                                span: key_start,
                                name,
                                kind: NameKind::Resolve,
                            }))
                        };
                        (ObjectLiteralPropertyKind::Data, key, value)
                    };
                properties.push(ObjectLiteralProperty {
                    span: join_spans(key_start, self.expr_span(value)),
                    key,
                    kind,
                    value,
                });
                if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                    break;
                }
                if cursor.at_punctuator(Punctuator::CloseBrace) {
                    break;
                }
            }
        }
        let close = cursor.expect_punctuator(Punctuator::CloseBrace)?;
        Ok(self.arena.alloc_expression(Expr::Object(ObjectLiteralExpr {
            span: join_spans(open.span(), close.span()),
            properties,
        })))
    }

    fn parse_object_literal_property_key(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstPropertyKey, ParserError> {
        let token = cursor.current().clone();
        match token.kind {
            TokenKind::Punctuator(Punctuator::OpenBracket) => {
                cursor.bump();
                let expression = self.parse_expression(cursor)?;
                cursor.expect_punctuator(Punctuator::CloseBracket)?;
                Ok(AstPropertyKey::Computed(expression))
            }
            TokenKind::StringLiteral => {
                cursor.bump();
                self.parse_string_literal_text(&token)
                    .map(AstPropertyKey::String)
            }
            TokenKind::NumericLiteral(kind) => {
                cursor.bump();
                self.parse_number_literal_property_key(&token, kind)
                    .map(AstPropertyKey::Number)
            }
            _ => self
                .parse_binding_identifier(cursor)
                .map(AstPropertyKey::Identifier),
        }
    }

    fn current_object_literal_accessor(
        &self,
        cursor: &TokenCursor<'_>,
    ) -> Option<(ObjectLiteralPropertyKind, FunctionSyntaxMode)> {
        let kind = match cursor.current().kind {
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Get)) => (
                ObjectLiteralPropertyKind::Getter,
                FunctionSyntaxMode::Getter,
            ),
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Set)) => (
                ObjectLiteralPropertyKind::Setter,
                FunctionSyntaxMode::Setter,
            ),
            _ => return None,
        };

        if matches!(
            cursor.peek(1).kind,
            TokenKind::Punctuator(Punctuator::OpenBracket)
        ) || (identifier_from_token(cursor.peek(1)).is_some()
            && matches!(
                cursor.peek(2).kind,
                TokenKind::Punctuator(Punctuator::OpenParen)
            ))
        {
            Some(kind)
        } else {
            None
        }
    }

    fn parse_array_literal(
        &mut self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<AstRef<Expr>, ParserError> {
        let open = cursor.expect_punctuator(Punctuator::OpenBracket)?;
        let mut elements = Vec::new();
        while !cursor.at_punctuator(Punctuator::CloseBracket) {
            if let Some(comma) = cursor.consume_punctuator(Punctuator::Comma) {
                elements.push(ArrayLiteralElement::Elision(comma.span()));
                continue;
            }
            if let Some(spread) = cursor.consume_punctuator(Punctuator::DotDotDot) {
                let value = self.parse_expression(cursor)?;
                elements.push(ArrayLiteralElement::Spread {
                    span: join_spans(spread.span(), self.expr_span(value)),
                    value,
                });
            } else {
                elements.push(ArrayLiteralElement::Expression(
                    self.parse_assignment_expression(cursor)?,
                ));
            }
            if cursor.consume_punctuator(Punctuator::Comma).is_none() {
                break;
            }
        }
        let close = cursor.expect_punctuator(Punctuator::CloseBracket)?;
        Ok(self.arena.alloc_expression(Expr::Array(ArrayLiteralExpr {
            span: join_spans(open.span(), close.span()),
            elements,
        })))
    }

    fn consume_statement_terminator(
        &self,
        cursor: &mut TokenCursor<'_>,
    ) -> Result<Option<SourceSpan>, ParserError> {
        if let Some(token) = cursor.consume_punctuator(Punctuator::Semicolon) {
            return Ok(Some(token.span()));
        }
        if cursor.statement_is_terminated() {
            return Ok(None);
        }
        syntax_error(cursor.current(), "expected statement terminator")
    }

    fn finish_root(&mut self, statements: Vec<AstRef<Stmt>>) -> AstRoot {
        let kind = match self.session.config.mode {
            ParseMode::Module => ScopeNodeKind::Module,
            ParseMode::FunctionBody => ScopeNodeKind::Function,
            ParseMode::Program | ParseMode::Eval => ScopeNodeKind::Script,
        };
        let scope = ScopeNode {
            id: SemanticScopeId(0),
            kind,
            span: self.session.source.range(),
            statements,
            semantics: EarlySemanticInfo {
                strict: self.state.strict,
                ..EarlySemanticInfo::default()
            },
            module: if matches!(kind, ScopeNodeKind::Module) {
                Some(ModuleAnalysis::default())
            } else {
                None
            },
        };
        let root = self.arena.alloc_scope_node(scope);
        match kind {
            ScopeNodeKind::Module => AstRoot::Module(root),
            ScopeNodeKind::Function => AstRoot::Function(root),
            _ => AstRoot::Script(root),
        }
    }

    fn expr_span(&self, expr: AstRef<Expr>) -> SourceSpan {
        self.arena
            .expression(expr)
            .map(expr_span)
            .unwrap_or_else(|| self.session.source.range())
    }

    fn stmt_span(&self, statement: AstRef<Stmt>) -> SourceSpan {
        match self.arena.statement(statement) {
            Some(Stmt::Empty(span)) => *span,
            Some(Stmt::Expression(expr)) => self.expr_span(*expr),
            Some(Stmt::Block(block)) => block.span,
            Some(Stmt::Declaration(declaration)) => declaration.span,
            Some(Stmt::FunctionDeclaration(declaration)) => declaration.span,
            Some(Stmt::If(statement)) => statement.span,
            Some(Stmt::DoWhile(statement)) => statement.span,
            Some(Stmt::While(statement)) => statement.span,
            Some(Stmt::Switch(statement)) => statement.span,
            Some(Stmt::For(statement)) => statement.span,
            Some(Stmt::ForOf(statement)) => statement.span,
            Some(Stmt::Try(statement)) => statement.span,
            Some(Stmt::Control(control)) => control.span,
            Some(Stmt::Module(module)) => match module {
                ModuleItem::Import(declaration) => declaration.span,
                ModuleItem::Export(declaration) => declaration.span,
            },
            None => self.session.source.range(),
        }
    }

    fn source_ascii(&self, span: SourceSpan) -> Option<String> {
        let mut bytes = Vec::new();
        for offset in span.start.0..span.end.0 {
            let unit = self
                .session
                .source
                .unit_at(crate::syntax::source::SourcePosition(offset))?;
            let byte = u8::try_from(unit).ok()?;
            if !byte.is_ascii() {
                return None;
            }
            bytes.push(byte);
        }
        String::from_utf8(bytes).ok()
    }

    fn parse_number_literal_value(
        &self,
        token: &Token,
        kind: NumericLiteralKind,
    ) -> Result<NumberLiteralValue, ParserError> {
        let raw = self.source_ascii(token.span()).ok_or_else(|| ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax(
                "core parser numeric literals currently require ascii source text".into(),
            ),
        })?;
        if kind == NumericLiteralKind::Integer {
            if let Some((digits, radix)) = non_decimal_literal_digits_and_radix(&raw) {
                return parse_non_decimal_number_literal(digits, radix).ok_or_else(|| {
                    ParserError {
                        span: Some(token.span()),
                        kind: ParserErrorKind::Syntax("invalid numeric literal token".into()),
                    }
                });
            }
            if let Ok(value) = raw.parse::<i32>() {
                return Ok(NumberLiteralValue::Int32(value));
            }
        }
        let value = raw.parse::<f64>().map_err(|_| ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax("invalid numeric literal token".into()),
        })?;
        Ok(NumberLiteralValue::DoubleBits(value.to_bits()))
    }

    fn parse_bigint_literal_text(
        &mut self,
        token: &Token,
    ) -> Result<ParserIdentifier, ParserError> {
        let raw = self.source_ascii(token.span()).ok_or_else(|| ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax(
                "core parser bigint literals currently require ascii source text".into(),
            ),
        })?;
        Ok(self
            .arena
            .identifiers_mut()
            .reserve_identifier_text(IdentifierSource::NumericLiteral, raw))
    }

    fn parse_number_literal_property_key(
        &mut self,
        token: &Token,
        kind: NumericLiteralKind,
    ) -> Result<ParserIdentifier, ParserError> {
        let value = self.parse_number_literal_value(token, kind)?;
        let text = match value {
            NumberLiteralValue::Int32(value) => value.to_string(),
            NumberLiteralValue::DoubleBits(bits) => {
                let value = f64::from_bits(bits);
                if value.is_nan() {
                    "NaN".into()
                } else if value == f64::INFINITY {
                    "Infinity".into()
                } else if value == f64::NEG_INFINITY {
                    "-Infinity".into()
                } else if value == 0.0 {
                    "0".into()
                } else if value.fract() == 0.0 {
                    format!("{value:.0}")
                } else {
                    value.to_string()
                }
            }
        };
        Ok(self
            .arena
            .identifiers_mut()
            .reserve_identifier_text(IdentifierSource::CookedString, text))
    }

    fn parse_string_literal_text(
        &mut self,
        token: &Token,
    ) -> Result<ParserIdentifier, ParserError> {
        let raw = self.source_ascii(token.span()).ok_or_else(|| ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax(
                "core parser string literals currently require ascii source text".into(),
            ),
        })?;
        let bytes = raw.as_bytes();
        if bytes.len() < 2 || !matches!(bytes.first(), Some(b'\'' | b'"')) {
            return Err(ParserError {
                span: Some(token.span()),
                kind: ParserErrorKind::Syntax("invalid string literal token".into()),
            });
        }
        let quote = bytes[0];
        if bytes.last().copied() != Some(quote) {
            return Err(ParserError {
                span: Some(token.span()),
                kind: ParserErrorKind::Syntax("unterminated string literal token".into()),
            });
        }

        let mut text = String::new();
        let mut index = 1;
        while index + 1 < bytes.len() {
            let byte = bytes[index];
            index += 1;
            if byte != b'\\' {
                text.push(char::from(byte));
                continue;
            }
            if index + 1 > bytes.len() {
                return Err(ParserError {
                    span: Some(token.span()),
                    kind: ParserErrorKind::Syntax("invalid string escape".into()),
                });
            }
            let escaped = bytes[index];
            index += 1;
            match escaped {
                b'\'' if quote == b'\'' => text.push('\''),
                b'"' if quote == b'"' => text.push('"'),
                b'\\' => text.push('\\'),
                b'n' => text.push('\n'),
                b'r' => text.push('\r'),
                b't' => text.push('\t'),
                b'b' => text.push('\u{0008}'),
                b'f' => text.push('\u{000c}'),
                b'v' => text.push('\u{000b}'),
                b'0' => text.push('\0'),
                _ => {
                    return Err(ParserError {
                        span: Some(token.span()),
                        kind: ParserErrorKind::Syntax(
                            "unsupported string escape in core parser".into(),
                        ),
                    });
                }
            }
        }
        Ok(self
            .arena
            .identifiers_mut()
            .reserve_identifier_text(IdentifierSource::CookedString, text))
    }

    fn parse_regexp_literal_parts(
        &mut self,
        token: &Token,
    ) -> Result<(ParserIdentifier, ParserIdentifier), ParserError> {
        let raw = self.source_ascii(token.span()).ok_or_else(|| ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax(
                "core parser regexp literals currently require ascii source text".into(),
            ),
        })?;
        let bytes = raw.as_bytes();
        if bytes.first().copied() != Some(b'/') {
            return Err(ParserError {
                span: Some(token.span()),
                kind: ParserErrorKind::Syntax("invalid regexp literal token".into()),
            });
        }

        let mut in_class = false;
        let mut index = 1;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index = index.saturating_add(2),
                b'[' => {
                    in_class = true;
                    index += 1;
                }
                b']' => {
                    in_class = false;
                    index += 1;
                }
                b'/' if !in_class => {
                    let pattern = raw[1..index].to_owned();
                    let flags = raw[index + 1..].to_owned();
                    let pattern = self
                        .arena
                        .identifiers_mut()
                        .reserve_identifier_text(IdentifierSource::RawString, pattern);
                    let flags = self
                        .arena
                        .identifiers_mut()
                        .reserve_identifier_text(IdentifierSource::SourceSlice, flags);
                    return Ok((pattern, flags));
                }
                _ => index += 1,
            }
        }

        Err(ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax("unterminated regexp literal token".into()),
        })
    }
}

fn regexp_literal_allowed_after(previous: Option<TokenKind>) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    match previous {
        TokenKind::Keyword(keyword) => matches!(
            keyword,
            Keyword::Return
                | Keyword::Throw
                | Keyword::Case
                | Keyword::Delete
                | Keyword::Void
                | Keyword::Typeof
                | Keyword::New
                | Keyword::In
                | Keyword::Instanceof
        ),
        TokenKind::Punctuator(punctuator) => matches!(
            punctuator,
            Punctuator::OpenBrace
                | Punctuator::OpenParen
                | Punctuator::OpenBracket
                | Punctuator::Comma
                | Punctuator::Question
                | Punctuator::Semicolon
                | Punctuator::Colon
                | Punctuator::Equal
                | Punctuator::PlusEqual
                | Punctuator::MinusEqual
                | Punctuator::MultiplyEqual
                | Punctuator::DivideEqual
                | Punctuator::LeftShiftEqual
                | Punctuator::RightShiftEqual
                | Punctuator::UnsignedRightShiftEqual
                | Punctuator::ModEqual
                | Punctuator::PowEqual
                | Punctuator::BitAndEqual
                | Punctuator::BitXorEqual
                | Punctuator::BitOrEqual
                | Punctuator::CoalesceEqual
                | Punctuator::OrEqual
                | Punctuator::AndEqual
                | Punctuator::ArrowFunction
                | Punctuator::Exclamation
                | Punctuator::Tilde
                | Punctuator::Coalesce
                | Punctuator::Or
                | Punctuator::And
                | Punctuator::BitOr
                | Punctuator::BitXor
                | Punctuator::BitAnd
                | Punctuator::EqualEqual
                | Punctuator::NotEqual
                | Punctuator::StrictEqual
                | Punctuator::StrictNotEqual
                | Punctuator::LessThan
                | Punctuator::GreaterThan
                | Punctuator::LessEqual
                | Punctuator::GreaterEqual
                | Punctuator::LeftShift
                | Punctuator::RightShift
                | Punctuator::UnsignedRightShift
                | Punctuator::Plus
                | Punctuator::Minus
                | Punctuator::Multiply
                | Punctuator::Divide
                | Punctuator::Mod
                | Punctuator::Pow
        ),
        TokenKind::EndOfFile
        | TokenKind::Identifier(_)
        | TokenKind::NumericLiteral(_)
        | TokenKind::StringLiteral
        | TokenKind::TemplateLiteral(_)
        | TokenKind::RegExpLiteral
        | TokenKind::Error(_) => false,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TemplateLexingContext {
    brace_depth: u32,
}

fn update_template_lexing_context(stack: &mut Vec<TemplateLexingContext>, kind: TokenKind) -> bool {
    match kind {
        TokenKind::TemplateLiteral(TemplateTokenKind::Head | TemplateTokenKind::Middle) => {
            stack.push(TemplateLexingContext::default());
            false
        }
        TokenKind::Punctuator(Punctuator::OpenBrace) => {
            if let Some(context) = stack.last_mut() {
                context.brace_depth = context.brace_depth.saturating_add(1);
            }
            false
        }
        TokenKind::Punctuator(Punctuator::CloseBrace) => {
            let Some(context) = stack.last_mut() else {
                return false;
            };
            if context.brace_depth == 0 {
                stack.pop();
                true
            } else {
                context.brace_depth = context.brace_depth.saturating_sub(1);
                false
            }
        }
        _ => false,
    }
}

fn previous_kind_after_lexed_token(kind: TokenKind) -> Option<TokenKind> {
    match kind {
        TokenKind::TemplateLiteral(TemplateTokenKind::Head | TemplateTokenKind::Middle) => {
            Some(TokenKind::Punctuator(Punctuator::OpenBrace))
        }
        _ => Some(kind),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JumpStatementKind {
    Break,
    Continue,
}

struct TokenCursor<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl<'a> TokenCursor<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, index: 0 }
    }

    fn current(&self) -> &'a Token {
        self.tokens
            .get(self.index)
            .or_else(|| self.tokens.last())
            .expect("parser token stream must contain EOF")
    }

    fn peek(&self, lookahead: usize) -> &'a Token {
        self.tokens
            .get(self.index.saturating_add(lookahead))
            .or_else(|| self.tokens.last())
            .expect("parser token stream must contain EOF")
    }

    fn bump(&mut self) -> &'a Token {
        let token = self.current();
        if token.kind != TokenKind::EndOfFile {
            self.index = self.index.saturating_add(1);
        }
        token
    }

    fn previous_span(&self) -> SourceSpan {
        self.tokens
            .get(self.index.saturating_sub(1))
            .map(Token::span)
            .unwrap_or_else(|| self.current().span())
    }

    fn is_eof(&self) -> bool {
        self.current().kind == TokenKind::EndOfFile
    }

    fn at_punctuator(&self, punctuator: Punctuator) -> bool {
        matches!(self.current().kind, TokenKind::Punctuator(current) if current == punctuator)
    }

    fn consume_punctuator(&mut self, punctuator: Punctuator) -> Option<&'a Token> {
        if self.at_punctuator(punctuator) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn expect_punctuator(&mut self, punctuator: Punctuator) -> Result<&'a Token, ParserError> {
        if self.at_punctuator(punctuator) {
            Ok(self.bump())
        } else {
            syntax_error(self.current(), "expected punctuator")
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<&'a Token, ParserError> {
        if matches!(self.current().kind, TokenKind::Keyword(current) if current == keyword) {
            Ok(self.bump())
        } else {
            syntax_error(self.current(), "expected keyword")
        }
    }

    fn consume_keyword(&mut self, keyword: Keyword) -> Option<&'a Token> {
        if matches!(self.current().kind, TokenKind::Keyword(current) if current == keyword) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn at_contextual_keyword(&self, keyword: ContextualKeyword) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Keyword(Keyword::Contextual(current)) if current == keyword
        )
    }

    fn peek_contextual_keyword(&self, lookahead: usize, keyword: ContextualKeyword) -> bool {
        matches!(
            self.peek(lookahead).kind,
            TokenKind::Keyword(Keyword::Contextual(current)) if current == keyword
        )
    }

    fn expect_contextual_keyword(
        &mut self,
        keyword: ContextualKeyword,
    ) -> Result<&'a Token, ParserError> {
        if self.at_contextual_keyword(keyword) {
            Ok(self.bump())
        } else {
            syntax_error(self.current(), "expected contextual keyword")
        }
    }

    fn expect_eof(&self) -> Result<(), ParserError> {
        if self.is_eof() {
            Ok(())
        } else {
            syntax_error(self.current(), "unexpected token after statement list")
        }
    }

    fn statement_is_terminated(&self) -> bool {
        self.is_eof()
            || self.at_punctuator(Punctuator::CloseBrace)
            || self.at_punctuator(Punctuator::Semicolon)
            || self.current().flags.has_line_terminator_before
    }

    fn current_binary_operator(&self) -> Option<(AstBinaryOperator, u8, bool)> {
        let TokenKind::Punctuator(punctuator) = self.current().kind else {
            return match self.current().kind {
                TokenKind::Keyword(Keyword::In) => Some((AstBinaryOperator::In, 7, false)),
                TokenKind::Keyword(Keyword::Instanceof) => {
                    Some((AstBinaryOperator::Instanceof, 7, false))
                }
                _ => None,
            };
        };
        let op = match punctuator {
            Punctuator::Or => (AstBinaryOperator::LogicalOr, 1, false),
            Punctuator::And => (AstBinaryOperator::LogicalAnd, 2, false),
            Punctuator::Coalesce => (AstBinaryOperator::Coalesce, 2, false),
            Punctuator::BitOr => (AstBinaryOperator::BitOr, 3, false),
            Punctuator::BitXor => (AstBinaryOperator::BitXor, 4, false),
            Punctuator::BitAnd => (AstBinaryOperator::BitAnd, 5, false),
            Punctuator::EqualEqual => (AstBinaryOperator::Equal, 6, false),
            Punctuator::NotEqual => (AstBinaryOperator::NotEqual, 6, false),
            Punctuator::StrictEqual => (AstBinaryOperator::StrictEqual, 6, false),
            Punctuator::StrictNotEqual => (AstBinaryOperator::StrictNotEqual, 6, false),
            Punctuator::LessThan => (AstBinaryOperator::LessThan, 7, false),
            Punctuator::GreaterThan => (AstBinaryOperator::GreaterThan, 7, false),
            Punctuator::LessEqual => (AstBinaryOperator::LessEqual, 7, false),
            Punctuator::GreaterEqual => (AstBinaryOperator::GreaterEqual, 7, false),
            Punctuator::LeftShift => (AstBinaryOperator::LeftShift, 8, false),
            Punctuator::RightShift => (AstBinaryOperator::RightShift, 8, false),
            Punctuator::UnsignedRightShift => (AstBinaryOperator::UnsignedRightShift, 8, false),
            Punctuator::Plus => (AstBinaryOperator::Add, 9, false),
            Punctuator::Minus => (AstBinaryOperator::Subtract, 9, false),
            Punctuator::Multiply => (AstBinaryOperator::Multiply, 10, false),
            Punctuator::Divide => (AstBinaryOperator::Divide, 10, false),
            Punctuator::Mod => (AstBinaryOperator::Modulo, 10, false),
            Punctuator::Pow => (AstBinaryOperator::Pow, 11, true),
            _ => return None,
        };
        Some(op)
    }

    fn current_assignment_operator(&self) -> Option<AssignmentOperator> {
        let TokenKind::Punctuator(punctuator) = self.current().kind else {
            return None;
        };
        match punctuator {
            Punctuator::Equal => Some(AssignmentOperator::Assign),
            Punctuator::PlusEqual => Some(AssignmentOperator::Add),
            Punctuator::MinusEqual => Some(AssignmentOperator::Subtract),
            Punctuator::MultiplyEqual => Some(AssignmentOperator::Multiply),
            Punctuator::DivideEqual => Some(AssignmentOperator::Divide),
            Punctuator::ModEqual => Some(AssignmentOperator::Modulo),
            Punctuator::BitOrEqual => Some(AssignmentOperator::BitOr),
            Punctuator::BitAndEqual => Some(AssignmentOperator::BitAnd),
            Punctuator::BitXorEqual => Some(AssignmentOperator::BitXor),
            Punctuator::LeftShiftEqual => Some(AssignmentOperator::LeftShift),
            Punctuator::RightShiftEqual => Some(AssignmentOperator::RightShift),
            Punctuator::UnsignedRightShiftEqual => Some(AssignmentOperator::UnsignedRightShift),
            Punctuator::PowEqual => Some(AssignmentOperator::Pow),
            Punctuator::CoalesceEqual => Some(AssignmentOperator::Coalesce),
            Punctuator::OrEqual => Some(AssignmentOperator::LogicalOr),
            Punctuator::AndEqual => Some(AssignmentOperator::LogicalAnd),
            _ => None,
        }
    }

    fn current_declaration_keyword(&self) -> Option<DeclarationSyntaxKind> {
        match self.current().kind {
            TokenKind::Keyword(Keyword::Var) => Some(DeclarationSyntaxKind::Var),
            TokenKind::Keyword(Keyword::Const) => Some(DeclarationSyntaxKind::Const),
            TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Let)) => {
                Some(DeclarationSyntaxKind::Let)
            }
            _ => None,
        }
    }
}

fn template_content_span(
    token: &Token,
    kind: TemplateTokenKind,
) -> Result<SourceSpan, ParserError> {
    let (leading, trailing) = match kind {
        TemplateTokenKind::NoSubstitution => (1, 1),
        TemplateTokenKind::Head => (1, 2),
        TemplateTokenKind::Middle => (0, 2),
        TemplateTokenKind::Tail => (0, 1),
    };
    if token.span().unit_len() < leading + trailing {
        return Err(ParserError {
            span: Some(token.span()),
            kind: ParserErrorKind::Syntax("invalid template literal token".into()),
        });
    }
    Ok(SourceSpan::new(
        crate::syntax::source::SourcePosition(token.span().start.0 + leading),
        crate::syntax::source::SourcePosition(token.span().end.0 - trailing),
    ))
}

fn cook_template_text(token: &Token, raw: &str) -> Result<String, ParserError> {
    let mut cooked = String::new();
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if byte != b'\\' {
            cooked.push(char::from(byte));
            continue;
        }
        if index >= bytes.len() {
            return Err(ParserError {
                span: Some(token.span()),
                kind: ParserErrorKind::Syntax("invalid template escape".into()),
            });
        }
        let escaped = bytes[index];
        index += 1;
        match escaped {
            b'\n' => {}
            b'\r' => {
                if bytes.get(index) == Some(&b'\n') {
                    index += 1;
                }
            }
            b'`' => cooked.push('`'),
            b'$' => cooked.push('$'),
            b'\'' => cooked.push('\''),
            b'"' => cooked.push('"'),
            b'\\' => cooked.push('\\'),
            b'b' => cooked.push('\u{0008}'),
            b'f' => cooked.push('\u{000c}'),
            b'n' => cooked.push('\n'),
            b'r' => cooked.push('\r'),
            b't' => cooked.push('\t'),
            b'v' => cooked.push('\u{000b}'),
            b'0' => cooked.push('\0'),
            b'x' => {
                let (unit, next) =
                    parse_fixed_hex_escape(bytes, index, 2).ok_or_else(|| ParserError {
                        span: Some(token.span()),
                        kind: ParserErrorKind::Syntax("invalid template hex escape".into()),
                    })?;
                index = next;
                cooked.push(char::from_u32(unit).ok_or_else(|| ParserError {
                    span: Some(token.span()),
                    kind: ParserErrorKind::Syntax("invalid template hex escape".into()),
                })?);
            }
            b'u' if bytes.get(index) == Some(&b'{') => {
                index += 1;
                let start = index;
                let mut unit = 0_u32;
                while bytes.get(index).is_some_and(|byte| *byte != b'}') {
                    let digit = hex_value(bytes[index]).ok_or_else(|| ParserError {
                        span: Some(token.span()),
                        kind: ParserErrorKind::Syntax("invalid template unicode escape".into()),
                    })?;
                    unit = unit.saturating_mul(16).saturating_add(u32::from(digit));
                    index += 1;
                }
                if index == start || bytes.get(index) != Some(&b'}') {
                    return Err(ParserError {
                        span: Some(token.span()),
                        kind: ParserErrorKind::Syntax("invalid template unicode escape".into()),
                    });
                }
                index += 1;
                cooked.push(char::from_u32(unit).ok_or_else(|| ParserError {
                    span: Some(token.span()),
                    kind: ParserErrorKind::Syntax("invalid template unicode escape".into()),
                })?);
            }
            b'u' => {
                let (unit, next) =
                    parse_fixed_hex_escape(bytes, index, 4).ok_or_else(|| ParserError {
                        span: Some(token.span()),
                        kind: ParserErrorKind::Syntax("invalid template unicode escape".into()),
                    })?;
                index = next;
                cooked.push(char::from_u32(unit).ok_or_else(|| ParserError {
                    span: Some(token.span()),
                    kind: ParserErrorKind::Syntax("invalid template unicode escape".into()),
                })?);
            }
            _ => {
                return Err(ParserError {
                    span: Some(token.span()),
                    kind: ParserErrorKind::Syntax(
                        "unsupported template escape in core parser".into(),
                    ),
                });
            }
        }
    }
    Ok(cooked)
}

fn non_decimal_literal_digits_and_radix(raw: &str) -> Option<(&str, u32)> {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'0') {
        return None;
    }
    let radix = match bytes.get(1) {
        Some(b'x' | b'X') => 16,
        Some(b'b' | b'B') => 2,
        Some(b'o' | b'O') => 8,
        _ => return None,
    };
    Some((&raw[2..], radix))
}

fn parse_non_decimal_number_literal(digits: &str, radix: u32) -> Option<NumberLiteralValue> {
    if digits.is_empty() {
        return None;
    }
    if let Ok(value) = i32::from_str_radix(digits, radix) {
        return Some(NumberLiteralValue::Int32(value));
    }
    if let Ok(value) = u128::from_str_radix(digits, radix) {
        return Some(NumberLiteralValue::DoubleBits((value as f64).to_bits()));
    }

    let mut value = 0.0;
    let radix_value = f64::from(radix);
    for unit in digits.bytes() {
        let digit = u32::from(hex_value(unit)?);
        if digit >= radix {
            return None;
        }
        value = value * radix_value + f64::from(digit);
    }

    Some(NumberLiteralValue::DoubleBits(value.to_bits()))
}

fn parse_fixed_hex_escape(bytes: &[u8], start: usize, count: usize) -> Option<(u32, usize)> {
    let mut unit = 0_u32;
    let mut index = start;
    for _ in 0..count {
        let digit = hex_value(*bytes.get(index)?)?;
        unit = unit.saturating_mul(16).saturating_add(u32::from(digit));
        index += 1;
    }
    Some((unit, index))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn identifier_from_token(token: &Token) -> Option<ParserIdentifier> {
    match token.data {
        TokenData::Identifier { symbol, .. } => Some(symbol),
        _ => None,
    }
}

fn syntax_error<T>(token: &Token, message: &str) -> Result<T, ParserError> {
    Err(ParserError {
        span: Some(token.span()),
        kind: ParserErrorKind::Syntax(message.into()),
    })
}

fn join_spans(start: SourceSpan, end: SourceSpan) -> SourceSpan {
    SourceSpan::new(start.start, end.end)
}

fn expr_span(expr: &Expr) -> SourceSpan {
    match expr {
        Expr::Literal(expr) => expr.span,
        Expr::Name(expr) => expr.span,
        Expr::Unary(expr) => expr.span,
        Expr::Binary(expr) => expr.span,
        Expr::Assignment(expr) => expr.span,
        Expr::Conditional(expr) => expr.span,
        Expr::Call(expr) => expr.span,
        Expr::New(expr) => expr.span,
        Expr::Member(expr) => expr.span,
        Expr::Object(expr) => expr.span,
        Expr::Array(expr) => expr.span,
        Expr::Function(_) => SourceSpan::default(),
        Expr::Class(expr) => expr.span,
        Expr::Template(expr) => expr.span,
        Expr::ImportMeta(span) => *span,
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
    pub implementation_visibility: ParserImplementationVisibility,
    pub source_parse_mode: SourceParseMode,
    pub strict: StrictModePolicy,
    pub module_goal: ModuleGoal,
    pub recovery: ErrorRecovery,
    pub features: ParserFeatureSet,
    pub derived_context: DerivedContextKind,
    pub eval_context: EvalContextKind,
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
            implementation_visibility: ParserImplementationVisibility::Public,
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
            derived_context: DerivedContextKind::None,
            eval_context: if matches!(mode, ParseMode::Eval) {
                EvalContextKind::FunctionEval
            } else {
                EvalContextKind::None
            },
        }
    }
}

/// Component that owns immutable parser mode and grammar tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParserModeTableOwner {
    #[default]
    SyntaxFrontend,
    BuiltinGenerator,
    TestFixture,
}

/// Authority allowed to replace parser mode/static syntax tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParserModeTableMutationAuthority {
    #[default]
    GeneratedSyntaxDataRefresh,
    CrateInitialization,
}

/// Immutable descriptor for a parser mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserModeDescriptor {
    pub mode: ParseMode,
    pub script_mode: ScriptMode,
    pub builtin_mode: BuiltinMode,
    pub source_parse_mode: SourceParseMode,
    pub strict: StrictModePolicy,
    pub module_goal: ModuleGoal,
    pub recovery: ErrorRecovery,
    pub feature_defaults: ParserFeatureSet,
}

/// Immutable descriptor for source parse mode classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceParseModeDescriptor {
    pub mode: SourceParseMode,
    pub is_function: bool,
    pub is_module: bool,
    pub body_kind: FunctionBodyKind,
    pub name_requirement: FunctionNameRequirement,
}

/// Immutable descriptor for one parser feature flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserFeatureDescriptor {
    pub name: &'static str,
    pub default_enabled: bool,
    pub owner: ParserModeTableOwner,
}

/// Read-only parser mode table.
///
/// This table records static syntax configuration only. It does not classify
/// source text, parse, recover, emit diagnostics, or mutate parser state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParserModeTable {
    pub owner: ParserModeTableOwner,
    pub mutation_authority: ParserModeTableMutationAuthority,
    pub parser_modes: &'static [ParserModeDescriptor],
    pub source_modes: &'static [SourceParseModeDescriptor],
    pub features: &'static [ParserFeatureDescriptor],
}

impl ParserModeTable {
    pub const fn parser_modes(self) -> &'static [ParserModeDescriptor] {
        self.parser_modes
    }

    pub const fn source_modes(self) -> &'static [SourceParseModeDescriptor] {
        self.source_modes
    }

    pub const fn features(self) -> &'static [ParserFeatureDescriptor] {
        self.features
    }

    pub fn descriptor_for_mode(self, mode: ParseMode) -> Option<&'static ParserModeDescriptor> {
        self.parser_modes
            .iter()
            .find(|descriptor| descriptor.mode == mode)
    }

    pub fn descriptor_for_source_mode(
        self,
        mode: SourceParseMode,
    ) -> Option<&'static SourceParseModeDescriptor> {
        self.source_modes
            .iter()
            .find(|descriptor| descriptor.mode == mode)
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
    pub allow_html_comments: bool,
    pub allow_annex_b_function_hoisting: bool,
}

/// Parser expression-state switches with source-level grammar meaning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParserGrammarContext {
    pub allows_in: bool,
    pub allows_yield: bool,
    pub allows_await: bool,
    pub inside_call_or_apply: bool,
    pub function_constructor_parameters_end: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionBodyKind {
    StandardBlock,
    ArrowExpression,
    ArrowBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionNameRequirement {
    None,
    Named,
    Unnamed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarationDefaultContext {
    Standard,
    ExportDefault,
    ClassDeclaration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DestructuringKind {
    ToVariables,
    ToLet,
    ToConst,
    ToCatchParameters,
    ToParameters,
    ToExpressions,
}

/// Declaration validation bitset produced by parser scope mutation.
///
/// The parser owns mutation of declaration tables. Later stages should consume
/// the frozen declaration records and early errors rather than rechecking these
/// transient parser results.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeclarationResultFlags {
    pub valid: bool,
    pub duplicate_declaration: bool,
    pub invalid_strict_mode: bool,
    pub invalid_private_static_non_static: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParserState {
    pub config: ParserConfig,
    pub strict: bool,
    pub labels: Vec<LabelFrame>,
    pub grammar: ParserGrammarContext,
    pub scope_depth: u32,
    pub function_depth: u32,
    pub loop_depth: u32,
    pub switch_depth: u32,
    pub class_depth: u32,
    pub private_name_scope_depth: u32,
    pub pending_early_errors: Vec<EarlyError>,
}

impl ParserState {
    fn new(config: ParserConfig) -> Self {
        Self {
            strict: matches!(config.strict, StrictModePolicy::AlwaysStrict),
            config,
            labels: Vec::new(),
            grammar: ParserGrammarContext::default(),
            scope_depth: 0,
            function_depth: 0,
            loop_depth: 0,
            switch_depth: 0,
            class_depth: 0,
            private_name_scope_depth: 0,
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
            class_depth: self.class_depth,
            private_name_scope_depth: self.private_name_scope_depth,
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
    pub class_depth: u32,
    pub private_name_scope_depth: u32,
    pub label_count: u32,
    pub early_error_count: u32,
}

/// Savepoint for speculative parser branches.
///
/// JSC snapshots lexer offset, parser flags, and error state when trying
/// ambiguous grammar forms. This Rust contract names the ownership boundary:
/// only the parser may restore a savepoint, and it must restore into the same
/// source and arena session that created it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserSavePoint {
    pub lexer_cursor: crate::syntax::SourcePosition,
    pub checkpoint: ParserCheckpoint,
    pub grammar: ParserGrammarContext,
    pub phase: FunctionParsePhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionParsePhase {
    Parameters,
    Body,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::source::{SourceOrigin, SourcePosition, SourceProvider, SourceText};
    use std::sync::Arc;

    fn source(text: &str) -> SourceCode {
        let provider = Arc::new(SourceProvider::new(
            SourceOrigin::default(),
            SourceText::Latin1(text.as_bytes().to_vec()),
        ));
        SourceCode::new(
            provider,
            SourceSpan::new(SourcePosition(0), SourcePosition(text.len() as u32)),
        )
    }

    fn parse_first_class_expression(arena: &mut ParserArena, text: &str) -> AstRef<Expr> {
        let source = source(text);
        let parsed = Parser::with_mode(arena, AstBuilder::default(), &source, ParseMode::Program)
            .parse()
            .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => Some(root),
            _ => None,
        }
        .unwrap();
        let scope = arena.scope_node(root).unwrap();
        match arena.statement(scope.statements[0]) {
            Some(Stmt::Declaration(DeclarationStmt { initializers, .. })) => initializers[0],
            _ => None,
        }
        .expect("class declaration must have initializer")
    }

    fn parse_script_scope(arena: &mut ParserArena, text: &str) -> AstRef<ScopeNode> {
        let source = source(text);
        let parsed = Parser::with_mode(arena, AstBuilder::default(), &source, ParseMode::Program)
            .parse()
            .unwrap();
        match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                unreachable!();
            }
        }
    }

    fn declaration_initializer(
        arena: &ParserArena,
        scope: &ScopeNode,
        statement_index: usize,
        initializer_index: usize,
    ) -> AstRef<Expr> {
        let Some(Stmt::Declaration(DeclarationStmt { initializers, .. })) =
            arena.statement(scope.statements[statement_index])
        else {
            panic!("expected declaration statement");
        };
        initializers[initializer_index].expect("expected declaration initializer")
    }

    fn number_literal_value(arena: &ParserArena, expression: AstRef<Expr>) -> NumberLiteralValue {
        let Some(Expr::Literal(LiteralExpr {
            kind: LiteralKind::Number { value },
            ..
        })) = arena.expression(expression)
        else {
            panic!("expected number literal expression");
        };
        *value
    }

    fn class_element_name(arena: &ParserArena, element: &ClassElement) -> String {
        match element.name {
            ClassElementName::Public(name) => arena
                .identifiers()
                .identifier_text(name)
                .unwrap_or("<missing>")
                .to_string(),
            ClassElementName::Private(_) => "#private".into(),
            ClassElementName::Computed(_) => "[computed]".into(),
        }
    }

    fn property_key_name(arena: &ParserArena, key: AstPropertyKey) -> String {
        match key {
            AstPropertyKey::Identifier(name)
            | AstPropertyKey::String(name)
            | AstPropertyKey::Number(name) => arena
                .identifiers()
                .identifier_text(name)
                .unwrap_or("<missing>")
                .to_string(),
            AstPropertyKey::Private(_) => "#private".into(),
            AstPropertyKey::Computed(_) => "[computed]".into(),
        }
    }

    #[test]
    fn parser_builds_arena_ast_for_declarations_calls_and_binary_expressions() {
        let source = source("let answer = add(40, 2 * 1); answer;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();

        assert_eq!(parsed.token_count, 14);
        assert_eq!(scope.kind, ScopeNodeKind::Script);
        assert_eq!(scope.statements.len(), 2);
        assert!(matches!(
            arena.statement(scope.statements[0]),
            Some(Stmt::Declaration(DeclarationStmt {
                kind: DeclarationSyntaxKind::Let,
                bindings,
                ..
            })) if bindings.len() == 1
        ));
        assert!(matches!(
            arena.statement(scope.statements[1]),
            Some(Stmt::Expression(expr))
                if matches!(arena.expression(*expr), Some(Expr::Name(NameExpr {
                    kind: NameKind::Resolve,
                    ..
            })))
        ));
    }

    #[test]
    fn parser_preserves_delete_member_reference_shape() {
        let source = source("let object = {}; return delete object.value;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::Control(ControlStmt {
            kind: ControlKind::Return(Some(value)),
            ..
        })) = arena.statement(scope.statements[1])
        else {
            assert!(matches!(
                arena.statement(scope.statements[1]),
                Some(Stmt::Control(_))
            ));
            return;
        };
        let Some(Expr::Unary(UnaryExpr {
            op: UnaryOperator::Delete,
            argument,
            ..
        })) = arena.expression(*value)
        else {
            assert!(matches!(arena.expression(*value), Some(Expr::Unary(_))));
            return;
        };

        assert!(matches!(
            arena.expression(*argument),
            Some(Expr::Member(MemberExpr {
                member: MemberKind::Dot(_),
                ..
            }))
        ));
    }

    #[test]
    fn syntax_checker_reports_root_shape_without_exposing_ast() {
        let source = source("{ const x = 1 + 2; x; }");
        let mut arena = ParserArena::new();
        let result = Parser::with_mode(
            &mut arena,
            SyntaxChecker::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();

        assert_eq!(result.root_kind, ScopeNodeKind::Script);
        assert_eq!(result.token_count, 11);
        assert_eq!(arena.node_count(), 10);
    }

    #[test]
    fn parser_rejects_missing_binding_identifier() {
        let source = source("let = 1;");
        let mut arena = ParserArena::new();
        let error = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap_err();

        assert!(
            matches!(error.kind, ParserErrorKind::Syntax(message) if message == "expected binding identifier")
        );
    }

    #[test]
    fn parser_classifies_public_class_accessors_and_static_names() {
        let mut arena = ParserArena::new();
        let class = parse_first_class_expression(
            &mut arena,
            "class C { get x() { return 1; } set x(value) { this.value = value; } get [\"computed\"]() { return 8; } [\"method\"]() { return 9; } [\"field\"] = 10; get() { return 2; } set() { return 3; } get; set = 1; static() { return 4; } static = 5; static; static get y() { return 6; } static get() { return 7; } static [\"computedStatic\"]() { return 11; } }",
        );
        let class = match arena.expression(class) {
            Some(Expr::Class(class)) => Some(class),
            _ => None,
        }
        .unwrap();
        let elements: Vec<_> = class
            .elements
            .iter()
            .filter(|element| !element.is_synthesized_default_constructor)
            .map(|element| {
                (
                    class_element_name(&arena, element),
                    element.kind,
                    element.is_static,
                )
            })
            .collect();

        assert_eq!(
            elements,
            vec![
                ("x".into(), ClassElementKind::Getter, false),
                ("x".into(), ClassElementKind::Setter, false),
                ("[computed]".into(), ClassElementKind::Getter, false),
                ("[computed]".into(), ClassElementKind::Method, false),
                ("[computed]".into(), ClassElementKind::Field, false),
                ("get".into(), ClassElementKind::Method, false),
                ("set".into(), ClassElementKind::Method, false),
                ("get".into(), ClassElementKind::Field, false),
                ("set".into(), ClassElementKind::Field, false),
                ("static".into(), ClassElementKind::Method, false),
                ("static".into(), ClassElementKind::Field, false),
                ("static".into(), ClassElementKind::Field, false),
                ("y".into(), ClassElementKind::Getter, true),
                ("get".into(), ClassElementKind::Method, true),
                ("[computed]".into(), ClassElementKind::Method, true),
            ]
        );
    }

    #[test]
    fn parser_validates_class_accessor_arity_and_strict_metadata() {
        let mut arena = ParserArena::new();
        let class = parse_first_class_expression(
            &mut arena,
            "class C { get x() { return 1; } set x(value) { this.value = value; } }",
        );
        let class = match arena.expression(class) {
            Some(Expr::Class(class)) => Some(class),
            _ => None,
        }
        .unwrap();
        for element in class.elements.iter().filter(|element| {
            matches!(
                element.kind,
                ClassElementKind::Getter | ClassElementKind::Setter
            )
        }) {
            let metadata = arena.function_metadata(element.metadata.unwrap()).unwrap();
            assert!(metadata.strict);
        }

        let mut arena = ParserArena::new();
        let error = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source("class C { get x(value) { return value; } }"),
            ParseMode::Program,
        )
        .parse()
        .unwrap_err();
        assert!(
            matches!(error.kind, ParserErrorKind::Syntax(message) if message == "getter must not have parameters")
        );

        let mut arena = ParserArena::new();
        let error = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source("class C { set x() { } }"),
            ParseMode::Program,
        )
        .parse()
        .unwrap_err();
        assert!(
            matches!(error.kind, ParserErrorKind::Syntax(message) if message == "setter must have exactly one parameter")
        );
    }

    #[test]
    fn parser_builds_if_else_and_while_statements() {
        let source = source("let i = 0; while (i < 3) { if (i === 1) i = i + 2; else i = i + 1; }");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();

        assert_eq!(scope.statements.len(), 2);
        let Some(Stmt::While(loop_stmt)) = arena.statement(scope.statements[1]) else {
            assert!(matches!(
                arena.statement(scope.statements[1]),
                Some(Stmt::While(_))
            ));
            return;
        };
        let Some(Expr::Binary(BinaryExpr {
            op: AstBinaryOperator::LessThan,
            ..
        })) = arena.expression(loop_stmt.condition)
        else {
            assert!(matches!(
                arena.expression(loop_stmt.condition),
                Some(Expr::Binary(_))
            ));
            return;
        };
        let Some(Stmt::Block(block)) = arena.statement(loop_stmt.body) else {
            assert!(matches!(
                arena.statement(loop_stmt.body),
                Some(Stmt::Block(_))
            ));
            return;
        };
        let loop_scope = arena.scope_node(block.scope).unwrap();
        assert!(matches!(
            arena.statement(loop_scope.statements[0]),
            Some(Stmt::If(IfStmt {
                alternate: Some(_),
                ..
            }))
        ));
    }

    #[test]
    fn parser_builds_do_while_statement() {
        let source = source("do { value = value + 1; } while (value < 3);");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let AstRoot::Script(root) = parsed.root else {
            panic!("expected script root");
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::DoWhile(statement)) = arena.statement(scope.statements[0]) else {
            panic!("expected do-while statement");
        };

        assert!(matches!(
            arena.expression(statement.condition),
            Some(Expr::Binary(BinaryExpr {
                op: AstBinaryOperator::LessThan,
                ..
            }))
        ));
        assert!(matches!(
            arena.statement(statement.body),
            Some(Stmt::Block(_))
        ));
    }

    #[test]
    fn parser_builds_switch_statement() {
        let source = source("switch (value) { case 1: value = 2; break; default: value = 3; }");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let AstRoot::Script(root) = parsed.root else {
            panic!("expected script root");
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::Switch(statement)) = arena.statement(scope.statements[0]) else {
            panic!("expected switch statement");
        };

        assert_eq!(statement.cases.len(), 2);
        assert!(statement.cases[0].test.is_some());
        assert!(statement.cases[1].test.is_none());
        assert_eq!(statement.cases[0].statements.len(), 2);
    }

    #[test]
    fn parser_rejects_duplicate_switch_default() {
        let source = source("switch (value) { default: break; default: break; }");
        let mut arena = ParserArena::new();
        let error = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap_err();

        assert!(
            matches!(error.kind, ParserErrorKind::Syntax(message) if message == "duplicate default clause in switch")
        );
    }

    #[test]
    fn parser_builds_for_statement_with_declaration_initializer() {
        let source = source("let total = 0; for (let i = 0; i < 3; i = i + 1) total = total + i;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();

        let Some(Stmt::For(statement)) = arena.statement(scope.statements[1]) else {
            assert!(matches!(
                arena.statement(scope.statements[1]),
                Some(Stmt::For(_))
            ));
            return;
        };
        assert!(matches!(
            &statement.init,
            Some(ForInit::Declaration(DeclarationStmt {
                kind: DeclarationSyntaxKind::Let,
                bindings,
                ..
            })) if bindings.len() == 1
        ));
        assert!(statement.condition.is_some());
        assert!(statement.update.is_some());
    }

    #[test]
    fn parser_builds_function_declaration_metadata() {
        let source = source("function add(a, b) { return a + b; } return add(1, 2);");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();

        assert_eq!(scope.statements.len(), 2);
        let Some(Stmt::FunctionDeclaration(declaration)) = arena.statement(scope.statements[0])
        else {
            assert!(matches!(
                arena.statement(scope.statements[0]),
                Some(Stmt::FunctionDeclaration(_))
            ));
            return;
        };
        let metadata = arena.function_metadata(declaration.metadata).unwrap();
        let body = arena.scope_node(metadata.body).unwrap();
        assert_eq!(metadata.parameter_count, 2);
        assert_eq!(metadata.parameters.len(), 2);
        assert_eq!(body.kind, ScopeNodeKind::Function);
        assert!(matches!(
            arena.statement(body.statements[0]),
            Some(Stmt::Control(ControlStmt {
                kind: ControlKind::Return(Some(_)),
                ..
            }))
        ));
    }

    #[test]
    fn parser_builds_function_expression_metadata() {
        let source = source("let add = function named(a, b) { return a + b; };");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::Declaration(declaration)) = arena.statement(scope.statements[0]) else {
            assert!(matches!(
                arena.statement(scope.statements[0]),
                Some(Stmt::Declaration(_))
            ));
            return;
        };
        let Some(initializer) = declaration.initializers[0] else {
            assert!(declaration.initializers[0].is_some());
            return;
        };
        let Some(Expr::Function(metadata)) = arena.expression(initializer) else {
            assert!(matches!(
                arena.expression(initializer),
                Some(Expr::Function(_))
            ));
            return;
        };
        let metadata = arena.function_metadata(*metadata).unwrap();
        assert_eq!(metadata.parameter_count, 2);
        assert!(metadata.name.is_some());
        assert_eq!(
            arena.scope_node(metadata.body).unwrap().kind,
            ScopeNodeKind::Function
        );
    }

    #[test]
    fn parser_preserves_string_literal_text() {
        let source = source("let value = \"line\\ntext\";");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::Declaration(declaration)) = arena.statement(scope.statements[0]) else {
            assert!(matches!(
                arena.statement(scope.statements[0]),
                Some(Stmt::Declaration(_))
            ));
            return;
        };
        let Some(initializer) = declaration.initializers[0] else {
            assert!(declaration.initializers[0].is_some());
            return;
        };
        let Some(Expr::Literal(LiteralExpr {
            kind: LiteralKind::String { text },
            ..
        })) = arena.expression(initializer)
        else {
            assert!(matches!(
                arena.expression(initializer),
                Some(Expr::Literal(_))
            ));
            return;
        };

        assert_eq!(
            arena.identifiers().identifier_text(*text),
            Some("line\ntext")
        );
    }

    #[test]
    fn parser_builds_template_literal_with_substitution() {
        let mut arena = ParserArena::new();
        let expression = parse_first_class_expression(&mut arena, "let value = `a${answer}b`;");
        let Some(Expr::Template(template)) = arena.expression(expression) else {
            assert!(matches!(
                arena.expression(expression),
                Some(Expr::Template(_))
            ));
            return;
        };

        assert_eq!(template.tag, None);
        assert_eq!(template.quasis.len(), 2);
        assert_eq!(template.expressions.len(), 1);
        assert_eq!(
            arena
                .identifiers()
                .identifier_text(template.quasis[0].cooked.unwrap()),
            Some("a")
        );
        assert_eq!(
            arena
                .identifiers()
                .identifier_text(template.quasis[1].cooked.unwrap()),
            Some("b")
        );
        assert!(matches!(
            arena.expression(template.expressions[0]),
            Some(Expr::Name(NameExpr {
                kind: NameKind::Resolve,
                ..
            }))
        ));
    }

    #[test]
    fn parser_preserves_cooked_template_escape_text() {
        let mut arena = ParserArena::new();
        let expression = parse_first_class_expression(&mut arena, "let value = `line\\ntext`;");
        let Some(Expr::Template(template)) = arena.expression(expression) else {
            assert!(matches!(
                arena.expression(expression),
                Some(Expr::Template(_))
            ));
            return;
        };

        assert_eq!(template.expressions.len(), 0);
        assert_eq!(
            arena
                .identifiers()
                .identifier_text(template.quasis[0].cooked.unwrap()),
            Some("line\ntext")
        );
        assert_eq!(
            arena.identifiers().identifier_text(template.quasis[0].raw),
            Some("line\\ntext")
        );
    }

    #[test]
    fn parser_keeps_escaped_template_substitution_marker_as_text() {
        let mut arena = ParserArena::new();
        let expression =
            parse_first_class_expression(&mut arena, "let value = `\\${notExpression}`;");
        let Some(Expr::Template(template)) = arena.expression(expression) else {
            assert!(matches!(
                arena.expression(expression),
                Some(Expr::Template(_))
            ));
            return;
        };

        assert_eq!(template.expressions.len(), 0);
        assert_eq!(
            arena
                .identifiers()
                .identifier_text(template.quasis[0].cooked.unwrap()),
            Some("${notExpression}")
        );
        assert_eq!(
            arena.identifiers().identifier_text(template.quasis[0].raw),
            Some("\\${notExpression}")
        );
    }

    #[test]
    fn parser_rejects_tagged_template_literals() {
        let source = source("tag`value`;");
        let mut arena = ParserArena::new();
        let error = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap_err();

        assert!(
            matches!(error.kind, ParserErrorKind::Syntax(message) if message == "tagged template literals are not supported")
        );
    }

    #[test]
    fn parser_rejects_invalid_untagged_template_escape() {
        let source = source("let value = `bad\\8`;");
        let mut arena = ParserArena::new();
        let error = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap_err();

        assert!(
            matches!(error.kind, ParserErrorKind::Syntax(message) if message == "unsupported template escape in core parser")
        );
    }

    #[test]
    fn parser_preserves_double_literal_bits() {
        let source = source("let value = 40.5;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::Declaration(declaration)) = arena.statement(scope.statements[0]) else {
            assert!(matches!(
                arena.statement(scope.statements[0]),
                Some(Stmt::Declaration(_))
            ));
            return;
        };
        let Some(initializer) = declaration.initializers[0] else {
            assert!(declaration.initializers[0].is_some());
            return;
        };
        let Some(Expr::Literal(LiteralExpr {
            kind:
                LiteralKind::Number {
                    value: NumberLiteralValue::DoubleBits(bits),
                },
            ..
        })) = arena.expression(initializer)
        else {
            assert!(matches!(
                arena.expression(initializer),
                Some(Expr::Literal(_))
            ));
            return;
        };

        assert_eq!(f64::from_bits(*bits), 40.5);
    }

    #[test]
    fn parser_parses_non_decimal_numeric_literals() {
        let mut arena = ParserArena::new();
        let root = parse_script_scope(
            &mut arena,
            "let hex = 0xD008; \
             let wide = 0xffffffff; \
             let binary = 0b1010; \
             let octal = 0o755; \
             let upper_hex = 0XD008; \
             let upper_binary = 0B1010; \
             let upper_octal = 0O755;",
        );
        let scope = arena.scope_node(root).unwrap();
        let expected = [
            NumberLiteralValue::Int32(0xD008),
            NumberLiteralValue::DoubleBits(f64::from(u32::MAX).to_bits()),
            NumberLiteralValue::Int32(0b1010),
            NumberLiteralValue::Int32(0o755),
            NumberLiteralValue::Int32(0xD008),
            NumberLiteralValue::Int32(0b1010),
            NumberLiteralValue::Int32(0o755),
        ];

        assert_eq!(scope.statements.len(), expected.len());
        for (index, expected_value) in expected.into_iter().enumerate() {
            let initializer = declaration_initializer(&arena, scope, index, 0);
            assert_eq!(number_literal_value(&arena, initializer), expected_value);
        }
    }

    #[test]
    fn parser_allows_trailing_commas_in_call_and_new_argument_lists() {
        let mut arena = ParserArena::new();
        let root = parse_script_scope(
            &mut arena,
            "let call = Color.add(a, b, ); let constructed = new Color(a, b, );",
        );
        let scope = arena.scope_node(root).unwrap();

        let call_initializer = declaration_initializer(&arena, scope, 0, 0);
        let Some(Expr::Call(CallExpr { arguments, .. })) = arena.expression(call_initializer)
        else {
            panic!("expected call expression");
        };
        assert_eq!(arguments.len(), 2);

        let new_initializer = declaration_initializer(&arena, scope, 1, 0);
        let Some(Expr::New(NewExpr { arguments, .. })) = arena.expression(new_initializer) else {
            panic!("expected new expression");
        };
        assert_eq!(arguments.len(), 2);
    }

    #[test]
    fn parser_builds_object_literal_and_member_assignment() {
        let source = source(
            "let object = { x: 40, \"y\": 2, 3: 4, [\"z\"]: 5, get total() { return 1; }, set total(value) { this.value = value; } }; object.x = object.x + object.y;",
        );
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();

        let Some(Stmt::Declaration(declaration)) = arena.statement(scope.statements[0]) else {
            assert!(matches!(
                arena.statement(scope.statements[0]),
                Some(Stmt::Declaration(_))
            ));
            return;
        };
        let Some(initializer) = declaration.initializers[0] else {
            assert!(declaration.initializers[0].is_some());
            return;
        };
        let Some(Expr::Object(ObjectLiteralExpr { properties, .. })) =
            arena.expression(initializer)
        else {
            assert!(matches!(
                arena.expression(initializer),
                Some(Expr::Object(_))
            ));
            return;
        };
        let entries = properties
            .iter()
            .map(|property| (property_key_name(&arena, property.key), property.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            vec![
                ("x".into(), ObjectLiteralPropertyKind::Data),
                ("y".into(), ObjectLiteralPropertyKind::Data),
                ("3".into(), ObjectLiteralPropertyKind::Data),
                ("[computed]".into(), ObjectLiteralPropertyKind::Data),
                ("total".into(), ObjectLiteralPropertyKind::Getter),
                ("total".into(), ObjectLiteralPropertyKind::Setter),
            ]
        );
        assert!(matches!(
            arena.statement(scope.statements[1]),
            Some(Stmt::Expression(expr))
                if matches!(arena.expression(*expr), Some(Expr::Assignment(_)))
        ));
    }

    #[test]
    fn parser_builds_array_literal_and_index_member_assignment() {
        let source = source("let array = [1, , \"x\"]; array[2] = \"y\";");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();
        let Some(Stmt::Declaration(declaration)) = arena.statement(scope.statements[0]) else {
            assert!(matches!(
                arena.statement(scope.statements[0]),
                Some(Stmt::Declaration(_))
            ));
            return;
        };
        let Some(initializer) = declaration.initializers[0] else {
            assert!(declaration.initializers[0].is_some());
            return;
        };
        assert!(matches!(
            arena.expression(initializer),
            Some(Expr::Array(ArrayLiteralExpr { elements, .. }))
                if elements.len() == 3
                    && matches!(elements[1], ArrayLiteralElement::Elision(_))
        ));
        assert!(matches!(
            arena.statement(scope.statements[1]),
            Some(Stmt::Expression(expr))
                if matches!(
                    arena.expression(*expr),
                    Some(Expr::Assignment(AssignmentExpr { target, .. }))
                        if matches!(
                            arena.pattern(*target),
                            Some(Pattern::AssignmentTarget(target_expr))
                                if matches!(
                                    arena.expression(*target_expr),
                                    Some(Expr::Member(MemberExpr {
                                        member: MemberKind::Bracket(_),
                                        ..
                                    }))
                                )
                        )
                )
        ));
    }

    #[test]
    fn parser_builds_update_compound_conditional_and_loose_equality_expressions() {
        let source = source(
            "let x = 0; ++x; x--; object.value += 1; object[key] >>>= 1; return x == 1 ? x != 2 : x === 3;",
        );
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();

        assert!(matches!(
            arena.statement(scope.statements[1]),
            Some(Stmt::Expression(expr))
                if matches!(
                    arena.expression(*expr),
                    Some(Expr::Unary(UnaryExpr {
                        op: UnaryOperator::PreIncrement,
                        ..
                    }))
                )
        ));
        assert!(matches!(
            arena.statement(scope.statements[2]),
            Some(Stmt::Expression(expr))
                if matches!(
                    arena.expression(*expr),
                    Some(Expr::Unary(UnaryExpr {
                        op: UnaryOperator::PostDecrement,
                        ..
                    }))
                )
        ));
        assert!(matches!(
            arena.statement(scope.statements[3]),
            Some(Stmt::Expression(expr))
                if matches!(
                    arena.expression(*expr),
                    Some(Expr::Assignment(AssignmentExpr {
                        op: AssignmentOperator::Add,
                        target,
                        ..
                    }))
                        if matches!(
                            arena.pattern(*target),
                            Some(Pattern::AssignmentTarget(target_expr))
                                if matches!(
                                    arena.expression(*target_expr),
                                    Some(Expr::Member(MemberExpr {
                                        member: MemberKind::Dot(_),
                                        ..
                                    }))
                                )
                        )
                )
        ));
        assert!(matches!(
            arena.statement(scope.statements[4]),
            Some(Stmt::Expression(expr))
                if matches!(
                    arena.expression(*expr),
                    Some(Expr::Assignment(AssignmentExpr {
                        op: AssignmentOperator::UnsignedRightShift,
                        target,
                        ..
                    }))
                        if matches!(
                            arena.pattern(*target),
                            Some(Pattern::AssignmentTarget(target_expr))
                                if matches!(
                                    arena.expression(*target_expr),
                                    Some(Expr::Member(MemberExpr {
                                        member: MemberKind::Bracket(_),
                                        ..
                                    }))
                                )
                        )
                )
        ));
        let Some(Stmt::Control(ControlStmt {
            kind: ControlKind::Return(Some(value)),
            ..
        })) = arena.statement(scope.statements[5])
        else {
            assert!(matches!(
                arena.statement(scope.statements[5]),
                Some(Stmt::Control(_))
            ));
            return;
        };
        let Some(Expr::Conditional(ConditionalExpr {
            test,
            consequent,
            alternate,
            ..
        })) = arena.expression(*value)
        else {
            assert!(matches!(
                arena.expression(*value),
                Some(Expr::Conditional(_))
            ));
            return;
        };

        assert!(matches!(
            arena.expression(*test),
            Some(Expr::Binary(BinaryExpr {
                op: AstBinaryOperator::Equal,
                ..
            }))
        ));
        assert!(matches!(
            arena.expression(*consequent),
            Some(Expr::Binary(BinaryExpr {
                op: AstBinaryOperator::NotEqual,
                ..
            }))
        ));
        assert!(matches!(
            arena.expression(*alternate),
            Some(Expr::Binary(BinaryExpr {
                op: AstBinaryOperator::StrictEqual,
                ..
            }))
        ));
    }

    #[test]
    fn parser_builds_all_update_operator_forms() {
        let source = source("let x = 0; ++x; --x; x++; x--;");
        let mut arena = ParserArena::new();
        let parsed = Parser::with_mode(
            &mut arena,
            AstBuilder::default(),
            &source,
            ParseMode::Program,
        )
        .parse()
        .unwrap();
        let root = match parsed.root {
            AstRoot::Script(root) => root,
            other => {
                assert!(matches!(other, AstRoot::Script(_)));
                return;
            }
        };
        let scope = arena.scope_node(root).unwrap();
        let expected = [
            UnaryOperator::PreIncrement,
            UnaryOperator::PreDecrement,
            UnaryOperator::PostIncrement,
            UnaryOperator::PostDecrement,
        ];

        for (index, expected_op) in expected.iter().enumerate() {
            assert!(matches!(
                arena.statement(scope.statements[index + 1]),
                Some(Stmt::Expression(expr))
                    if matches!(
                        arena.expression(*expr),
                        Some(Expr::Unary(UnaryExpr { op, .. })) if op == expected_op
                    )
            ));
        }
    }
}
