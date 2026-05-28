use std::marker::PhantomData;

use crate::syntax::arena::{IdentifierArena, IdentifierSource};
use crate::syntax::source::{SourceCode, SourcePosition, SourceSpan};
use crate::syntax::token::{
    ContextualKeyword, IdentifierTokenKind, Keyword, LexicalErrorKind, NumericLiteralKind,
    NumericRadix, Punctuator, TemplateTokenKind, Token, TokenData, TokenFlags, TokenKind,
    TokenLocation,
};

/// Encoding-specialized lexer cursor.
#[derive(Debug)]
pub struct Lexer<'src, 'arena, E> {
    source: &'src SourceCode,
    identifiers: &'arena mut IdentifierArena,
    cursor: SourcePosition,
    line_number: u32,
    line_start: SourcePosition,
    column: u32,
    state: LexerState,
    _encoding: PhantomData<E>,
}

impl<'src, 'arena, E> Lexer<'src, 'arena, E> {
    pub fn new(source: &'src SourceCode, identifiers: &'arena mut IdentifierArena) -> Self {
        Self {
            source,
            identifiers,
            cursor: source.range().start,
            line_number: source.first_line(),
            line_start: source.range().start,
            column: source.start_column(),
            state: LexerState::default(),
            _encoding: PhantomData,
        }
    }

    pub fn source(&self) -> &'src SourceCode {
        self.source
    }

    pub fn identifiers(&self) -> &IdentifierArena {
        self.identifiers
    }

    pub fn snapshot(&self) -> LexerSnapshot {
        LexerSnapshot {
            cursor: self.cursor,
            line_number: self.line_number,
            line_start: self.line_start,
            column: self.column,
            state: self.state,
        }
    }

    pub fn restore(&mut self, snapshot: LexerSnapshot) {
        self.cursor = snapshot.cursor;
        self.line_number = snapshot.line_number;
        self.line_start = snapshot.line_start;
        self.column = snapshot.column;
        self.state = snapshot.state;
    }

    pub fn state(&self) -> LexerState {
        self.state
    }

    pub fn next_token(&mut self, request: LexRequest) -> LexResult<Token> {
        self.state.goal = request.goal;
        self.skip_trivia(request.allow_html_comment_tokens);
        let start = self.cursor;
        let start_line = self.line_number;
        let start_line_start = self.line_start;
        let start_column = self.column;
        let begins_at_line_start = self.state.at_line_start;

        if self.is_eof() {
            return LexResult::Ready(Token::end_of_file(SourceSpan::at(start)));
        }

        if request.goal == LexGoal::RegExp && self.peek_unit() == Some(b'/') {
            return self.scan_regexp_literal(RegExpLexContext {
                pattern_prefix: None,
                skip_syntax_check: false,
            });
        }

        let Some(unit) = self.peek_unit() else {
            return LexResult::Ready(Token::end_of_file(SourceSpan::at(start)));
        };

        if is_identifier_start(unit) || unit == b'\\' {
            return self.scan_identifier_or_keyword(
                request,
                start,
                start_line,
                start_line_start,
                start_column,
                begins_at_line_start,
            );
        }
        if is_decimal_digit(unit) {
            return self.scan_numeric_literal(
                start,
                start_line,
                start_line_start,
                start_column,
                begins_at_line_start,
            );
        }
        if unit == b'\'' || unit == b'"' {
            return self.scan_string_literal(
                unit,
                start,
                start_line,
                start_line_start,
                start_column,
                begins_at_line_start,
            );
        }
        if unit == b'`' {
            return self.scan_template_literal(
                TemplateLexContext {
                    raw_strings: RawStringMode::Build,
                    expression_depth: 0,
                },
                start,
                start_line,
                start_line_start,
                start_column,
                begins_at_line_start,
            );
        }
        // C++ JSC: `.5` is a valid numeric literal (DecimalLiteral starting
        // with DecimalDigits omitted before the dot).
        if unit == b'.' && matches!(self.peek_next_unit(), Some(next) if is_decimal_digit(next)) {
            return self.scan_numeric_literal(
                start,
                start_line,
                start_line_start,
                start_column,
                begins_at_line_start,
            );
        }

        match self.scan_punctuator() {
            Some(punctuator) => LexResult::Ready(self.make_token(
                TokenKind::Punctuator(punctuator),
                TokenData::None,
                start,
                start_line,
                start_line_start,
                start_column,
                begins_at_line_start,
                TokenFlags::default(),
            )),
            None => {
                self.advance_unit();
                LexResult::Error(self.error_at(
                    start,
                    self.cursor,
                    LexicalErrorKind::InvalidCharacter,
                ))
            }
        }
    }

    pub fn regexp_literal(&mut self, context: RegExpLexContext) -> LexResult<Token> {
        self.state.goal = LexGoal::RegExp;
        self.skip_trivia(false);
        self.scan_regexp_literal(context)
    }

    pub fn template_literal(&mut self, context: TemplateLexContext) -> LexResult<Token> {
        self.state.goal = LexGoal::TemplateTail;
        let start = self.cursor;
        self.scan_template_literal(
            context,
            start,
            self.line_number,
            self.line_start,
            self.column,
            self.state.at_line_start,
        )
    }
}

