use crate::yarr::RegexFlags;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrSimpleMatchRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrSimpleMatch {
    pub start: usize,
    pub end: usize,
    pub captures: Vec<Option<YarrSimpleMatchRange>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum YarrSimpleExecError {
    InvalidPattern,
}

#[derive(Clone, Debug)]
struct Pattern {
    alternatives: Vec<Alternative>,
    capture_count: usize,
}

#[derive(Clone, Debug)]
struct Alternative {
    terms: Vec<Term>,
}

#[derive(Clone, Debug)]
struct Term {
    atom: Atom,
    quantifier: Quantifier,
}

#[derive(Clone, Debug)]
enum Atom {
    Literal(char),
    Dot,
    Class(CharacterClass),
    AnchorStart,
    AnchorEnd,
    Group {
        capture_index: Option<usize>,
        alternatives: Vec<Alternative>,
    },
    BackReference(usize),
}

#[derive(Clone, Debug)]
struct Quantifier {
    min: usize,
    max: Option<usize>,
    greedy: bool,
}

#[derive(Clone, Debug)]
struct CharacterClass {
    inverted: bool,
    members: Vec<CharacterClassMember>,
}

#[derive(Clone, Debug)]
enum CharacterClassMember {
    Char(char),
    Range(char, char),
    BuiltIn(BuiltInClass, bool),
}

#[derive(Clone, Copy, Debug)]
enum BuiltInClass {
    Digit,
    Space,
    Word,
}

#[derive(Clone, Debug)]
struct MatchState {
    position: usize,
    captures: Vec<Option<YarrSimpleMatchRange>>,
}

#[derive(Clone, Debug)]
struct RepetitionState {
    state: MatchState,
    can_expand: bool,
}

pub fn execute_simple_yarr(
    pattern: &str,
    flags: RegexFlags,
    input: &str,
    start_index: usize,
) -> Result<Option<YarrSimpleMatch>, YarrSimpleExecError> {
    if start_index > input.len() || !input.is_char_boundary(start_index) {
        return Ok(None);
    }

    let pattern = Parser::new(pattern).parse()?;
    let mut positions = input
        .char_indices()
        .map(|(index, _)| index)
        .filter(|index| *index >= start_index)
        .collect::<Vec<_>>();
    positions.push(input.len());
    if flags.sticky {
        positions.retain(|index| *index == start_index);
    }

    for position in positions {
        let state = MatchState {
            position,
            captures: vec![None; pattern.capture_count],
        };
        if let Some(result) = match_alternatives(&pattern.alternatives, input, flags, state) {
            return Ok(Some(YarrSimpleMatch {
                start: position,
                end: result.position,
                captures: result.captures,
            }));
        }
    }

    Ok(None)
}

struct Parser<'a> {
    chars: Vec<char>,
    index: usize,
    capture_count: usize,
    _source: core::marker::PhantomData<&'a str>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().collect(),
            index: 0,
            capture_count: 0,
            _source: core::marker::PhantomData,
        }
    }

    fn parse(mut self) -> Result<Pattern, YarrSimpleExecError> {
        let alternatives = self.parse_alternatives(None)?;
        if self.index != self.chars.len() {
            return Err(YarrSimpleExecError::InvalidPattern);
        }
        Ok(Pattern {
            alternatives,
            capture_count: self.capture_count,
        })
    }

    fn parse_alternatives(
        &mut self,
        terminator: Option<char>,
    ) -> Result<Vec<Alternative>, YarrSimpleExecError> {
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(Alternative {
                terms: self.parse_terms(terminator)?,
            });
            if self.peek() == Some('|') {
                self.index += 1;
                continue;
            }
            break;
        }
        Ok(alternatives)
    }

    fn parse_terms(&mut self, terminator: Option<char>) -> Result<Vec<Term>, YarrSimpleExecError> {
        let mut terms = Vec::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == terminator || ch == '|' {
                break;
            }
            let atom = self.parse_atom()?;
            let quantifier = self.parse_quantifier()?;
            terms.push(Term { atom, quantifier });
        }
        Ok(terms)
    }

    fn parse_atom(&mut self) -> Result<Atom, YarrSimpleExecError> {
        let Some(ch) = self.next() else {
            return Err(YarrSimpleExecError::InvalidPattern);
        };
        match ch {
            '^' => Ok(Atom::AnchorStart),
            '$' => Ok(Atom::AnchorEnd),
            '.' => Ok(Atom::Dot),
            '[' => self.parse_character_class(),
            '(' => self.parse_group(),
            ')' | '*' | '+' | '?' => Err(YarrSimpleExecError::InvalidPattern),
            '\\' => self.parse_escape_atom(),
            _ => Ok(Atom::Literal(ch)),
        }
    }

    fn parse_group(&mut self) -> Result<Atom, YarrSimpleExecError> {
        let capture_index = if self.peek() == Some('?') {
            self.index += 1;
            if self.next() != Some(':') {
                return Err(YarrSimpleExecError::InvalidPattern);
            }
            None
        } else {
            self.capture_count += 1;
            Some(self.capture_count)
        };
        let alternatives = self.parse_alternatives(Some(')'))?;
        if self.next() != Some(')') {
            return Err(YarrSimpleExecError::InvalidPattern);
        }
        Ok(Atom::Group {
            capture_index,
            alternatives,
        })
    }

    fn parse_escape_atom(&mut self) -> Result<Atom, YarrSimpleExecError> {
        let Some(ch) = self.next() else {
            return Err(YarrSimpleExecError::InvalidPattern);
        };
        match ch {
            'd' => Ok(Atom::Class(CharacterClass::built_in(
                BuiltInClass::Digit,
                false,
            ))),
            'D' => Ok(Atom::Class(CharacterClass::built_in(
                BuiltInClass::Digit,
                true,
            ))),
            's' => Ok(Atom::Class(CharacterClass::built_in(
                BuiltInClass::Space,
                false,
            ))),
            'S' => Ok(Atom::Class(CharacterClass::built_in(
                BuiltInClass::Space,
                true,
            ))),
            'w' => Ok(Atom::Class(CharacterClass::built_in(
                BuiltInClass::Word,
                false,
            ))),
            'W' => Ok(Atom::Class(CharacterClass::built_in(
                BuiltInClass::Word,
                true,
            ))),
            'n' => Ok(Atom::Literal('\n')),
            'r' => Ok(Atom::Literal('\r')),
            't' => Ok(Atom::Literal('\t')),
            '0' => Ok(Atom::Literal('\0')),
            '1'..='9' => {
                let mut value = ch.to_digit(10).unwrap_or_default() as usize;
                while self.peek().is_some_and(|next| next.is_ascii_digit()) {
                    value = value
                        .saturating_mul(10)
                        .saturating_add(self.next().unwrap_or_default() as usize - '0' as usize);
                }
                Ok(Atom::BackReference(value))
            }
            _ => Ok(Atom::Literal(ch)),
        }
    }

    fn parse_character_class(&mut self) -> Result<Atom, YarrSimpleExecError> {
        let inverted = if self.peek() == Some('^') {
            self.index += 1;
            true
        } else {
            false
        };
        let mut members = Vec::new();
        let mut first = true;
        while let Some(ch) = self.peek() {
            if ch == ']' && !first {
                self.index += 1;
                return Ok(Atom::Class(CharacterClass { inverted, members }));
            }
            first = false;
            let start = self.parse_class_member()?;
            if self.peek() == Some('-')
                && self.chars.get(self.index + 1).copied().is_some()
                && self.chars.get(self.index + 1).copied() != Some(']')
            {
                self.index += 1;
                let end = self.parse_class_member()?;
                if let (CharacterClassMember::Char(start), CharacterClassMember::Char(end)) =
                    (&start, &end)
                {
                    members.push(CharacterClassMember::Range(*start, *end));
                } else {
                    return Err(YarrSimpleExecError::InvalidPattern);
                }
            } else {
                members.push(start);
            }
        }
        Err(YarrSimpleExecError::InvalidPattern)
    }

    fn parse_class_member(&mut self) -> Result<CharacterClassMember, YarrSimpleExecError> {
        let Some(ch) = self.next() else {
            return Err(YarrSimpleExecError::InvalidPattern);
        };
        if ch != '\\' {
            return Ok(CharacterClassMember::Char(ch));
        }
        let Some(escaped) = self.next() else {
            return Err(YarrSimpleExecError::InvalidPattern);
        };
        Ok(match escaped {
            'd' => CharacterClassMember::BuiltIn(BuiltInClass::Digit, false),
            'D' => CharacterClassMember::BuiltIn(BuiltInClass::Digit, true),
            's' => CharacterClassMember::BuiltIn(BuiltInClass::Space, false),
            'S' => CharacterClassMember::BuiltIn(BuiltInClass::Space, true),
            'w' => CharacterClassMember::BuiltIn(BuiltInClass::Word, false),
            'W' => CharacterClassMember::BuiltIn(BuiltInClass::Word, true),
            'n' => CharacterClassMember::Char('\n'),
            'r' => CharacterClassMember::Char('\r'),
            't' => CharacterClassMember::Char('\t'),
            _ => CharacterClassMember::Char(escaped),
        })
    }

    fn parse_quantifier(&mut self) -> Result<Quantifier, YarrSimpleExecError> {
        let mut quantifier = match self.peek() {
            Some('*') => {
                self.index += 1;
                Quantifier {
                    min: 0,
                    max: None,
                    greedy: true,
                }
            }
            Some('+') => {
                self.index += 1;
                Quantifier {
                    min: 1,
                    max: None,
                    greedy: true,
                }
            }
            Some('?') => {
                self.index += 1;
                Quantifier {
                    min: 0,
                    max: Some(1),
                    greedy: true,
                }
            }
            Some('{') => match self.try_parse_braced_quantifier()? {
                Some(quantifier) => quantifier,
                None => {
                    return Ok(Quantifier {
                        min: 1,
                        max: Some(1),
                        greedy: true,
                    });
                }
            },
            _ => {
                return Ok(Quantifier {
                    min: 1,
                    max: Some(1),
                    greedy: true,
                });
            }
        };
        if self.peek() == Some('?') {
            self.index += 1;
            quantifier.greedy = false;
        }
        Ok(quantifier)
    }

    fn try_parse_braced_quantifier(&mut self) -> Result<Option<Quantifier>, YarrSimpleExecError> {
        let start = self.index;
        self.index += 1;
        let Some(min) = self.parse_decimal() else {
            self.index = start;
            return Ok(None);
        };
        let max = if self.peek() == Some(',') {
            self.index += 1;
            self.parse_decimal()
        } else {
            Some(min)
        };
        if self.next() != Some('}') {
            return Err(YarrSimpleExecError::InvalidPattern);
        }
        if max.is_some_and(|max| min > max) {
            return Err(YarrSimpleExecError::InvalidPattern);
        }
        Ok(Some(Quantifier {
            min,
            max,
            greedy: true,
        }))
    }

    fn parse_decimal(&mut self) -> Option<usize> {
        let start = self.index;
        let mut value = 0usize;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            value = value
                .saturating_mul(10)
                .saturating_add(self.next().unwrap_or_default() as usize - '0' as usize);
        }
        (self.index > start).then_some(value)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;
        Some(ch)
    }
}

