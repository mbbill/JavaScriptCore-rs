- Register::jsValue returns the stored JSValue pointer without execution-state context.
- Argument list accessors read register values without a way to allocate or box on demand.

## Moves

- 2008-07-23 (77e10c97) replaced by [[call-frame-layout]]: The old Register::jsValue() signature cannot support on-the-fly JSValue* creation when a register stores a raw double; requiring an ExecState* allows a future implementation to box the double into a heap-allocated JSValue* on demand, which the callee-side had no way to do before. (sourced)
