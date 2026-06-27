- Baseline prologue and op_loop_hint bodies are emitted inline into each compiled function.

## Moves

- 2021-06-07 (d76d00e3) replaced by [[unlinked-code-sharing]]: Moving Baseline JIT prologue and op_loop_hint bodies into shared thunks cut Speedometer2 LinkBuffer size from 188.379295 MB to 179.728931 MB with neutral Speedometer2 and JetStream2 performance. (sourced)
