- RegularExpression represents the caller-supplied pattern in default partial-match mode.
- HTML pattern validation compiles the raw pattern once to test validity.
- HTML pattern matching compiles a second anchored wrapper expression around the same raw pattern.

## Moves

- 2026-03-29 (3591ed5f) replaced by [[pattern-analysis]]: HTML pattern matching needed raw-pattern validation plus anchored matching without compiling both the raw and anchored regular expressions, because anchoring can make an invalid raw pattern valid. (code)
