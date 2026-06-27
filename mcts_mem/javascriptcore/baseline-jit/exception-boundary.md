- Baseline throw and exception paths return explicit handler state via the shared JIT exception protocol.
- Exception unwinding materializes enough VM, call-frame, and code-origin state for native thunks, baseline frames, and higher-tier exits to resume or report correctly.
- Pointer-tag domains and thunk dependencies are kept separate; exception handlers, operations, and generated JIT code keep distinct call-target representations.

## Facts

- 2015-10-22 (ed1af54f) pitfall: native-thunk exception handling must pass the current frame to operationVMHandleException because native frames may have no CodeBlock and unwinding must tolerate that frame shape. (code)
- 2016-05-17 (1c02a90e) pitfall: ShadowChicken throw logging must start from genericUnwind's selected unwind frame because a stack-overflow current frame can have a CodeBlock but no valid scope. (sourced)
- 2020-10-06 (7be05685) rationale: OperationPtrTag is reserved for C++ operation code, keeping JIT code and C++ code out of the same tagged-pointer representation. (sourced)
- 2021-05-12 (335209c0) pitfall: a thunk that needs another thunk must use existingCTIStub(..., NoLockingNecessary) after preinitialization rather than taking the JITThunks lock recursively. (sourced)
