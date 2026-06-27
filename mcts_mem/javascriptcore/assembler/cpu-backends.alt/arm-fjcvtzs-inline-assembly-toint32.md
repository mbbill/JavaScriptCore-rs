- FJCVTZS-enabled ARM conversion used inline assembly to issue fjcvtzs into a general-purpose result register.

## Moves

- 2025-07-23 (0f857e32) replaced by [[cpu-backends]]: The compiler builtin was chosen because it existed and was cleaner than inline assembly for issuing the same ARM conversion instruction. (sourced)
