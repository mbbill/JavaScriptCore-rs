- ENABLE_WREC conditional on PLATFORM(X86)&&PLATFORM(MAC), PLATFORM(X86_64)&&PLATFORM(MAC), PLATFORM(X86)&&PLATFORM(WIN)
- ENABLE_WREC=1 triggers ENABLE_ASSEMBLER=1
- YARR disabled with ENABLE_YARR 0 / ENABLE_YARR_JIT 0

## Moves

- 2009-04-24 (0f668258) replaced by [[yarr]]: YARR JIT (Yet Another Regex Runtime) was enabled by default on x86/x86-64 Mac and Windows, replacing WREC, after YARR reached sufficient correctness to pass tests on those platforms. (sourced)
