- DFG exit feedback was stored on each linked CodeBlock.
- Cached sibling CodeBlocks did not share exit-site history.

## Moves

- 2018-01-13 (3abf574c) replaced by [[osr]]: Storing DFG exit profile data on UnlinkedCodeBlock lets all CodeBlocks backed by the same unlinked code, including those from the unlinked code cache, share OSR-exit feedback for earlier better compilation decisions. (sourced)
