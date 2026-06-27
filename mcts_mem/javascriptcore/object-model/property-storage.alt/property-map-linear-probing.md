- PropertyMap used a single-step probe from the primary hash bucket.
- Colliding keys advanced by one slot under the table mask.
- The table had no secondary probe step to distribute clusters.

## Moves

- 2003-04-25 (f185265f) replaced by [[property-storage]]: Linear probing suffers from primary clustering when the table is heavily loaded; double hashing (step = 1 | (h % sizeMask)) distributes colliding keys more uniformly, yielding a measured 0.7% speedup on iBench JavaScript. (sourced)
