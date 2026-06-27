- Parser::parseParentheses always set m_error=UnsupportedParentheses and returned false
- Generator::generateParentheses(ParenthesesType) single function handling capturing/non_capturing/inverted_assertion with complex flow

## Moves

- 2008-12-05 (3fde03e7) replaced by [[yarr]]: The old parseParentheses immediately returned false with UnsupportedParentheses (forcing PCRE fallback) for all parenthesized subexpressions; the new code adds native JIT codegen for (?=) and (?!) lookahead/lookbehind assertions while still falling back to PCRE for capturing and non-capturing groups. (code)
