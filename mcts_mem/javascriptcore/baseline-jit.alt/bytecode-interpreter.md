- Execution is a C++ switch-dispatch loop over bytecode instructions.
- No native code is cached per CodeBlock.
- Property access and opcode dispatch return to per-opcode C++ helpers.

## Moves

- 2008-09-07 (9b948e40) replaced by [[baseline-jit]]: The pure software interpreter (privateExecute) was replaced by CTI (Call-Threaded Interpreter / JIT), which compiles bytecode to native x86 machine code at first execution, enabling direct hardware dispatch and inline property-access caches instead of per-opcode C++ dispatch. (code)
