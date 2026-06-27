- emitConstruct performs prototype get_by_id in caller bytecode generator
- op_construct carries proto and thisRegister as extra operands
- JSFunction host-function check and this-object creation done in op_construct slow path
- NativeExecutable stores only m_function (no m_constructor)
- ctiNativeCall thunk used for both call and construct of host functions

## Moves

- 2010-05-24 (203ccb5c) replaced by [[instruction-format]]: Caller passing proto+thisRegister operands to op_construct could not support callee-side prototype lookup or a per-callee native-constructor thunk; moving this-creation into op_create_this planted in the callee enables NativeExecutable to carry a separate constructor NativeFunction and mirrors the call path already used for non-host functions. (sourced)
