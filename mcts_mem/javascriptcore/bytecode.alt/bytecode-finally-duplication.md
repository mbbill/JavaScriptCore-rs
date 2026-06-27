- FinallyContext stored either a StatementNode* finallyBlock or iterator-close data plus snapshots of control-flow, switch, for-in, try, label, lexical, finally-depth, and dynamic-scope state.
- emitComplexPopScopes walked m_controlFlowScopeStack, temporarily shrank/restored generator state, and emitted the saved finally block or IteratorClose block at each abrupt completion path.
- Return/yield/delegate-yield inside finally called emitPopScopes(scopeRegister(), 0) before emitReturn.
- TryNode emitted finally code on the normal path and again on the uncaught-exception path after popTryAndEmitCatch, while break and continue invoked emitPopScopes before jumping.

## Moves

- 2016-12-22 (c8db412b) replaced by [[bytecode]]: Completion-record threading replaced finally-body duplication because duplicated finally code caused exponential bytecode generation for deeply nested finallys while the new scheme emits each finally body once and dispatches on saved completion type. (sourced)
