- UnlinkedCodeBlock::RareData stored Vector<WriteBarrier<RegExp>> m_regexps.
- BytecodeGenerator::emitNewRegExp encoded an index returned by addRegExp(regExp) into op_new_regexp.
- LLInt, JIT, and DFG consumers recovered RegExp objects through CodeBlock::regexp(index).
- UnlinkedCodeBlock::visitChildren separately marked m_rareData->m_regexps.

## Moves

- 2018-07-09 (2256116f) replaced by [[codeblock-split]]: RegExp no longer needs a special RareData vector because JSCells can reside in the bytecode constant buffer. (sourced)
