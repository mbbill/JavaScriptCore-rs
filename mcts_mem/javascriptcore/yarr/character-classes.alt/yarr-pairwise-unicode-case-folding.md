- Case-insensitive matching expands a non-ASCII code point by adding Unicode::toUpper and Unicode::toLower variants.
- Backreference comparison accepts either direct equality or that single cased pair.
- Literal JIT compares assume non-ASCII case-insensitive characters have already been converted to classes when toUpper and toLower differ.

## Moves

- 2012-03-26 (d881db51) replaced by [[character-classes]]: The old mechanism assumed each codepoint has at most one additional case-insensitive match obtainable by Unicode::toUpper/Unicode::toLower, while the new mechanism can represent ES5.1 canonical equivalence sets and range encodings. (sourced)
