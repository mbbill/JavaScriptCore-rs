- Every call site checks whether the callee needs a full scope chain.
- Scope-chain activation updates are paid dynamically by callers.

## Moves

- 2008-09-26 (8cd9e824) replaced by [[call-linking-stubs]]: The dynamic needsFullScopeChain check at each call site (scopeChainForCall / cti_vm_updateScopeChain) added per-call overhead for all function calls; replacing it with a dedicated op_init_activation opcode emitted only when needed means the cost is paid once in the callee prologue, yielding 0.5% SunSpider, 0.7% v8-bytecode, and 1.3% empty-call-bytecode speedups. (sourced)
