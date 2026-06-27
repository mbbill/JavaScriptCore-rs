- JSWebAssemblyInstance stores an instance-tail ImportFunctionInfo record per import.
- The table/export descriptor is wasm-only and cannot carry JS call-link information for jitless table or reference calls.

## Moves

- 2024-12-18 (602054ed) replaced by [[js-boundary]]: The jitless wasm-to-JS thunks need one descriptor that carries type index, import JS function, call-link info, target instance, entrypoint, and boxed callee data for both direct imports and table/ref calls, while the old instance-tail ImportFunctionInfo and wasm-only table entries could not represent JS call targets from tables or pass the descriptor through the non-JIT frame. (code)
