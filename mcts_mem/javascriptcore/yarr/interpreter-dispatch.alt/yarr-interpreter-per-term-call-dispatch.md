- Matching dispatches through per-term match and backtrack functions.
- matchAlternative loops over terms and calls matchTerm until failure or the alternative ends.
- Parenthetical assertions save and restore input position through dedicated function calls.

## Moves

- 2009-04-24 (45451967) replaced by [[interpreter-dispatch]]: Per-term virtual-call dispatch (matchAlternative->matchTerm->typed match fns) replaced by a single inlined matchDisjunction with goto-based MATCH_NEXT()/BACKTRACK() macros to eliminate call overhead between terms. (code)
