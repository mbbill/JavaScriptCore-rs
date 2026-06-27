- UnlinkedInstruction had a StringImpl* uid union arm, making captured-variable operands pointer-sized.
- BytecodeGenerator stored watchable variables as Vector<StringImpl*, 16> and emitted the StringImpl* directly as the third operand of op_captured_mov and op_new_captured_func.
- CodeBlock linking interpreted the third unlinked operand as StringImpl* uid, treated null as no watchpoint, and looked up the symbol table entry by that uid.

## Moves

- 2014-01-21 (b1b14ab9) replaced by [[codeblock-split]]: This makes UnlinkedCodeBlocks use 32-bit instruction streams again. (sourced)
