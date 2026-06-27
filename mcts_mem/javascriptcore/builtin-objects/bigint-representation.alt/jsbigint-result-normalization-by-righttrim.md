- Arithmetic paths could allocate a JSBigInt temporary and then right-trim it to canonical length.
- Zero high digits were removed after allocation rather than before final object creation.

## Moves

- 2026-02-08 (304cf071) replaced by [[bigint-representation]]: BigInt arithmetic should normalize spans in temporary Vector storage and allocate the final JSBigInt once, rather than allocate a JSBigInt as a temporary buffer and then right-trim it. (code)
