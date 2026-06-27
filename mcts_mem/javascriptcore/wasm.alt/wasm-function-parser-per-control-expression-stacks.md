- Each active control entry owns expression-stack vectors for enclosed and else-block values. (`ControlEntry`)
- Entering and leaving nested control blocks moves or swaps stack vectors at the boundary.
- Stack-height queries sum the current stack with all enclosed control-entry stacks.

## Moves

- 2026-06-17 (16828c4b) replaced by [[wasm]]: The function parser replaced per-control-block expression-stack vectors with one contiguous expression stack plus begin offsets to avoid keeping N live vectors and copying/swapping stack slices at every nested control boundary. (sourced)
