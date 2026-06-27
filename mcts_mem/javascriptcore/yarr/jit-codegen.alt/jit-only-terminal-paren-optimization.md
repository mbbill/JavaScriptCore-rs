- terminal paren detection inside RegexGenerator::generateTerm in RegexJIT.cpp
- no BackTrackInfoParenthesesTerminal struct in RegexInterpreter.cpp
- no isTerminal flag on PatternTerm at the pattern level
- no RegexStackSpaceForBackTrackInfoParenthesesTerminal constant

## Moves

- 2010-11-29 (a4df4828) replaced by [[jit-codegen]]: Moving isTerminal detection from the JIT into the compiler's checkForTerminalParentheses() pass allows the interpreter to also exploit the no-backtrack optimization for greedy unbounded non-capturing parentheses at the end of a regex, and is required because frame-size allocation (which must account for terminal paren frame slots) happens at compile time, not JIT time. (code)
