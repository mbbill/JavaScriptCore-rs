- The old PatternTerm representation had one BackReference type and one ForwardReference type, with named references resolved to subpattern numbers before compilation.
- The old interpreter and JIT applied m_duplicateNamedGroupForSubpatternId to any backreference subpattern id when duplicate named groups existed, redirecting explicit numeric references to the duplicate-name group.
- The old JIT treated ForwardReference as an always-empty match path rather than a compile failure.

## Moves

- 2026-03-20 (12ef604e) replaced by [[character-classes]]: Yarr split named and numbered backreference term types because duplicate-named-capture indirection is only correct for named references, while numeric references must keep targeting the explicitly numbered capture. (code)
