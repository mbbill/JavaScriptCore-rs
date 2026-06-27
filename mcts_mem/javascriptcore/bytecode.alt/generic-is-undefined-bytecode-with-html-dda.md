- op_is_undefined / DFG IsUndefined was a reusable undefined-test bytecode and node whose JIT/LLInt lowering used masquerades-as-undefined watchpoint semantics.
- BytecodeGenerator::emitIsUndefined directly emitted OpIsUndefined, non-typeof language constructs inherited HTMLDDA undefined semantics.

## Moves

- 2020-07-17 (ed327a18) replaced by [[bytecode]]: Only typeof/equality/ToBoolean should see `[[IsHTMLDDA]]` as undefined, so the bytecode was renamed and confined to typeof while emitIsUndefined emits strict equality with jsUndefined. (code)
