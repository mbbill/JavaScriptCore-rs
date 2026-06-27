- Prediction state was indexed by argument or local virtual register for the whole CodeBlock.
- All uses of the same virtual register merged into one prediction slot.

## Moves

- 2011-09-29 (f3283a0d) replaced by [[speculation]]: The old representation attached one prediction to each virtual register for the whole CodeBlock, while the new representation attaches prediction state to each variable access and unifies only aliased accesses, allowing distinct predictions for unaliased uses of the same virtual register. (code)
