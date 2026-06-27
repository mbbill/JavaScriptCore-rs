- Compare fusion required the comparison to be internal to a single use.
- Shared comparisons were materialized as boolean temporaries.
- Fused comparisons committed internal loads and allowed load-promise fusion.

## Moves

- 2015-12-14 (776ca177) replaced by [[lower-to-air]]: B3-to-Air compare/branch fusion now duplicates shared comparisons because testing a previously materialized boolean is usually less efficient than redoing the fused compare, but it refuses load fusion once a shared comparison is involved because duplicating loads is wrong and inefficient. (sourced)
