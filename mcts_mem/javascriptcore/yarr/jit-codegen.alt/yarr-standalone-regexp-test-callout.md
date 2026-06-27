- YarrGenerator privately inherited MacroAssembler and emitted self-contained Yarr entry/exit code into YarrCodeBlock.
- DFG RegExpTest lowered to a JIT operation call such as operationRegExpTest or operationRegExpTestString rather than embedding Yarr matching code.

## Moves

- 2021-11-11 (b5429a89) replaced by [[jit-codegen]]: RegExp.test gained an inline DFG/FTL path by making Yarr emit into a caller-provided MacroAssembler with caller-selected registers, avoiding the C++ operation call for eligible 8-bit non-rope non-global/sticky non-Unicode cases. (code)
