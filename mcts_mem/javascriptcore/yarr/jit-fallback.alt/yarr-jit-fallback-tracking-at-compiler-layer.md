- m_shouldFallBack set in RegexCompiler::atomBackReference
- m_shouldFallBack set in RegexCompiler::quantifyAtom for max>1 TypeParenthesesSubpattern
- jitCompileRegex checks m_shouldFallBack before attempting JIT

## Moves

- 2010-05-27 (6fd9c6ee) replaced by [[jit-fallback]]: The old single m_shouldFallBack flag set by the compiler for both backreferences and quantified parentheses forced the JIT to skip all parentheses patterns regardless of whether the JIT could handle them; splitting into a per-feature flag (m_containsBackreferences) for backreferences and runtime detection inside the JIT for parentheses enables the new generateParenthesesGreedyNoBacktrack path for last-term greedy quantified parens that never need backtracking, yielding 18% improvement on tagcloud. (sourced)