impl CharacterClass {
    fn built_in(kind: BuiltInClass, inverted: bool) -> Self {
        Self {
            inverted: false,
            members: vec![CharacterClassMember::BuiltIn(kind, inverted)],
        }
    }
}

fn match_alternatives(
    alternatives: &[Alternative],
    input: &str,
    flags: RegexFlags,
    state: MatchState,
) -> Option<MatchState> {
    match_alternatives_states(alternatives, input, flags, state)
        .into_iter()
        .next()
}

fn match_alternatives_states(
    alternatives: &[Alternative],
    input: &str,
    flags: RegexFlags,
    state: MatchState,
) -> Vec<MatchState> {
    alternatives
        .iter()
        .flat_map(|alternative| {
            match_terms_states(&alternative.terms, 0, input, flags, state.clone())
        })
        .collect()
}

fn quantified_atom_states(
    term: &Term,
    input: &str,
    flags: RegexFlags,
    state: MatchState,
) -> Vec<MatchState> {
    let mut levels = vec![vec![RepetitionState {
        state,
        can_expand: true,
    }]];
    loop {
        if term
            .quantifier
            .max
            .is_some_and(|max| levels.len().saturating_sub(1) >= max)
        {
            break;
        };
        let Some(previous_level) = levels.last() else {
            break;
        };
        let mut next_level = Vec::new();
        for previous in previous_level {
            if !previous.can_expand {
                continue;
            }
            for next in match_atom_states(&term.atom, input, flags, previous.state.clone()) {
                next_level.push(RepetitionState {
                    can_expand: next.position != previous.state.position,
                    state: next,
                });
            }
        }
        if next_level.is_empty() {
            break;
        }
        let can_expand = next_level.iter().any(|state| state.can_expand);
        levels.push(next_level);
        if !can_expand {
            break;
        }
    }

    let mut counts = (term.quantifier.min..levels.len()).collect::<Vec<_>>();
    if term.quantifier.greedy {
        counts.reverse();
    }

    let mut states = Vec::new();
    for count in counts {
        states.extend(levels[count].iter().map(|state| state.state.clone()));
    }
    states
}

