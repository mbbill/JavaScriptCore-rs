- Active execution state is tracked through per-global currentExec and savedExec links.
- Marking follows callingExec and savedExec chains from global objects.
- Reentrant execution relies on global-object-owned current execution pointers.

## Moves

- 2008-01-22 (f3e70746) replaced by [[call-frame-layout]]: The per-global-object currentExec/savedExec linked-list mechanism caused crashes (including Amazon.com regression) because it failed to correctly track ExecState across multiple JSGlobalObjects and reentrancy; an explicit process-wide Vector<ExecState*,16> stack fixes ownership and enables correct GC marking of all active frames. (sourced)
