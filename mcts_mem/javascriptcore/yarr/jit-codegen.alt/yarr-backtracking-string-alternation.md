- Anchored literal alternations use generic nested-alternative opcodes.
- Failed comparisons accumulate backtracking jumps.
- Successful alternatives jump to a shared end through the continuation path.

## Moves

- 2025-02-24 (5d4feeae) replaced by [[jit-codegen]]: Anchored non-capturing alternations of fixed strings can stop at the first matched string and jump on match, eliminating the usual continuation-PC backtracking path. (code)
