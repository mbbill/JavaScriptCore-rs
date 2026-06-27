- Yarr::Parser and Yarr::parse were unconstrained templates over Delegate.
- The required delegate callbacks were documented only as a long comment in YarrParser.h.
- YarrPatternConstructor and SyntaxChecker were accepted structurally without explicit static assertions that they satisfied the full parser callback contract.

## Moves

- 2025-02-20 (be68a2f3) replaced by [[pattern-analysis]]: The parser delegate contract was moved from an advisory comment to a C++ concept so missing callback methods become compile-time type errors. (code)
