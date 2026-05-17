use crate::syntax::arena::ParserIdentifier;
use crate::syntax::source::{LineColumn, SourcePosition, SourceSpan};

/// Parser token with lifetime-independent payload handles.
///
/// Tokens carry spans into source storage and parser-arena identifier handles.
/// They must not become a long-lived runtime name representation. The shape is
/// intentionally close to JSC's `JSToken`: compact kind, source location, and a
/// payload union modeled as Rust enum variants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub location: TokenLocation,
    pub data: TokenData,
    pub flags: TokenFlags,
}

impl Token {
    pub fn end_of_file(span: SourceSpan) -> Self {
        Self {
            kind: TokenKind::EndOfFile,
            location: TokenLocation::from_span(span),
            data: TokenData::None,
            flags: TokenFlags::default(),
        }
    }

    pub fn span(&self) -> SourceSpan {
        self.location.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    EndOfFile,
    Identifier(IdentifierTokenKind),
    Keyword(Keyword),
    NumericLiteral(NumericLiteralKind),
    StringLiteral,
    TemplateLiteral(TemplateTokenKind),
    RegExpLiteral,
    Punctuator(Punctuator),
    Error(LexicalErrorKind),
}

/// Identifier-class tokens whose validity depends on parser context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierTokenKind {
    Ordinary,
    PrivateName,
    EscapedKeyword,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Keyword {
    Null,
    True,
    False,
    Break,
    Case,
    Default,
    For,
    New,
    Var,
    Const,
    Continue,
    Function,
    Return,
    If,
    This,
    Do,
    While,
    Switch,
    With,
    Throw,
    Try,
    Catch,
    Finally,
    Debugger,
    Else,
    Import,
    Export,
    Class,
    Extends,
    Super,
    Typeof,
    Void,
    Delete,
    Instanceof,
    In,
    Reserved,
    ReservedIfStrict,
    Contextual(ContextualKeyword),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextualKeyword {
    Let,
    Yield,
    Await,
    As,
    From,
    Of,
    Static,
    Get,
    Set,
    Async,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericLiteralKind {
    Integer,
    Double,
    BigInt { radix: NumericRadix },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericRadix {
    Binary,
    Octal,
    Decimal,
    Hex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemplateTokenKind {
    Head,
    Middle,
    Tail,
    NoSubstitution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Punctuator {
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    Comma,
    Question,
    Backquote,
    Semicolon,
    Colon,
    Dot,
    Equal,
    PlusEqual,
    MinusEqual,
    MultiplyEqual,
    DivideEqual,
    LeftShiftEqual,
    RightShiftEqual,
    UnsignedRightShiftEqual,
    ModEqual,
    PowEqual,
    BitAndEqual,
    BitXorEqual,
    BitOrEqual,
    CoalesceEqual,
    OrEqual,
    AndEqual,
    DotDotDot,
    ArrowFunction,
    QuestionDot,
    PlusPlus,
    MinusMinus,
    Exclamation,
    Tilde,
    Coalesce,
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    EqualEqual,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    Plus,
    Minus,
    Multiply,
    Divide,
    Mod,
    Pow,
}

impl Punctuator {
    pub fn binary_operator(self, allow_in: bool) -> Option<BinaryOperator> {
        let op = match self {
            Self::Coalesce => BinaryOperator::Coalesce,
            Self::Or => BinaryOperator::LogicalOr,
            Self::And => BinaryOperator::LogicalAnd,
            Self::BitOr => BinaryOperator::BitOr,
            Self::BitXor => BinaryOperator::BitXor,
            Self::BitAnd => BinaryOperator::BitAnd,
            Self::EqualEqual => BinaryOperator::Equal,
            Self::NotEqual => BinaryOperator::NotEqual,
            Self::StrictEqual => BinaryOperator::StrictEqual,
            Self::StrictNotEqual => BinaryOperator::StrictNotEqual,
            Self::LessThan => BinaryOperator::LessThan,
            Self::GreaterThan => BinaryOperator::GreaterThan,
            Self::LessEqual => BinaryOperator::LessEqual,
            Self::GreaterEqual => BinaryOperator::GreaterEqual,
            Self::LeftShift => BinaryOperator::LeftShift,
            Self::RightShift => BinaryOperator::RightShift,
            Self::UnsignedRightShift => BinaryOperator::UnsignedRightShift,
            Self::Plus => BinaryOperator::Add,
            Self::Minus => BinaryOperator::Subtract,
            Self::Multiply => BinaryOperator::Multiply,
            Self::Divide => BinaryOperator::Divide,
            Self::Mod => BinaryOperator::Modulo,
            Self::Pow => BinaryOperator::Pow,
            _ => return None,
        };
        op.filter_in(allow_in)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    Coalesce,
    LogicalOr,
    LogicalAnd,
    BitOr,
    BitXor,
    BitAnd,
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    Instanceof,
    In,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Pow,
}

impl BinaryOperator {
    fn filter_in(self, allow_in: bool) -> Option<Self> {
        if matches!(self, Self::In) && !allow_in {
            None
        } else {
            Some(self)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenData {
    None,
    Identifier {
        symbol: ParserIdentifier,
        escaped: bool,
    },
    String {
        cooked: ParserIdentifier,
        raw: Option<ParserIdentifier>,
    },
    Numeric {
        raw: ParserIdentifier,
    },
    Template {
        cooked: Option<ParserIdentifier>,
        raw: ParserIdentifier,
        is_tail: bool,
    },
    RegExp {
        pattern: ParserIdentifier,
        flags: ParserIdentifier,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenFlags {
    pub has_line_terminator_before: bool,
    pub begins_at_line_start: bool,
    pub contains_escape: bool,
    pub unterminated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenLocation {
    pub span: SourceSpan,
    pub line: u32,
    pub line_start: SourcePosition,
    pub start: LineColumn,
    pub end: LineColumn,
}

impl TokenLocation {
    pub fn from_span(span: SourceSpan) -> Self {
        Self {
            span,
            line: 0,
            line_start: span.start,
            start: LineColumn::default(),
            end: LineColumn::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexicalErrorKind {
    InvalidCharacter,
    UnterminatedIdentifierEscape,
    InvalidIdentifierEscape,
    UnterminatedIdentifierUnicodeEscape,
    InvalidIdentifierUnicodeEscape,
    UnterminatedMultilineComment,
    UnterminatedNumericLiteral,
    UnterminatedOctalNumber,
    InvalidNumericLiteral,
    UnterminatedStringLiteral,
    InvalidStringLiteral,
    InvalidPrivateName,
    UnterminatedHexNumber,
    UnterminatedBinaryNumber,
    UnterminatedTemplateLiteral,
    UnterminatedRegExpLiteral,
    InvalidTemplateLiteral,
    InvalidUnicodeEncoding,
}
