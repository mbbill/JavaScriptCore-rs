- With YARR_JIT_BACKREFERENCES enabled, patterns with backreferences still fell back for MatchOnly mode or for ignoreCase 16-bit character size.
- generateBackReference refused ignoreCase non-Char8 expressions by setting JITFailureReason::BackReference.
- The existing inline ignoreCase backreference comparison only used canonicalTableLChar after direct equality.

## Moves

- 2024-03-26 (9ff5c9b7) replaced by [[jit-fallback]]: The JIT could not inline canonical equivalence for non-ASCII 16-bit ignore-case backreferences with its fixed Yarr registers, so supported platforms call a thunk that saves caller-saves, remaps fixed registers to operationAreCanonicallyEquivalent, and returns the boolean in the character register. (code)
