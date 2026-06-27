- op_new_arrow_func_exp had an extra operand for the current this register and JIT::emitNewFuncExprCommon passed that operand to operationNewArrowFunction.
- op_load_arrowfunction_this loaded JSArrowFunction::m_boundThis from the callee and stored it into the arrow function's local this register.
- BytecodeGenerator::emitNewArrowFunctionExpression performed a TDZ check on this before creating an arrow function in class constructors or empty-generator-this mode.

## Moves

- 2015-12-06 (2c4dd62d) replaced by [[instruction-format]]: The lexical-scope representation can carry this, new.target, and the derived constructor through arrow functions and eval using ordinary scope loads/stores, while the old JSArrowFunction bound-this field only carried this at arrow function creation. (code)
