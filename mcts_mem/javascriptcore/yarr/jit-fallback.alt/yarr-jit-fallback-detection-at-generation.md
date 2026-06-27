- m_generationFailed bool field in RegexGenerator
- generationFailed() query method on RegexGenerator
- TypeBackReference case sets m_generationFailed=true in generateTerm
- TypeParenthesesSubpattern with count>1 or isCopy sets m_generationFailed=true in generateTerm
- jitCompileRegex calls generator.compile() then checks generator.generationFailed() to decide fallback

## Moves

- 2010-04-14 (d122b0d1) replaced by [[jit-fallback]]: Detecting unsupported regex features (back-references, multi-quantifier subpatterns) during JIT code generation wasted partial JIT work; moving detection to the regex compiler lets jitCompileRegex skip RegexGenerator entirely for patterns that must fall back to PCRE. (code)
