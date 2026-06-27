- ValueRep had 32_64-only RegisterPair kinds.
- Air Arg had a 32_64-only TmpPair kind.
- LowerToAir represented Int64 values as either one Tmp or a high/low Tmp pair.

## Moves

- 2024-09-07 (6c002efb) replaced by [[lower-to-air]]: Instructions like add64 that require a full int64 now extract their arguments from the Int64 input as if it were a tuple. (sourced)
