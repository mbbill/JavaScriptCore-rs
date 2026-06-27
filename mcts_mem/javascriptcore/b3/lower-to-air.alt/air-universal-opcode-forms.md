- Air opcode forms were unconditional.
- Generated validity and generation code had no per-form CPU guards.
- 64-bit opcodes and x86 memory-immediate forms appeared generally available.

## Moves

- 2015-12-14 (38688698) replaced by [[lower-to-air]]: Air opcodes and forms needed architecture masks so reflective queries and the instruction selector would reject unavailable architecture-specific address forms while keeping opcode names mentionable in C++. (sourced)