fn match_terms_states(
    terms: &[Term],
    index: usize,
    input: &str,
    flags: RegexFlags,
    state: MatchState,
) -> Vec<MatchState> {
    if index == terms.len() {
        return vec![state];
    }
    let term = &terms[index];
    quantified_atom_states(term, input, flags, state)
        .into_iter()
        .flat_map(|next| match_terms_states(terms, index + 1, input, flags, next))
        .collect()
}

fn match_atom_states(
    atom: &Atom,
    input: &str,
    flags: RegexFlags,
    mut state: MatchState,
) -> Vec<MatchState> {
    match atom {
        Atom::Literal(expected) => {
            let Some((ch, next)) = read_char(input, state.position) else {
                return Vec::new();
            };
            if !char_eq(*expected, ch, flags.ignore_case) {
                return Vec::new();
            }
            state.position = next;
            vec![state]
        }
        Atom::Dot => {
            let Some((ch, next)) = read_char(input, state.position) else {
                return Vec::new();
            };
            if !flags.dot_all && is_line_terminator(ch) {
                return Vec::new();
            }
            state.position = next;
            vec![state]
        }
        Atom::Class(class) => {
            let Some((ch, next)) = read_char(input, state.position) else {
                return Vec::new();
            };
            if !character_class_matches(class, ch, flags.ignore_case) {
                return Vec::new();
            }
            state.position = next;
            vec![state]
        }
        Atom::AnchorStart => {
            if state.position == 0
                || (flags.multiline
                    && previous_char(input, state.position).is_some_and(is_line_terminator))
            {
                vec![state]
            } else {
                Vec::new()
            }
        }
        Atom::AnchorEnd => {
            if state.position == input.len()
                || (flags.multiline
                    && read_char(input, state.position)
                        .map(|(ch, _)| is_line_terminator(ch))
                        .unwrap_or(false))
            {
                vec![state]
            } else {
                Vec::new()
            }
        }
        Atom::Group {
            capture_index,
            alternatives,
        } => {
            let start = state.position;
            match_alternatives_states(alternatives, input, flags, state)
                .into_iter()
                .map(|mut result| {
                    if let Some(capture_index) = capture_index {
                        let index = capture_index.saturating_sub(1);
                        if let Some(slot) = result.captures.get_mut(index) {
                            *slot = Some(YarrSimpleMatchRange {
                                start,
                                end: result.position,
                            });
                        }
                    }
                    result
                })
                .collect()
        }
        Atom::BackReference(capture_index) => {
            let Some(range) = state
                .captures
                .get(capture_index.saturating_sub(1))
                .copied()
                .flatten()
            else {
                return vec![state];
            };
            let Some(captured) = input.get(range.start..range.end) else {
                return Vec::new();
            };
            let Some(remaining) = input.get(state.position..) else {
                return Vec::new();
            };
            if starts_with(remaining, captured, flags.ignore_case) {
                state.position = state.position.saturating_add(captured.len());
                vec![state]
            } else {
                Vec::new()
            }
        }
    }
}

