- FJCVTZS-enabled ARM conversion used the compiler-provided __builtin_arm_jcvt entry point.

## Moves

- 2025-07-24 (e4e85425) replaced by [[cpu-backends]]: The inline assembly implementation was restored because the __builtin_arm_jcvt builtin was not working well on macOS. (sourced)
