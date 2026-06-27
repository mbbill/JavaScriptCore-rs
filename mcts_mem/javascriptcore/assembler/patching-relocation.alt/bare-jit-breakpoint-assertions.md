- JIT assertion failures emitted undifferentiated breakpoint instructions.
- The failing invariant had to be inferred from the stopped code address.

## Moves

- 2014-05-14 (626b6a5c) replaced by [[patching-relocation]]: Coded abort reasons make a JIT SIGTRAP diagnoseable from the platform abort-reason register instead of from only the trap address. (sourced)