impl<'src, 'arena, E> Lexer<'src, 'arena, E> {
    fn is_eof(&self) -> bool {
        self.cursor >= self.source.range().end
    }

    fn peek_unit(&self) -> Option<u8> {
        self.peek_unit_at(self.cursor)
    }

    fn peek_next_unit(&self) -> Option<u8> {
        self.peek_unit_at(SourcePosition(self.cursor.0.saturating_add(1)))
    }

    fn peek_unit_at(&self, position: SourcePosition) -> Option<u8> {
        let unit = self.source.unit_at(position)?;
        Some(u8::try_from(unit).unwrap_or(0x80))
    }

    fn advance_unit(&mut self) -> Option<u8> {
        let unit = self.peek_unit()?;
        self.cursor.0 = self.cursor.0.saturating_add(1);
        match unit {
            b'\n' => {
                self.line_number = self.line_number.saturating_add(1);
                self.line_start = self.cursor;
                self.column = 0;
                self.state.at_line_start = true;
            }
            b'\r' => {
                if self.peek_unit() == Some(b'\n') {
                    self.cursor.0 = self.cursor.0.saturating_add(1);
                }
                self.line_number = self.line_number.saturating_add(1);
                self.line_start = self.cursor;
                self.column = 0;
                self.state.at_line_start = true;
            }
            _ => {
                self.column = self.column.saturating_add(1);
                self.state.at_line_start = false;
            }
        }
        Some(unit)
    }

    fn skip_trivia(&mut self, allow_html_comments: bool) {
        let mut saw_line = false;
        loop {
            match self.peek_unit() {
                Some(b' ' | b'\t' | 0x0b | 0x0c) => {
                    self.advance_unit();
                }
                Some(b'\n' | b'\r') => {
                    saw_line = true;
                    self.advance_unit();
                }
                Some(b'/') if self.peek_next_unit() == Some(b'/') => {
                    self.advance_unit();
                    self.advance_unit();
                    while let Some(unit) = self.peek_unit() {
                        if is_line_terminator(unit) {
                            break;
                        }
                        self.advance_unit();
                    }
                }
                Some(b'/') if self.peek_next_unit() == Some(b'*') => {
                    self.advance_unit();
                    self.advance_unit();
                    while let Some(unit) = self.advance_unit() {
                        if is_line_terminator(unit) {
                            saw_line = true;
                        }
                        if unit == b'*' && self.peek_unit() == Some(b'/') {
                            self.advance_unit();
                            break;
                        }
                    }
                }
                Some(b'<')
                    if allow_html_comments
                        && self.peek_unit_at(SourcePosition(self.cursor.0 + 1)) == Some(b'!')
                        && self.peek_unit_at(SourcePosition(self.cursor.0 + 2)) == Some(b'-')
                        && self.peek_unit_at(SourcePosition(self.cursor.0 + 3)) == Some(b'-') =>
                {
                    while let Some(unit) = self.peek_unit() {
                        if is_line_terminator(unit) {
                            break;
                        }
                        self.advance_unit();
                    }
                }
                _ => break,
            }
        }
        self.state.has_line_terminator_before_token = saw_line;
    }

