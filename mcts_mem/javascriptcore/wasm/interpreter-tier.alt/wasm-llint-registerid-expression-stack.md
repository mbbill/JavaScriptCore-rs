- The Wasm LLInt generator expression stack stores retained RegisterID temporaries.
- Constants, locals, calls, and control results allocate temporaries before being mapped back to stack slots.

## Moves

- 2019-11-22 (56546744) replaced by [[interpreter-tier]]: The LLInt generator needed the parser expression stack to match virtual registers directly so constants and locals could avoid redundant temporaries while still being materialized at control-flow boundaries. (code)
