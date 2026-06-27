- Register* k = codeBlock->registers.data() local in privateExecute
- op_load opcode: copies k[src] to r[dst]
- Vector<Register> registers field in CodeBlock
- addConstant(JSValue*) returning unsigned index
- k passed as parameter to unwindCallFrame and throwException
- k = codeBlock->registers.data() reset on call return

## Moves

- 2008-08-06 (09af41bc) replaced by [[codeblock-split]]: Emitting an op_load instruction for every constant use added instruction-dispatch overhead at each use site; pre-copying all constants into the register file once at function entry eliminates the per-use opcode entirely, yielding a 2.6% speedup on SunSpider. (sourced)