    fn scan_identifier_or_keyword(
        &mut self,
        request: LexRequest,
        start: SourcePosition,
        line: u32,
        line_start: SourcePosition,
        start_column: u32,
        begins_at_line_start: bool,
    ) -> LexResult<Token> {
        let mut contains_escape = false;
        if self.peek_unit() == Some(b'\\') {
            contains_escape = true;
            if !self.scan_identifier_escape() {
                return LexResult::Error(self.error_at(
                    start,
                    self.cursor,
                    LexicalErrorKind::InvalidIdentifierEscape,
                ));
            }
        } else {
            self.advance_unit();
        }
        while let Some(unit) = self.peek_unit() {
            if is_identifier_continue(unit) {
                self.advance_unit();
            } else if unit == b'\\' {
                contains_escape = true;
                if !self.scan_identifier_escape() {
                    return LexResult::Error(self.error_at(
                        start,
                        self.cursor,
                        LexicalErrorKind::InvalidIdentifierEscape,
                    ));
                }
            } else {
                break;
            }
        }

        let raw = self.ascii_slice(start, self.cursor);
        let symbol = if raw.starts_with('#') {
            self.identifiers
                .reserve_identifier_text(IdentifierSource::PrivateName, raw.clone())
        } else {
            self.identifiers
                .reserve_identifier_text(IdentifierSource::SourceSlice, raw.clone())
        };
        let flags = TokenFlags {
            contains_escape,
            ..TokenFlags::default()
        };

        if raw.starts_with('#') && raw.len() == 1 {
            return LexResult::Ready(self.make_token(
                TokenKind::Error(LexicalErrorKind::InvalidPrivateName),
                TokenData::Identifier {
                    symbol,
                    escaped: contains_escape,
                },
                start,
                line,
                line_start,
                start_column,
                begins_at_line_start,
                flags,
            ));
        }

        if raw.starts_with('#') {
            return LexResult::Ready(self.make_token(
                TokenKind::Identifier(IdentifierTokenKind::PrivateName),
                TokenData::Identifier {
                    symbol,
                    escaped: contains_escape,
                },
                start,
                line,
                line_start,
                start_column,
                begins_at_line_start,
                flags,
            ));
        }

        if !contains_escape && request.keyword_policy == KeywordPolicy::Classify {
            if let Some(keyword) = classify_keyword(&raw, request.strict) {
                return LexResult::Ready(self.make_token(
                    TokenKind::Keyword(keyword),
                    TokenData::Identifier {
                        symbol,
                        escaped: false,
                    },
                    start,
                    line,
                    line_start,
                    start_column,
                    begins_at_line_start,
                    flags,
                ));
            }
        }

        let kind = if contains_escape && classify_keyword(&raw, request.strict).is_some() {
            IdentifierTokenKind::EscapedKeyword
        } else {
            IdentifierTokenKind::Ordinary
        };
        LexResult::Ready(self.make_token(
            TokenKind::Identifier(kind),
            TokenData::Identifier {
                symbol,
                escaped: contains_escape,
            },
            start,
            line,
            line_start,
            start_column,
            begins_at_line_start,
            flags,
        ))
    }

    fn scan_identifier_escape(&mut self) -> bool {
        self.advance_unit();
        if self.peek_unit() != Some(b'u') {
            return false;
        }
        self.advance_unit();
        if self.peek_unit() == Some(b'{') {
            self.advance_unit();
            let mut digits = 0;
            while let Some(unit) = self.peek_unit() {
                if unit == b'}' {
                    self.advance_unit();
                    return digits > 0;
                }
                if !is_hex_digit(unit) {
                    return false;
                }
                digits += 1;
                self.advance_unit();
            }
            return false;
        }
        for _ in 0..4 {
            if !matches!(self.peek_unit(), Some(unit) if is_hex_digit(unit)) {
                return false;
            }
            self.advance_unit();
        }
        true
    }

