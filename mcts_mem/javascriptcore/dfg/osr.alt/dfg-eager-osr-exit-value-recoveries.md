- Every OSRExit eagerly stored argument and variable ValueRecovery vectors.
- Speculative checks computed recovery data for all visible operands when the exit was created.

## Moves

- 2012-07-03 (403f771e) replaced by [[osr]]: The DFG now saves a variable event stream and minified graph so DFG::OSRExitCompiler can reconstruct recoveries lazily instead of computing argument and variable ValueRecoveries at every speculation check. (code)
