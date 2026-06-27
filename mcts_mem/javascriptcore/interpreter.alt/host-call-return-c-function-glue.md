- Host-call return values are recovered through per-architecture C-function glue stubs.
- Native host-call slow paths store the result in VM state and return a JIT-tagged C function pointer.
- C_LOOP simulates the return handoff with a pseudo-opcode handler.

## Moves

- 2020-10-01 (b63a0f1b) replaced by [[interpreter]]: JIT-caging restricts JIT-related PtrTags to JIT code, so getHostCallReturnValue could not remain a C function tagged as a JIT entrypoint. (sourced)
