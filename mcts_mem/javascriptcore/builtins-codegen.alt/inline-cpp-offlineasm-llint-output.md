- Offlineasm emitted C++ inline assembly fragments bracketed by offlineasm macros.
- Windows x86 output used inline-assembly assumptions rather than standalone MASM input files.

## Moves

- 2014-02-15 (bb19dd1f) replaced by [[builtins-codegen]]: Windows LLInt adopted standalone MASM-compatible Intel-syntax assembly output instead of inline C++ assembly so Microsoft assembler builds could process it and the path could support 64-bit. (sourced)
