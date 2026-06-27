- unsigned maxCharactersAtOnce = 4 (platform-independent)
- int/uint32_t allCharacters accumulator
- int ignoreCaseMask (32-bit)
- cases 1..4 only in switch; no check8

## Moves

- 2018-08-21 (4ce019d1) replaced by [[jit-codegen]]: On 64-bit platforms (X86_64, ARM64) a single GPR can hold 8 bytes; extending the character accumulator from int/uint32_t to uint64_t and adding a check8 lambda (load64/branch64) allows matching up to 8 characters in one compare, reducing code size by fusing multiple mov+cmp pairs into one. (sourced)
