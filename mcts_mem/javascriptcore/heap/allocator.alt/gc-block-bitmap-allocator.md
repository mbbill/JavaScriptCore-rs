- CollectorBlock stored a uint32_t bitmap next to its fixed cell array.
- Allocation searched bitmap words and bits to find an unused cell.
- Sweeping scanned block bitmaps even after the block live count was already known.

## Moves

- 2002-11-24 (9f2a01bb) replaced by [[allocator]]: Replaced per-block bitmap (uint32_t array) with an embedded singly-linked free-list inside CollectorCell, giving O(1) allocation and a 3% iBench gain; also added firstBlockWithPossibleSpace cursor and early-exit sweep when live count reached. (sourced)
