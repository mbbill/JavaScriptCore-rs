use std::marker::PhantomData;

use crate::syntax::arena::IdentifierArena;
use crate::syntax::source::{SourceCode, SourcePosition, SourceSpan};
use crate::syntax::token::{LexicalErrorKind, Token};

/// Encoding-specialized lexer cursor.
///
/// Public APIs remain lifetime-checked. Future optimized cursor walking may use
/// unsafe internally, confined to this module and documented around source
/// storage ownership and bounds invariants. The type names the state JSC's
/// lexer preserves across parser lookahead, regexp rescans, template literal
/// rescans, and function reparsing; it does not implement tokenization.
#[derive(Debug)]
pub struct Lexer<'src, 'arena, E> {
    source: &'src SourceCode,
    identifiers: &'arena IdentifierArena,
    cursor: SourcePosition,
    line_number: u32,
    line_start: SourcePosition,
    state: LexerState,
    _encoding: PhantomData<E>,
}

impl<'src, 'arena, E> Lexer<'src, 'arena, E> {
    pub fn new(source: &'src SourceCode, identifiers: &'arena IdentifierArena) -> Self {
        Self {
            source,
            identifiers,
            cursor: source.range().start,
            line_number: source.first_line(),
            line_start: source.range().start,
            state: LexerState::default(),
            _encoding: PhantomData,
        }
    }

    pub fn source(&self) -> &'src SourceCode {
        self.source
    }

    pub fn identifiers(&self) -> &'arena IdentifierArena {
        self.identifiers
    }

    pub fn snapshot(&self) -> LexerSnapshot {
        LexerSnapshot {
            cursor: self.cursor,
            line_number: self.line_number,
            line_start: self.line_start,
            state: self.state,
        }
    }

    pub fn restore(&mut self, snapshot: LexerSnapshot) {
        self.cursor = snapshot.cursor;
        self.line_number = snapshot.line_number;
        self.line_start = snapshot.line_start;
        self.state = snapshot.state;
    }

    pub fn state(&self) -> LexerState {
        self.state
    }

    /// Tokenization boundary for the future lexer implementation.
    ///
    /// Returning `Deferred` is a deliberate design-skeleton contract: callers
    /// may model parser state and error boundaries without a fake EOF-only
    /// tokenizer.
    pub fn next_token(&mut self, request: LexRequest) -> LexResult<Token> {
        self.state.goal = request.goal;
        LexResult::Deferred(LexDeferred {
            phase: LexPhase::Token,
            cursor: self.cursor,
            request,
        })
    }

    pub fn regexp_literal(&mut self, context: RegExpLexContext) -> LexResult<Token> {
        self.state.goal = LexGoal::RegExp;
        LexResult::Deferred(LexDeferred {
            phase: LexPhase::RegExpLiteral(context),
            cursor: self.cursor,
            request: LexRequest::for_goal(LexGoal::RegExp),
        })
    }

    pub fn template_literal(&mut self, context: TemplateLexContext) -> LexResult<Token> {
        self.state.goal = LexGoal::TemplateTail;
        LexResult::Deferred(LexDeferred {
            phase: LexPhase::TemplateLiteral(context),
            cursor: self.cursor,
            request: LexRequest::for_goal(LexGoal::TemplateTail),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexerSnapshot {
    pub cursor: SourcePosition,
    pub line_number: u32,
    pub line_start: SourcePosition,
    pub state: LexerState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexerState {
    pub goal: LexGoal,
    pub flags: LexerFlags,
    pub has_line_terminator_before_token: bool,
    pub at_line_start: bool,
    pub is_reparsing_function: bool,
}

impl Default for LexerState {
    fn default() -> Self {
        Self {
            goal: LexGoal::Div,
            flags: LexerFlags::default(),
            has_line_terminator_before_token: false,
            at_line_start: true,
            is_reparsing_function: false,
        }
    }
}

/// ECMAScript lexical goal symbol requested by the parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexGoal {
    Div,
    RegExp,
    TemplateTail,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LexerFlags {
    pub ignore_reserved_words: bool,
    pub build_strings: bool,
    pub build_keywords: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexRequest {
    pub goal: LexGoal,
    pub strict: LexStrictness,
    pub keyword_policy: KeywordPolicy,
    pub allow_html_comment_tokens: bool,
}

impl LexRequest {
    pub fn for_goal(goal: LexGoal) -> Self {
        Self {
            goal,
            strict: LexStrictness::Sloppy,
            keyword_policy: KeywordPolicy::Classify,
            allow_html_comment_tokens: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexStrictness {
    Sloppy,
    Strict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeywordPolicy {
    Classify,
    IdentifierExpected,
    IgnoreReservedWords,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegExpLexContext {
    /// Prefix supplied by parser productions such as `/=` ambiguity handling.
    pub pattern_prefix: Option<char>,
    pub skip_syntax_check: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemplateLexContext {
    pub raw_strings: RawStringMode,
    pub expression_depth: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawStringMode {
    Build,
    Skip,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LexResult<T> {
    Ready(T),
    Deferred(LexDeferred),
    Error(LexerError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexDeferred {
    pub phase: LexPhase,
    pub cursor: SourcePosition,
    pub request: LexRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexPhase {
    Token,
    RegExpLiteral(RegExpLexContext),
    TemplateLiteral(TemplateLexContext),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexerError {
    pub span: SourceSpan,
    pub kind: LexicalErrorKind,
    pub message: String,
}
