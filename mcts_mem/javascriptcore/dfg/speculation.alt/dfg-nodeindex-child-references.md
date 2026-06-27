- Node children were raw NodeIndex values in fixed and vararg child storage.
- Edge-specific use information had to live outside the child reference.

## Moves

- 2012-02-06 (24882e71) replaced by [[speculation]]: DFG child edges needed to carry per-use type information without making DFG::Node larger. (sourced)
