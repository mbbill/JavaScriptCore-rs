- The JIT generator interleaves forward-match and backtrack code in one pass.
- Out-of-line backtracking uses IndirectJumpHashMap and ParenthesesTail callbacks.
- GenerationState owns backtrack records and indirect-jump maps while terms are emitted.

## Moves

- 2011-05-16 (8f351921) replaced by [[jit-codegen]]: The old single-pass generator interleaved forward-match and backtrack code inline, requiring IndirectJumpHashMap and ParenthesesTail callbacks for out-of-line backtrack emission; the new approach first compiles the pattern to a linear YarrOp sequence, then emits all forward code in one pass and all backtrack code in reverse order (so the common fall-through is optimized) in a separate pass via BacktrackingState. (code)
