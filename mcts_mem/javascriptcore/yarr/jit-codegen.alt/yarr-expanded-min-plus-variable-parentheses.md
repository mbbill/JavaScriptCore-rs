- Parenthesized subpatterns with a non-zero minimum are expanded into a FixedCount prefix plus a variable tail before JIT code generation.

## Moves

- 2026-06-02 (6646c49f) replaced by [[jit-codegen]]: Forward-direction parenthesized subpatterns with non-zero minimum are kept as one quantified term because expanding them into FixedCount{min} plus variable tail deep-copied the disjunction subtree and could hit OffsetTooLarge or pattern-size limits for very large bounds. (code)