    fn scan_numeric_literal(
        &mut self,
        start: SourcePosition,
        line: u32,
        line_start: SourcePosition,
        start_column: u32,
        begins_at_line_start: bool,
    ) -> LexResult<Token> {
        let mut kind = NumericLiteralKind::Integer;
        // C++ JSC: handle `.5` style literals (DecimalLiteral with leading dot)
        if self.peek_unit() == Some(b'.') {
            kind = NumericLiteralKind::Double;
            self.advance_unit();
            self.scan_digits(is_decimal_digit);
            if matches!(self.peek_unit(), Some(b'e' | b'E')) {
                self.advance_unit();
                if matches!(self.peek_unit(), Some(b'+' | b'-')) {
                    self.advance_unit();
                }
                self.scan_digits(is_decimal_digit);
            }
        } else if self.peek_unit() == Some(b'0') {
            match self.peek_next_unit() {
                Some(b'x' | b'X') => {
                    self.advance_unit();
                    self.advance_unit();
                    if !self.scan_digits(is_hex_digit) {
                        return LexResult::Error(self.error_at(
                            start,
                            self.cursor,
                            LexicalErrorKind::UnterminatedHexNumber,
                        ));
                    }
                    if self.peek_unit() == Some(b'n') {
                        self.advance_unit();
                        kind = NumericLiteralKind::BigInt {
                            radix: NumericRadix::Hex,
                        };
                    }
                }
                Some(b'b' | b'B') => {
                    self.advance_unit();
                    self.advance_unit();
                    if !self.scan_digits(is_binary_digit) {
                        return LexResult::Error(self.error_at(
                            start,
                            self.cursor,
                            LexicalErrorKind::UnterminatedBinaryNumber,
                        ));
                    }
                    if self.peek_unit() == Some(b'n') {
                        self.advance_unit();
                        kind = NumericLiteralKind::BigInt {
                            radix: NumericRadix::Binary,
                        };
                    }
                }
                Some(b'o' | b'O') => {
                    self.advance_unit();
                    self.advance_unit();
                    if !self.scan_digits(is_octal_digit) {
                        return LexResult::Error(self.error_at(
                            start,
                            self.cursor,
                            LexicalErrorKind::UnterminatedOctalNumber,
                        ));
                    }
                    if self.peek_unit() == Some(b'n') {
                        self.advance_unit();
                        kind = NumericLiteralKind::BigInt {
                            radix: NumericRadix::Octal,
                        };
                    }
                }
                _ => self.scan_decimal_number(&mut kind),
            }
        } else {
            self.scan_decimal_number(&mut kind);
        }

        if matches!(self.peek_unit(), Some(unit) if is_identifier_start(unit)) {
            return LexResult::Error(self.error_at(
                start,
                self.cursor,
                LexicalErrorKind::InvalidNumericLiteral,
            ));
        }

        let raw = self
            .identifiers
            .reserve_identifier(IdentifierSource::NumericLiteral);
        LexResult::Ready(self.make_token(
            TokenKind::NumericLiteral(kind),
            TokenData::Numeric { raw },
            start,
            line,
            line_start,
            start_column,
            begins_at_line_start,
            TokenFlags::default(),
        ))
    }

    fn scan_decimal_number(&mut self, kind: &mut NumericLiteralKind) {
        self.scan_digits(is_decimal_digit);
        if self.peek_unit() == Some(b'.')
            && matches!(self.peek_next_unit(), Some(unit) if is_decimal_digit(unit))
        {
            *kind = NumericLiteralKind::Double;
            self.advance_unit();
            self.scan_digits(is_decimal_digit);
        }
        if matches!(self.peek_unit(), Some(b'e' | b'E')) {
            *kind = NumericLiteralKind::Double;
            self.advance_unit();
            if matches!(self.peek_unit(), Some(b'+' | b'-')) {
                self.advance_unit();
            }
            self.scan_digits(is_decimal_digit);
        }
        if self.peek_unit() == Some(b'n') && *kind == NumericLiteralKind::Integer {
            self.advance_unit();
            *kind = NumericLiteralKind::BigInt {
                radix: NumericRadix::Decimal,
            };
        }
    }

    fn scan_digits(&mut self, accepts: fn(u8) -> bool) -> bool {
        let mut saw_digit = false;
        while let Some(unit) = self.peek_unit() {
            if accepts(unit) || unit == b'_' {
                if accepts(unit) {
                    saw_digit = true;
                }
                self.advance_unit();
            } else {
                break;
            }
        }
        saw_digit
    }