fn read_char(input: &str, position: usize) -> Option<(char, usize)> {
    let ch = input.get(position..)?.chars().next()?;
    Some((ch, position + ch.len_utf8()))
}

fn previous_char(input: &str, position: usize) -> Option<char> {
    input.get(..position)?.chars().next_back()
}

fn starts_with(input: &str, expected: &str, ignore_case: bool) -> bool {
    if input.len() < expected.len() {
        return false;
    }
    let Some(candidate) = input.get(..expected.len()) else {
        return false;
    };
    if ignore_case {
        candidate.eq_ignore_ascii_case(expected)
    } else {
        candidate == expected
    }
}

fn character_class_matches(class: &CharacterClass, ch: char, ignore_case: bool) -> bool {
    let matched = class
        .members
        .iter()
        .any(|member| character_class_member_matches(member, ch, ignore_case));
    matched ^ class.inverted
}

fn character_class_member_matches(
    member: &CharacterClassMember,
    ch: char,
    ignore_case: bool,
) -> bool {
    match member {
        CharacterClassMember::Char(expected) => char_eq(*expected, ch, ignore_case),
        CharacterClassMember::Range(start, end) => {
            let ch = if ignore_case {
                ch.to_ascii_lowercase()
            } else {
                ch
            };
            let start = if ignore_case {
                start.to_ascii_lowercase()
            } else {
                *start
            };
            let end = if ignore_case {
                end.to_ascii_lowercase()
            } else {
                *end
            };
            start <= ch && ch <= end
        }
        CharacterClassMember::BuiltIn(kind, inverted) => {
            built_in_class_matches(*kind, ch) ^ *inverted
        }
    }
}

