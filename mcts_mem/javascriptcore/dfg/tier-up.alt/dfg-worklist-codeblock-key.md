- DFG worklist plans were keyed only by CodeBlock.
- Compilation-state queries could not distinguish replacement and OSR-entry modes for the same code block.

## Moves

- 2013-08-29 (3aa21ae9) replaced by [[tier-up]]: A single worklist is reused for DFG, FTL-replacement, and FTL-OSR-entry compilations; keying by CodeBlock* alone cannot distinguish two concurrent plans for the same block with different compilation modes, causing false positive 'already compiling' checks and dropped OSR-entry triggers. (sourced)
