- YARR records JIT fallback as feature-specific failure reasons rather than a single fallback boolean.
- Fallback detection is split between compile-time feature knowledge and JIT-time shape checks; codegen is attempted for supported special cases.
- Backreferences and complex parentheses moved from blanket interpreter fallback toward targeted code generation or internal subpattern storage.
- Match-only and capture-producing executions are separate modes, including for fallback decisions.

## Facts

- 2007-06-28 (47ca41ed) rationale: Unsupported regexp constructs initially stayed on PCRE because WREC/YARR code generation did not yet cover the full JavaScript regexp surface. (code)
- 2007-11-04 (f26e598e) pitfall: Backreference support requires capture start/end data even when the public caller only asked whether a match exists. (code)
- 2007-11-04 (f26e598e) rationale: Feature-specific failure reporting makes it possible to diagnose why a regexp fell back under compiled-pattern dumps. (sourced)
- 2022-05-18 (7f4a2f1f) pitfall: JIT fallback must preserve the caller's match mode; a capture-producing fallback cannot be substituted for match-only execution without changing allocation and output behavior. (code)
- 2026-02-05 (98d1969d) pitfall: ParenContext sizing and capture save/restore must be enabled for internal backreference storage in MatchOnly mode, not just for IncludeSubpatterns mode. (code)
- 2026-02-25 (33b89872) pitfall: Feature-specific failure reasons must remain stable enough for dumpCompiledRegExpPatterns output to identify real unsupported constructs rather than generic fallback. (sourced)

## Moves

- 2010-04-14 (d122b0d1) replaced [[yarr-jit-fallback-detection-at-generation]]: Detecting unsupported regex features (back-references, multi-quantifier subpatterns) during JIT code generation wasted partial JIT work; moving detection to the regex compiler lets jitCompileRegex skip RegexGenerator entirely for patterns that must fall back to PCRE. (code)
- 2010-05-27 (6fd9c6ee) replaced [[yarr-jit-fallback-tracking-at-compiler-layer]]: The old single m_shouldFallBack flag set by the compiler for both backreferences and quantified parentheses forced the JIT to skip all parentheses patterns regardless of whether the JIT could handle them; splitting into a per-feature flag (m_containsBackreferences) for backreferences and runtime detection inside the JIT for parentheses enables the new generateParenthesesGreedyNoBacktrack path for last-term greedy quantified parens that never need backtracking, yielding 18% improvement on tagcloud. (sourced)
- 2010-05-27 (b0e898b0) replaced [[yarr-jit-fallback-detection-at-compile-time]]: Tracking fallback in the compiler layer (m_shouldFallBack flag set for all quantified parens) over-approximated fallback cases and prevented JIT generation of greedy quantified parens at end of main disjunction; moving detection to JIT time allows more patterns to be JIT-compiled with a special-case no-backtrack path. (sourced)
- 2018-01-24 (99b1f7d2) replaced [[yarr-jit-boolean-fallback]]: Replacing the boolean fallback with JITFailureReason lets YarrJIT preserve which unsupported construct or allocation failure caused interpreter fallback and dump it under Options::dumpCompiledRegExpPatterns. (sourced)
- 2018-08-24 (37f6320c) replaced [[yarr-jit-non-greedy-paren-fallback]]: Non-greedy parenthesized subpatterns previously caused a JIT bail-out (fell back to interpreter); the new implementation adds JIT code generation for non-greedy nested parens by extending the existing greedy paren infrastructure with a skip-first-try jump and backtrack re-entry path, expanding what patterns the JIT can compile. (code)
- 2024-03-26 (9ff5c9b7) replaced [[yarr-16bit-ignorecase-backreference-fallback]]: The JIT could not inline canonical equivalence for non-ASCII 16-bit ignore-case backreferences with its fixed Yarr registers, so supported platforms call a thunk that saves caller-saves, remaps fixed registers to operationAreCanonicallyEquivalent, and returns the boolean in the character register. (code)
- 2026-02-05 (98d1969d) replaced [[matchonly-backreference-interpreter-fallback]]: MatchOnly backreferences changed from JIT fallback to internal frame subpattern storage because MatchOnly has no external output vector but backreferences need capture start/end data during matching. (code)