fn built_in_class_matches(kind: BuiltInClass, ch: char) -> bool {
    match kind {
        BuiltInClass::Digit => ch.is_ascii_digit(),
        BuiltInClass::Space => matches!(
            ch,
            '\t' | '\n'
                | '\u{000B}'
                | '\u{000C}'
                | '\r'
                | ' '
                | '\u{00A0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200A}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202F}'
                    | '\u{205F}'
                    | '\u{3000}'
                    | '\u{FEFF}'
        ),
        BuiltInClass::Word => ch.is_ascii_alphanumeric() || ch == '_',
    }
}

fn char_eq(left: char, right: char, ignore_case: bool) -> bool {
    if ignore_case {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn is_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(pattern: &str, input: &str) -> Option<YarrSimpleMatch> {
        execute_simple_yarr(pattern, RegexFlags::default(), input, 0).unwrap()
    }

    #[test]
    fn simple_yarr_exec_matches_captures_and_backreferences() {
        let matched = exec(r#"('|")(.+?)\1"#, r#""core" tail"#).unwrap();

        assert_eq!(matched.start, 0);
        assert_eq!(matched.end, 6);
        assert_eq!(
            matched.captures[1],
            Some(YarrSimpleMatchRange { start: 1, end: 5 })
        );
    }

    #[test]
    fn simple_yarr_exec_matches_typescript_amd_dependency_pattern() {
        let flags = RegexFlags {
            global: true,
            ignore_case: true,
            multiline: true,
            ..RegexFlags::default()
        };
        let pattern =
            r#"^(\/\/\/\s*<amd-dependency\s+path=)('|")(.+?)\2\s*(static=('|")(.+?)\2\s*)*\/>"#;
        let input = r#"/// <amd-dependency path="core" static="true"/>"#;
        let matched = execute_simple_yarr(pattern, flags, input, 0)
            .unwrap()
            .unwrap();

        assert_eq!(matched.start, 0);
        assert_eq!(matched.end, input.len());
        assert_eq!(
            matched.captures[2],
            Some(YarrSimpleMatchRange { start: 26, end: 30 })
        );
        assert_eq!(
            matched.captures[5],
            Some(YarrSimpleMatchRange { start: 40, end: 44 })
        );
    }

    #[test]
    fn simple_yarr_exec_supports_multiline_anchor_search() {
        let flags = RegexFlags {
            multiline: true,
            ..RegexFlags::default()
        };
        let matched = execute_simple_yarr(r"^b", flags, "a\nb", 0)
            .unwrap()
            .unwrap();

        assert_eq!(matched.start, 2);
        assert_eq!(matched.end, 3);
    }

    #[test]
    fn simple_yarr_exec_unmatched_backreference_matches_empty() {
        let matched = exec(r"(a)?\1b", "b").unwrap();

        assert_eq!(matched.start, 0);
        assert_eq!(matched.end, 1);
        assert_eq!(matched.captures[0], None);
    }
}
