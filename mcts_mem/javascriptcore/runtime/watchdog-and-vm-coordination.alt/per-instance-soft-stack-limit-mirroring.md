- The VM stores a soft stack limit and mirrors it into each Wasm instance.
- Updating stack limits iterates live Wasm instances under heap state.
- Wasm and LLInt stack checks treat soft-limit failure directly as stack overflow.

## Moves

- 2025-08-16 (37f1943f) replaced by [[watchdog-and-vm-coordination]]: StackManager mirrors replace per-instance soft-stack-limit updates so VMTraps can request stop-the-world by flipping trap-aware stack limits without needing VM apiLocks held by running mutators. (sourced)