    fn scan_string_literal(
        &mut self,
        quote: u8,
        start: SourcePosition,
        line: u32,
        line_start: SourcePosition,
        start_column: u32,
        begins_at_line_start: bool,
    ) -> LexResult<Token> {
        let mut contains_escape = false;
        self.advance_unit();
        while let Some(unit) = self.peek_unit() {
            if unit == quote {
                self.advance_unit();
                let cooked = self
                    .identifiers
                    .reserve_identifier(IdentifierSource::CookedString);
                return LexResult::Ready(self.make_token(
                    TokenKind::StringLiteral,
                    TokenData::String { cooked, raw: None },
                    start,
                    line,
                    line_start,
                    start_column,
                    begins_at_line_start,
                    TokenFlags {
                        contains_escape,
                        ..TokenFlags::default()
                    },
                ));
            }
            if is_line_terminator(unit) {
                return LexResult::Error(self.error_at(
                    start,
                    self.cursor,
                    LexicalErrorKind::UnterminatedStringLiteral,
                ));
            }
            if unit == b'\\' {
                contains_escape = true;
                self.advance_unit();
                if !self.is_eof() {
                    self.advance_unit();
                }
            } else {
                self.advance_unit();
            }
        }
        LexResult::Error(self.error_at(
            start,
            self.cursor,
            LexicalErrorKind::UnterminatedStringLiteral,
        ))
    }

    fn scan_template_literal(
        &mut self,
        context: TemplateLexContext,
        start: SourcePosition,
        line: u32,
        line_start: SourcePosition,
        start_column: u32,
        begins_at_line_start: bool,
    ) -> LexResult<Token> {
        let initial_segment = context.expression_depth == 0;
        if initial_segment && self.peek_unit() == Some(b'`') {
            self.advance_unit();
        }
        while let Some(unit) = self.peek_unit() {
            if unit == b'\\' {
                self.advance_unit();
                if !self.is_eof() {
                    self.advance_unit();
                }
                continue;
            }
            if unit == b'`' {
                self.advance_unit();
                let raw = self
                    .identifiers
                    .reserve_identifier(IdentifierSource::RawString);
                let cooked = (context.raw_strings == RawStringMode::Build).then(|| {
                    self.identifiers
                        .reserve_identifier(IdentifierSource::CookedString)
                });
                return LexResult::Ready(self.make_token(
                    TokenKind::TemplateLiteral(if initial_segment {
                        TemplateTokenKind::NoSubstitution
                    } else {
                        TemplateTokenKind::Tail
                    }),
                    TokenData::Template {
                        cooked,
                        raw,
                        is_tail: true,
                    },
                    start,
                    line,
                    line_start,
                    start_column,
                    begins_at_line_start,
                    TokenFlags::default(),
                ));
            }
            if unit == b'$' && self.peek_next_unit() == Some(b'{') {
                self.advance_unit();
                self.advance_unit();
                let raw = self
                    .identifiers
                    .reserve_identifier(IdentifierSource::RawString);
                let cooked = (context.raw_strings == RawStringMode::Build).then(|| {
                    self.identifiers
                        .reserve_identifier(IdentifierSource::CookedString)
                });
                return LexResult::Ready(self.make_token(
                    TokenKind::TemplateLiteral(if initial_segment {
                        TemplateTokenKind::Head
                    } else {
                        TemplateTokenKind::Middle
                    }),
                    TokenData::Template {
                        cooked,
                        raw,
                        is_tail: false,
                    },
                    start,
                    line,
                    line_start,
                    start_column,
                    begins_at_line_start,
                    TokenFlags::default(),
                ));
            }
            self.advance_unit();
        }
        LexResult::Error(self.error_at(
            start,
            self.cursor,
            LexicalErrorKind::UnterminatedTemplateLiteral,
        ))
    }

