- WebAssembly.asm owns wasm calling-convention constants, wrapper entries, trampoline routines, and SIMD prologue routines separately from InPlaceInterpreter.asm.
- WasmCallingConvention keeps LLInt-specific callee-save counts beside IPInt counts.

## Moves

- 2025-08-27 (9104872c) replaced by [[interpreter-tier]]: The separate WebAssembly.asm path was folded into InPlaceInterpreter.asm because only IPInt remained available and keeping a separate wasm LLInt file left duplicate or slightly divergent definitions. (sourced)
