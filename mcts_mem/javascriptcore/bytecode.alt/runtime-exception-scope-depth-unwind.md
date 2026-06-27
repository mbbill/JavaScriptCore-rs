- UnlinkedHandlerInfo stored a scopeDepth and HandlerInfo initialized it by adding nonLocalScopeDepth computed from the linked scope chain.
- Interpreter::unwind read handler->scopeDepth, adjusted for activation, computed scopeDelta = scope->depth() - targetScopeDepth, walked scope->next(), and wrote the result into the call frame scope register.
- BytecodeGenerator::calculateTargetScopeDepthForExceptionHandler derived a target local scope depth, with a special decrement for m_lexicalEnvironmentRegister.

## Moves

- 2015-08-07 (4588c578) replaced by [[bytecode]]: The bytecode generator knows every local scope it creates and can assign the correct catch scope directly, so the exception runtime no longer has to rediscover it by walking scope depth. (sourced)
