- The Baseline JIT compiles bytecode to native code as the first JIT tier, preserving bytecode order and using per-opcode templates rather than an optimizing intermediate representation.
- Compilation is split into a main fast-path pass, a jump/link pass, and a slow-case pass; bytecode operations that can fail record slow-case jumps and patch them to out-of-line handlers.
- Generated code keeps bytecode-index correspondence for profiling, exception delivery, OSR, and tier handoff rather than erasing the interpreter view of execution.
- Baseline code owns inline feedback for calls, property access, math operations, and value/array profiles, while higher tiers consume that feedback rather than rebuilding it from scratch.
- Baseline shares the LLInt/JIT ABI, register conventions, and executable-memory machinery with the rest of JSC's JIT stack.

## Facts

- 2008-09-21 (3f085549) rationale: the CTI slow-case helper name changed from emitJumpSlowCaseIfNotImm to emitJumpSlowCaseIfNotImmNum because the guard only tested the integer-immediate tag bit, not all immediate values. (sourced)
- 2011-07-06 (c055f488) pitfall: the 32/64-bit call opcode path must append call structure-stub compilation info and mark the site as a call just like the 64-bit path. (code)
- 2015-02-13 (929e0b42) pitfall: the baseline prologue stack check must sit outside the FunctionCode-only prologue so ProgramCode and EvalCode also validate the computed stack pointer before installation. (code)
- 2016-01-05 (6ebc10cf) rationale: ResultProfiles must be creatable at runtime from slow paths rather than only in bytecode order during baseline compilation. (code)
- 2015-10-22 (ed1af54f) pitfall: native-thunk exception handling must pass the current call frame to operationVMHandleException, not the caller frame, because native frames may have no CodeBlock. (code)
- 2021-10-30 (24e25cc7) rationale: JSVALUE32_64 baseline DataIC support required abstracting JSValue registers and ideal ABI arguments so call and property opcodes could share the DataIC implementation. (sourced)
- 2021-05-24 (a60abb99) rationale: JITThunks uses a recursive lock because a thunk generator may request other thunks while running, and the thunk dependency graph is expected to be a DAG. (sourced)

## Moves

- 2008-09-07 (9b948e40) replaced [[bytecode-interpreter]]: The pure software interpreter (privateExecute) was replaced by CTI (Call-Threaded Interpreter / JIT), which compiles bytecode to native x86 machine code at first execution, enabling direct hardware dispatch and inline property-access caches instead of per-opcode C++ dispatch. (code)
