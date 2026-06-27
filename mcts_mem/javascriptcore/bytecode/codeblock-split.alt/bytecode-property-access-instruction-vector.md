- UnlinkedCodeBlock::m_propertyAccessInstructions: Vector<InstructionStream::Offset>
- BytecodeGenerator::emitGetById/emitPutById/emitResolveScope etc each called addPropertyAccessInstruction(offset)
- CodeBlock::finalizeLLIntInlineCaches: iterated m_unlinkedCode->propertyAccessInstructions(), dispatched on opcodeID with switch
- CachedCodeBlock: serialized/deserialized m_propertyAccessInstructions vector

## Moves

- 2019-09-09 (73a006a1) replaced by [[codeblock-split]]: The propertyAccessInstructions vector required explicit registration at each bytecode emit site and was easily missed (op_create_promise was missing its registration); the MetadataTable::forEach<Op> API can enumerate all metadata for a specific opcode directly without a side vector, removing a class of omission bugs. (sourced)
