- The O0 path created a separate spill slot for each non-register temporary.
- Register-backed temporaries also received spill slots through the same helper.
- Liveness was not used to recycle stack slots.

## Moves

- 2020-01-18 (be7e8364) replaced by [[register-allocation]]: Allocating a distinct spill slot for every Air Tmp produced huge O0 stack frames, while live-range-end reuse can assign dead Tmp slots to later Tmps. (sourced)