    fn scan_regexp_literal(&mut self, context: RegExpLexContext) -> LexResult<Token> {
        let start = self.cursor;
        let line = self.line_number;
        let line_start = self.line_start;
        let start_column = self.column;
        let begins_at_line_start = self.state.at_line_start;
        if context.pattern_prefix == Some('/') {
            self.advance_unit();
        } else if self.peek_unit() != Some(b'/') {
            return LexResult::Error(self.error_at(
                start,
                self.cursor,
                LexicalErrorKind::UnterminatedRegExpLiteral,
            ));
        } else {
            self.advance_unit();
        }
        let mut in_class = false;
        while let Some(unit) = self.peek_unit() {
            if is_line_terminator(unit) {
                return LexResult::Error(self.error_at(
                    start,
                    self.cursor,
                    LexicalErrorKind::UnterminatedRegExpLiteral,
                ));
            }
            match unit {
                b'\\' => {
                    self.advance_unit();
                    if !self.is_eof() {
                        self.advance_unit();
                    }
                }
                b'[' => {
                    in_class = true;
                    self.advance_unit();
                }
                b']' => {
                    in_class = false;
                    self.advance_unit();
                }
                b'/' if !in_class => {
                    self.advance_unit();
                    while matches!(self.peek_unit(), Some(unit) if is_identifier_continue(unit)) {
                        self.advance_unit();
                    }
                    let pattern = self
                        .identifiers
                        .reserve_identifier(IdentifierSource::SourceSlice);
                    let flags = self
                        .identifiers
                        .reserve_identifier(IdentifierSource::SourceSlice);
                    return LexResult::Ready(self.make_token(
                        TokenKind::RegExpLiteral,
                        TokenData::RegExp { pattern, flags },
                        start,
                        line,
                        line_start,
                        start_column,
                        begins_at_line_start,
                        TokenFlags::default(),
                    ));
                }
                _ => {
                    self.advance_unit();
                }
            }
        }
        LexResult::Error(self.error_at(
            start,
            self.cursor,
            LexicalErrorKind::UnterminatedRegExpLiteral,
        ))
    }

    fn scan_punctuator(&mut self) -> Option<Punctuator> {
        for (spelling, punctuator) in PUNCTUATORS {
            if self.starts_with_ascii(spelling) {
                for _ in 0..spelling.len() {
                    self.advance_unit();
                }
                return Some(*punctuator);
            }
        }
        None
    }

