- CPU-specific assembler backends own raw instruction encodings and expose only the operations and capability predicates that the portable MacroAssembler can compose.
- ARM-family backends prefer direct instruction forms when the architecture supplies them, but keep fallback encodings where reach, immediates, or platform support require it.
- x86-family support is centered on x86-64 plus common x86 machinery; obsolete 32-bit or unmaintained backends are removed rather than kept as live targets.
- RISCV64 instruction helpers mirror typed instruction forms, turning invalid immediate and FP combinations into compile-time failures.

## Facts

- 2012-07-11 (cf8c44c1) measurement: copying ARMv7 LinkRecord entries with three explicit 32-bit word assignments rather than compiler-emitted memmove reduced ARMv7 link time by 19%. (sourced)
- 2014-03-10 (1b94e211) rationale: the x86 assembler special-cases accumulator forms such as xchg with eax so it can emit shorter accumulator opcodes instead of generic register/memory encodings. (code)
- 2014-04-10 (70f485c3) rationale: x86-64 code padding uses sized multi-byte NOP encodings up to 15 bytes instead of filling every byte with one-byte NOPs. (code)
- 2021-11-29 (66db9c06) rationale: RISCV64 assembler helpers mirror RISCV64 instruction classes and use immediate wrapper types, templates, and static assertions so invalid instruction forms are rejected at compile time. (code)
- 2021-11-29 (66db9c06) rationale: RISCV64 conditions are narrowed to branch forms viable on RISCV and ordered in inverse pairs so condition inversion is a constexpr XOR-by-one operation. (code)
- 2023-05-05 (7fe1a751) rationale: LinkRecord sorting uses an inline lambda at the std::sort call site because passing a named function pointer prevented the compiler from inlining the comparison. (sourced)
- 2024-08-09 (86d19af6) rationale: on platforms with FJCVTZS, typed-array double-to-integer conversion skips the speculative static_cast fast path because the direct FJCVTZS path is faster. (sourced)

## Moves

- 2009-11-05 (15b6e9ee) replaced [[arm-complex-immediate-pc-relative-pool]]: ARMv7 (ARM_ARCH_VERSION >= 7) supports MOVW and MOVT instructions that can load a 32-bit immediate in two 16-bit-immediate instructions without a PC-relative literal pool load, eliminating the need for genInt's two-instruction OR/MVN sequence or the ldr_imm pool fallback. (code)
- 2018-05-08 (b6ddc8dd) replaced [[mips-seven-temporary-gprs]]: MIPS DFG paths could use the generic register-hungry code once caller-save argument registers a0-a3 were admitted as temporary registers. (sourced)
- 2019-04-29 (9498e00f) replaced [[jit-stub-routine-set-hashmap]]: HashMap<uintptr_t,StubRoutine*> registered every 16-byte step of each routine, creating O(size/step) entries per routine and ~2MB table on Gmail; sorted Vector<{startAddress,StubRoutine*}> with binary search shrinks memory to O(count) entries at the cost of a sort before each conservative scan. (sourced)
- 2025-07-23 (0f857e32) replaced [[arm-fjcvtzs-inline-assembly-toint32]]: The compiler builtin was chosen because it existed and was cleaner than inline assembly for issuing the same ARM conversion instruction. (sourced)
- 2025-07-24 (e4e85425) replaced [[arm-jcvt-builtin-toint32]]: The inline assembly implementation was restored because the __builtin_arm_jcvt builtin was not working well on macOS. (sourced)
