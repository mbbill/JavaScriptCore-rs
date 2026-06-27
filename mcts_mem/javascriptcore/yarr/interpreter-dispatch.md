- The YARR interpreter is a flat dispatch machine over compiled byte terms and disjunctions rather than recursive per-term calls.
- Interpreter entry points are specialized by subject character width once the caller knows it.
- Interpreter state reports match, no-match, and limit/context-allocation failures explicitly rather than only returning a boolean.
- Parenthesis backtracking contexts are allocated from stack space and checked through matching-context stack limits rather than a fixed VM buffer.

## Facts

- 2009-04-28 (463dacc4) rationale: Bytecode compilation precomputes per-disjunction frame sizes so the interpreter can allocate and recycle backtracking context frames without per-term heap allocation. (code)
- 2009-05-01 (8f8daa0d) pitfall: Recursive interpreter dispatch had to preserve input position and parenthesis state across nested alternatives; the flat dispatch model makes those continuation points explicit in bytecode terms. (code)
- 2010-11-16 (728dc3f9) pitfall: The recursion-limit error must propagate through all interpreter call paths instead of collapsing into ordinary no-match. (code)
- 2018-07-03 (6ca10d5a) rationale: The interpreter carries explicit match-limit state so pathological backtracking returns an error result rather than consuming unbounded CPU. (code)
- 2017-12-12 (5ee39810) pitfall: ParenContext allocation pressure is part of matching correctness; allocation failure must return an explicit failure result rather than corrupting saved captures. (code)
- 2017-12-12 (5ee39810) rationale: The interpreter computes context frame sizes from bytecode and pattern shape so execution can allocate only the backtracking state each disjunction needs. (code)
- 2025-12-01 (a721886c) pitfall: Stack-based ParenContext allocation still needs entry paths to establish the expected stack pointer state before generated YARR code runs. (code)
- 2026-02-03 (eef70034) rationale: FixedCount ParenContext saves both begin and end indexes at ParenthesesSubpatternEnd because alternative retry needs the iteration begin index while quantified-content backtracking needs the iteration end index. (code)
- 2026-02-03 (eef70034) pitfall: Failed later fixed-count iterations can allocate Begin-created contexts that never reach End; backtracking must mark those contexts incomplete with matchAmount == -1 and skip them. (code)

## Moves

- 2009-04-24 (45451967) replaced [[yarr-interpreter-per-term-call-dispatch]]: Per-term virtual-call dispatch (matchAlternative->matchTerm->typed match fns) replaced by a single inlined matchDisjunction with goto-based MATCH_NEXT()/BACKTRACK() macros to eliminate call overhead between terms. (code)
- 2010-11-16 (728dc3f9) replaced [[yarr-interpreter-bool-return]]: The bool return type could only distinguish match/no-match and could not propagate a HitLimit error code when unbounded recursion was detected; switching to JSRegExpResult enum (JSRegExpMatch=1, JSRegExpNoMatch=0, JSRegExpErrorHitLimit=-2, etc.) allows the recursion depth counter (remainingMatchCount) check in matchDisjunction to return a distinct error value that propagates up through the call tree without using exceptions or global state. (code)
- 2012-03-29 (f3df9bfc) replaced [[yarr-interpreter-runtime-character-access]]: We should be able to call to the interpreter after having already checked the character type, without having to re-package the character pointer back up into a string! (sourced)
- 2020-03-26 (b9eeefcd) replaced [[yarr-vm-stack-limit-and-separate-context-buffer]]: YARR JIT code needed an explicit stack limit because it can run from the compiler thread, so the stack limit and pattern context buffer were passed together in MatchingContextHolder instead of adding another raw execute parameter. (code)
- 2025-12-01 (a721886c) replaced [[yarr-paren-context-vm-buffer-freelist]]: ParenContext allocation moved from a fixed VM buffer to the native stack so nested-parentheses context is limited by stack space instead of an 8192-byte buffer. (code)
