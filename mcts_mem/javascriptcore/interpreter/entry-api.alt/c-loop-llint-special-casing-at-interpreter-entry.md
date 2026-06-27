- C_LOOP LLInt entry is special-cased in Interpreter entry paths.
- Interpreter code pushes CallFrame objects, copies this and arguments, calls CLoop::execute with a prologue opcode, and pops the frame afterward.
- Executable JITCode storage is bypassed for C_LOOP helper entrypoints.

## Moves

- 2013-12-05 (b2ea0fe7) replaced by [[entry-api]]: C Loop LLINT was made to dispatch through Executable JITCode entries so it shared the same call/construct entry mechanism as the ASM LLINT and no longer needed Interpreter-level LLINT_C_LOOP branches. (code)
