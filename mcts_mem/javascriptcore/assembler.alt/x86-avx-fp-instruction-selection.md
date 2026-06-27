- x86 floating-point MacroAssembler operations selected AVX encodings when CPUID reported AVX support.
- SSE encodings remained the fallback for non-AVX processors.

## Moves

- 2016-04-18 (375eee88) replaced by [[assembler]]: AVX was disabled because using it while other code is not careful with float register bits can make execution 10x slower, and a massive regression was seen on real code. (sourced)
