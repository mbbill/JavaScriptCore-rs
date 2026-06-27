- Range matching chooses a median range and emits recursive lower/upper branch trees.
- Literal ASCII matches are emitted as sequential equality branches with a separate ignore-case ASCII-letter path.
- Unicode matches and ranges are handled in a separate pre-ASCII path.

## Moves

- 2024-08-23 (0f30a2dc) replaced by [[character-classes]]: YarrJIT replaced range/match branch trees and ASCII-letter special casing with recursive grouping plus biased immediate bitset tests because clustered character classes can be matched with subtract/shift/branchTest instead of O(n) equality branches. (sourced)
