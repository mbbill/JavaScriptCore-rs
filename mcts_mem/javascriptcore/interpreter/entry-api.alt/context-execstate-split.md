- Context stores execution-context data, while ExecState wraps a Context pointer plus exception state.
- Context and ExecState are allocated together but represented as separate objects.
- Interpreter entry tracks the current Context independently from ExecState.

## Moves

- 2007-10-26 (f10a4fbc) replaced by [[entry-api]]: Context and ExecState were always created and destroyed together and carried redundant interpreter pointers; ExecState held only a Context* plus exception state while Context held all execution-context data (scope chain, activation, this value, code type, calling context); merging eliminates one allocation, one pointer indirection, and one pointer field per call frame. (sourced)
