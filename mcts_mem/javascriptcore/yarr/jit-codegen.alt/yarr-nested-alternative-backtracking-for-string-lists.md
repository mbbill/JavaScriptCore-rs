- Multi-alternative parenthesized patterns used NestedAlternativeBegin/Next/End once the disjunction had more than one alternative.
- Pattern character generation appended failure branches and fell through on match for each term.
- The old anchored string-list case still saved a continuation PC and emitted backtracking code between alternatives.

## Moves

- 2025-02-21 (12c34ef5) replaced by [[jit-codegen]]: Anchored non-capturing alternations of literal strings were recognized as non-backtracking string lists so the JIT could jump on a whole-alternative match instead of emitting per-term fall-through and backtracking continuation code. (sourced)
