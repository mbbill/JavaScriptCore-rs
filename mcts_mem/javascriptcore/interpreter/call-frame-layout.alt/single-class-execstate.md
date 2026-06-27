- One ExecState class represents global, program, eval, and function execution contexts through constructor overloads.
- Destruction branches at runtime to decide whether active execution state bookkeeping is needed.
- Context kind is stored as data rather than encoded in C++ type.

## Moves

- 2008-01-26 (d2ae8b8b) replaced by [[call-frame-layout]]: Single ExecState class with multiple constructor overloads required a runtime branch in the destructor to determine whether to manipulate activeExecStates; splitting into GlobalExecState/InterpreterExecState/EvalExecState/FunctionExecState encodes execution context kind in the C++ type and pushes lifecycle code to each derived destructor, eliminating the branch. (sourced)
