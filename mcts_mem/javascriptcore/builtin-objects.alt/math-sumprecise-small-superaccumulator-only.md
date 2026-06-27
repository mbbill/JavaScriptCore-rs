- Math.sumPrecise used one PreciseSum accumulator form for all iterable lengths.
- The implementation did not choose a larger superaccumulator only for large inputs.

## Moves

- 2025-06-12 (d95d6a80) replaced by [[builtin-objects]]: Math.sumPrecise chooses the large superaccumulator only for arrays longer than PRECISE_SUM_THRESHOLD because measured large-input speedups were about 1.11x while small inputs were slightly slower. (sourced)
