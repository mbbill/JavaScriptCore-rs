- The CTI runtime helper initializes new call-frame fields and returns one pointer plus a hidden second result slot.

## Moves

- 2008-10-07 (a00bca94) replaced by [[platform-calling-convention]]: Initializing the new call frame (CodeBlock, ScopeChain, CallerRegisters, ArgumentCount, Callee, OptionalCalleeArguments) inside the C++ runtime helper required storing those fields through ARG_setR and returning the new ctiCode pointer as a void*, with the second result (new r pointer) passed through a hidden CTI_ARGS slot (CTI_ARGS_2ndResult); emitting the frame-init stores as inline JIT instructions and returning both ctiCode and the new r as a VoidPtrPair struct eliminates the hidden-slot round-trip and lets the JIT place r directly in edi without a memory reload. (code)
