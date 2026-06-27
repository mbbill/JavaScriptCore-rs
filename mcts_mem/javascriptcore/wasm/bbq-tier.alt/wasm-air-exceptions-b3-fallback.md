- BBQPlan forces B3 compilation for functions that use Wasm exceptions.
- Air exception parser hooks are stubs or unreachable assertions rather than native try/catch lowering.

## Moves

- 2022-01-08 (0c3a8460) replaced by [[bbq-tier]]: Air gained native try/catch/throw/rethrow/delegate lowering, so BBQ no longer forces B3 for functions that use Wasm exceptions. (code)
