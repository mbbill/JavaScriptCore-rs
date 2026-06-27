- The allocator encoded every undirected interference edge into one global HashSet.
- Membership and insertion probed that hash table for each edge.
- Large functions concentrated peak allocator memory in the interference set.

## Moves

- 2021-05-19 (30a045cc) replaced by [[register-allocation]]: The allocator's interference graph moved from one global HashSet of encoded Tmp pairs to a bit-vector for small graphs and per-Tmp likely-dense sets for larger graphs to reduce the allocator's peak memory footprint. (code)
