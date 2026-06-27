- m_shouldFallBack flag set in RegexPatternConstructor::quantifyAtom for all max>1 ParenthesesSubpattern
- m_shouldFallBack flag set in atomBackReference
- RegexGenerator::shouldFallBack() reads m_shouldFallBack flag

## Moves

- 2010-05-27 (b0e898b0) replaced by [[jit-fallback]]: Tracking fallback in the compiler layer (m_shouldFallBack flag set for all quantified parens) over-approximated fallback cases and prevented JIT generation of greedy quantified parens at end of main disjunction; moving detection to JIT time allows more patterns to be JIT-compiled with a special-case no-backtrack path. (code)
