- Each linked CodeBlock owns its own generated Baseline machine code.

## Moves

- 2021-09-27 (bfd44c5c) replaced by [[unlinked-code-sharing]]: Baseline machine code is generated against UnlinkedCodeBlock and per-CodeBlock state is loaded through a linked constant pool so all CodeBlocks for the same UnlinkedCodeBlock can share the compiled code. (code)
