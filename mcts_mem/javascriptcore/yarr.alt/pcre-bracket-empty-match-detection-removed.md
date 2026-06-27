- MatchFrame.args.subpatternStart (single UChar* tracking bracket start)
- startNewGroup() saved args.subpatternStart into locals.subpatternStart
- KETRMIN/KETRMAX opcodes restored subpatternStart from locals
- no chain linkage — only one level tracked per frame

## Moves

- 2008-01-01 (81c47fd1) replaced by [[yarr]]: A prior optimization commit removed the eptrblock stack that tracked bracket-start positions to detect infinite loops on brackets matching the empty string; this caused jsRegExpExecute to return -2 on 34 test cases; the stack must be maintained to prevent infinite repetition of zero-length matches. (sourced)
