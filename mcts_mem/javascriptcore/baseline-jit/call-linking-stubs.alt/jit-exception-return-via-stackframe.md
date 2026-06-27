- Exception value and updated call frame are returned through stack-frame output slots.
- Call thunks reserve stack space for exception and call-frame write-backs.

## Moves

- 2010-10-27 (f36beff4) replaced by [[call-linking-stubs]]: Returning the exception value through a stackframe output slot (stackFrame.exception pointer) and the updated callFrame through stackFrame.callFrame required the call thunk ABI to reserve stack space for two extra write-back slots; consolidating to JSGlobalData::exception for the value and regT0 for the new callFrame pointer removes the output-parameter fields and unifies the exception-propagation path for both caught and uncaught exceptions. (sourced)
