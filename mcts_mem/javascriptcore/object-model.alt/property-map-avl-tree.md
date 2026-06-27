- PropertyMap stored entries in an AVL tree of heap-allocated PropertyMapNode objects.
- Lookup, insertion, and deletion walked or rotated node pointers.
- Enumeration traversed the tree in order through node links.

## Moves

- 2002-11-19 (fcfb139a) replaced by [[object-model]]: AVL tree gave O(log n) property lookup with per-node heap allocation and pointer chasing; open-addressing hash table keyed on UString::Rep* gives O(1) average lookup with a single flat array allocation, yielding ~7% improvement on iBench. (sourced)
