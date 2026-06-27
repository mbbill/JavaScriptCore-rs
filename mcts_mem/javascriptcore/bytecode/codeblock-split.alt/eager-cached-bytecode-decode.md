- UnlinkedFunctionExecutable stores m_unlinkedCodeBlockForCall and m_unlinkedCodeBlockForConstruct as plain WriteBarrier<UnlinkedFunctionCodeBlock> fields
- Decoder is stack-allocated (WTF_FORBID_HEAP_ALLOCATION) local to decodeCodeBlockImpl
- All nested UnlinkedCodeBlocks decoded immediately when executable is decoded from cache

## Moves

- 2019-03-07 (77eff8b8) replaced by [[codeblock-split]]: Eager decode of all UnlinkedCodeBlocks at cache restore time was replaced by storing byte offsets in a union with the WriteBarrier slots and decoding on first call to unlinkedCodeBlockFor, matching lazy-parsing's block-boundary pause strategy to avoid unnecessary work for never-called functions. (code)
