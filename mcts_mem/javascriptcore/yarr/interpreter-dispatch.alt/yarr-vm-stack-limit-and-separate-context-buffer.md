- PatternContextBufferHolder only acquired and released the optional regexp pattern context buffer and exposed buffer pointer plus size.
- YarrPatternConstructor received a VM stackLimit pointer and manually compared currentStackPointer() against it for parser recursion checks.
- Yarr JIT execute signatures passed freeParenContext and parenContextSize as separate trailing parameters and did not carry a stack-limit parameter.

## Moves

- 2020-03-26 (b9eeefcd) replaced by [[interpreter-dispatch]]: YARR JIT code needed an explicit stack limit because it can run from the compiler thread, so the stack limit and pattern context buffer were passed together in MatchingContextHolder instead of adding another raw execute parameter. (sourced)
