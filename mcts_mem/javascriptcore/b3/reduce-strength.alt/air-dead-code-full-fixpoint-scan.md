- Air eliminateDeadCode scanned every block and every instruction in each forward/backward fixpoint iteration.
- Instructions already proven live were repeatedly processed.

## Moves

- 2017-04-05 (90667eef) replaced by [[reduce-strength]]: Tracking only instructions that might still be dead avoids repeatedly processing instructions already proven live during the eliminateDeadCode fixpoint. (code)
