- Move-related Tmps mapped to sets keyed by Inst pointers.
- Coalescing worklists and active moves used Inst identity.
- Source and destination Tmps were recovered from the move instruction's operands.

## Moves

- 2015-11-16 (5a3e4d8a) replaced by [[register-allocation]]: For coalescing, the allocator only needs the abstract source and destination Tmps of a move, so replacing Inst* identity with dense move indices enables array and bit-vector based sets. (sourced)
