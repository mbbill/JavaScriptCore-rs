- ARM Traditional JIT thunks push the return address below the JITStackFrame on the hardware stack.

## Moves

- 2009-10-26 (64bf77e0) replaced by [[platform-calling-convention]]: ARM Traditional JIT stored thunk return address by pushing it onto the hardware stack (below JITStackFrame), but JSValue32_64 support requires the return address to be at a fixed struct offset inside JITStackFrame (as ARM Thumb2 already did); commit message explicitly states this as a requirement. (sourced)
