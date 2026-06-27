- Interpreter owns builtins, prototypes, constructors, debugger, timeout state, and current execution pointers.
- JSGlobalObject is a thin client that reaches runtime state through its Interpreter pointer.
- JSContextRef creates the Interpreter before deriving global execution state.

## Moves

- 2007-12-06 (e2f9a746) replaced by [[entry-api]]: The Interpreter class held all runtime data (builtins, prototypes, constructors, currentExec, debugger, timeout state) with JSGlobalObject as a thin client; moving these into JSGlobalObject's JSGlobalObjectData struct eliminated a separate Interpreter indirection and fixed a bootstrapping bug where globalExec was used before Interpreter initialised the global object. (sourced)
