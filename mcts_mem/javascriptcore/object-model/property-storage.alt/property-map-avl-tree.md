- PropertyMap used heap-allocated AVL nodes keyed by property name.
- Each node held value, attributes, child, parent, and height metadata.
- Insertions rebalanced the tree with rotations.

## Moves

- 2002-11-19 (fcfb139a) replaced by [[property-storage]]: AVL tree gave O(log n) property lookup with per-node heap allocation and pointer chasing; open-addressing hash table keyed on UString::Rep* gives O(1) average lookup with a single flat array allocation, yielding ~7% improvement on iBench. (sourced)
