- B3 memory CSE reused a single MemoryValue only when it found a local match or a match in a block that strictly dominated the current block.
- The search gave up on overlapping intervening writes and cached one dominating match back into predecessor data.

## Moves

- 2016-01-22 (abd7613c) replaced by [[stack-slots]]: B3 CSE changed from requiring one dominating memory match to accepting a set of matches that cover all predecessor paths, using anonymous stack slots so FixSSA can synthesize the needed Phi graph. (code)
