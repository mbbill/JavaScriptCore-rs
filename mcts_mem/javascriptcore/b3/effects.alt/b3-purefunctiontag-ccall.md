- CCallValue had a PureFunctionTag overload for side-effect-free calls.
- Untagged C calls defaulted to Effects::forCall.
- PureFunction calls represented no reads or writes through a special constructor shape.

## Moves

- 2015-12-02 (af839507) replaced by [[effects]]: Filip prefers explicit effects. (sourced)