    fn starts_with_ascii(&self, spelling: &[u8]) -> bool {
        spelling.iter().enumerate().all(|(index, expected)| {
            self.peek_unit_at(SourcePosition(self.cursor.0.saturating_add(index as u32)))
                == Some(*expected)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn make_token(
        &self,
        kind: TokenKind,
        data: TokenData,
        start: SourcePosition,
        line: u32,
        line_start: SourcePosition,
        start_column: u32,
        begins_at_line_start: bool,
        flags: TokenFlags,
    ) -> Token {
        Token {
            kind,
            data,
            location: TokenLocation {
                span: SourceSpan::new(start, self.cursor),
                line,
                line_start,
                start: crate::syntax::source::LineColumn {
                    line,
                    column: start_column,
                },
                end: crate::syntax::source::LineColumn {
                    line: self.line_number,
                    column: self.column,
                },
            },
            flags: TokenFlags {
                has_line_terminator_before: self.state.has_line_terminator_before_token,
                begins_at_line_start,
                ..flags
            },
        }
    }

    fn error_at(
        &self,
        start: SourcePosition,
        end: SourcePosition,
        kind: LexicalErrorKind,
    ) -> LexerError {
        LexerError {
            span: SourceSpan::new(start, end),
            kind,
            message: format!("{kind:?}"),
        }
    }

    fn ascii_slice(&self, start: SourcePosition, end: SourcePosition) -> String {
        let mut result = String::new();
        let mut cursor = start;
        while cursor < end {
            if let Some(unit) = self.peek_unit_at(cursor) {
                result.push(char::from(unit));
            }
            cursor.0 = cursor.0.saturating_add(1);
        }
        result
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LexerSnapshot {
    pub cursor: SourcePosition,
    pub line_number: u32,
    pub line_start: SourcePosition,
    pub column: u32,
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

fn is_line_terminator(unit: u8) -> bool {
    matches!(unit, b'\n' | b'\r')
}

fn is_decimal_digit(unit: u8) -> bool {
    unit.is_ascii_digit()
}

fn is_binary_digit(unit: u8) -> bool {
    matches!(unit, b'0' | b'1')
}

fn is_octal_digit(unit: u8) -> bool {
    matches!(unit, b'0'..=b'7')
}

fn is_hex_digit(unit: u8) -> bool {
    unit.is_ascii_hexdigit()
}

fn is_identifier_start(unit: u8) -> bool {
    unit.is_ascii_alphabetic() || matches!(unit, b'$' | b'_' | b'#') || unit >= 0x80
}

fn is_identifier_continue(unit: u8) -> bool {
    is_identifier_start(unit) || is_decimal_digit(unit)
}

fn classify_keyword(spelling: &str, strictness: LexStrictness) -> Option<Keyword> {
    Some(match spelling {
        "null" => Keyword::Null,
        "true" => Keyword::True,
        "false" => Keyword::False,
        "break" => Keyword::Break,
        "case" => Keyword::Case,
        "default" => Keyword::Default,
        "for" => Keyword::For,
        "new" => Keyword::New,
        "var" => Keyword::Var,
        "const" => Keyword::Const,
        "continue" => Keyword::Continue,
        "function" => Keyword::Function,
        "return" => Keyword::Return,
        "if" => Keyword::If,
        "this" => Keyword::This,
        "do" => Keyword::Do,
        "while" => Keyword::While,
        "switch" => Keyword::Switch,
        "with" => Keyword::With,
        "throw" => Keyword::Throw,
        "try" => Keyword::Try,
        "catch" => Keyword::Catch,
        "finally" => Keyword::Finally,
        "debugger" => Keyword::Debugger,
        "else" => Keyword::Else,
        "import" => Keyword::Import,
        "export" => Keyword::Export,
        "class" => Keyword::Class,
        "extends" => Keyword::Extends,
        "super" => Keyword::Super,
        "typeof" => Keyword::Typeof,
        "void" => Keyword::Void,
        "delete" => Keyword::Delete,
        "instanceof" => Keyword::Instanceof,
        "in" => Keyword::In,
        "let" => Keyword::Contextual(ContextualKeyword::Let),
        "yield" => Keyword::Contextual(ContextualKeyword::Yield),
        "await" => Keyword::Contextual(ContextualKeyword::Await),
        "as" => Keyword::Contextual(ContextualKeyword::As),
        "from" => Keyword::Contextual(ContextualKeyword::From),
        "of" => Keyword::Contextual(ContextualKeyword::Of),
        "static" => Keyword::Contextual(ContextualKeyword::Static),
        "get" => Keyword::Contextual(ContextualKeyword::Get),
        "set" => Keyword::Contextual(ContextualKeyword::Set),
        "async" => Keyword::Contextual(ContextualKeyword::Async),
        "enum" => Keyword::Reserved,
        "implements" | "interface" | "package" | "private" | "protected" | "public"
            if strictness == LexStrictness::Strict =>
        {
            Keyword::ReservedIfStrict
        }
        "implements" | "interface" | "package" | "private" | "protected" | "public" => return None,
        _ => return None,
    })
}

const PUNCTUATORS: &[(&[u8], Punctuator)] = &[
    (b">>>=", Punctuator::UnsignedRightShiftEqual),
    (b"===", Punctuator::StrictEqual),
    (b"!==", Punctuator::StrictNotEqual),
    (b">>>", Punctuator::UnsignedRightShift),
    (b"<<=", Punctuator::LeftShiftEqual),
    (b">>=", Punctuator::RightShiftEqual),
    (b"**=", Punctuator::PowEqual),
    (b"&&=", Punctuator::AndEqual),
    (b"||=", Punctuator::OrEqual),
    (b"??=", Punctuator::CoalesceEqual),
    (b"...", Punctuator::DotDotDot),
    (b"=>", Punctuator::ArrowFunction),
    (b"?.", Punctuator::QuestionDot),
    (b"++", Punctuator::PlusPlus),
    (b"--", Punctuator::MinusMinus),
    (b"??", Punctuator::Coalesce),
    (b"||", Punctuator::Or),
    (b"&&", Punctuator::And),
    (b"==", Punctuator::EqualEqual),
    (b"!=", Punctuator::NotEqual),
    (b"<=", Punctuator::LessEqual),
    (b">=", Punctuator::GreaterEqual),
    (b"<<", Punctuator::LeftShift),
    (b">>", Punctuator::RightShift),
    (b"+=", Punctuator::PlusEqual),
    (b"-=", Punctuator::MinusEqual),
    (b"*=", Punctuator::MultiplyEqual),
    (b"/=", Punctuator::DivideEqual),
    (b"%=", Punctuator::ModEqual),
    (b"&=", Punctuator::BitAndEqual),
    (b"^=", Punctuator::BitXorEqual),
    (b"|=", Punctuator::BitOrEqual),
    (b"**", Punctuator::Pow),
    (b"{", Punctuator::OpenBrace),
    (b"}", Punctuator::CloseBrace),
    (b"(", Punctuator::OpenParen),
    (b")", Punctuator::CloseParen),
    (b"[", Punctuator::OpenBracket),
    (b"]", Punctuator::CloseBracket),
    (b",", Punctuator::Comma),
    (b"?", Punctuator::Question),
    (b"`", Punctuator::Backquote),
    (b";", Punctuator::Semicolon),
    (b":", Punctuator::Colon),
    (b".", Punctuator::Dot),
    (b"=", Punctuator::Equal),
    (b"!", Punctuator::Exclamation),
    (b"~", Punctuator::Tilde),
    (b"|", Punctuator::BitOr),
    (b"^", Punctuator::BitXor),
    (b"&", Punctuator::BitAnd),
    (b"<", Punctuator::LessThan),
    (b">", Punctuator::GreaterThan),
    (b"+", Punctuator::Plus),
    (b"-", Punctuator::Minus),
    (b"*", Punctuator::Multiply),
    (b"/", Punctuator::Divide),
    (b"%", Punctuator::Mod),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::arena::IdentifierArena;
    use crate::syntax::source::{SourceOrigin, SourceProvider, SourceText};
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

    #[test]
    fn lexer_classifies_keywords_identifiers_numbers_and_punctuators() {
        let source = source("let answer = 42n ?? value;");
        let mut identifiers = IdentifierArena::default();
        let mut lexer = Lexer::<()>::new(&source, &mut identifiers);
        let request = LexRequest::for_goal(LexGoal::Div);

        let kinds = [
            lexer.next_token(request),
            lexer.next_token(request),
            lexer.next_token(request),
            lexer.next_token(request),
            lexer.next_token(request),
            lexer.next_token(request),
            lexer.next_token(request),
        ]
        .map(|result| {
            if let LexResult::Ready(token) = result {
                token.kind
            } else {
                TokenKind::Error(LexicalErrorKind::InvalidCharacter)
            }
        });

        assert_eq!(
            kinds,
            [
                TokenKind::Keyword(Keyword::Contextual(ContextualKeyword::Let)),
                TokenKind::Identifier(IdentifierTokenKind::Ordinary),
                TokenKind::Punctuator(Punctuator::Equal),
                TokenKind::NumericLiteral(NumericLiteralKind::BigInt {
                    radix: NumericRadix::Decimal,
                }),
                TokenKind::Punctuator(Punctuator::Coalesce),
                TokenKind::Identifier(IdentifierTokenKind::Ordinary),
                TokenKind::Punctuator(Punctuator::Semicolon),
            ]
        );
    }

    #[test]
    fn lexer_reports_invalid_numeric_suffix() {
        let source = source("123abc");
        let mut identifiers = IdentifierArena::default();
        let mut lexer = Lexer::<()>::new(&source, &mut identifiers);

        assert!(matches!(
            lexer.next_token(LexRequest::for_goal(LexGoal::Div)),
            LexResult::Error(LexerError {
                kind: LexicalErrorKind::InvalidNumericLiteral,
                ..
            })
        ));
    }

    #[test]
    fn lexer_scans_regexp_without_creating_execution_path() {
        let source = source("/a[b\\/]/gi");
        let mut identifiers = IdentifierArena::default();
        let mut lexer = Lexer::<()>::new(&source, &mut identifiers);

        let result = lexer.regexp_literal(RegExpLexContext {
            pattern_prefix: None,
            skip_syntax_check: false,
        });

        assert!(matches!(
            result,
            LexResult::Ready(Token {
                kind: TokenKind::RegExpLiteral,
                ..
            })
        ));
    }
}
