- Profiler checks and callbacks are emitted inline in every JIT call and return path.
- Call and return fast paths pay profiler null checks even when profiling is disabled.

## Moves

- 2008-10-20 (026b0a87) replaced by [[call-linking-stubs]]: Inlining profiler null-checks and calls inside every JIT-compiled call/ret path imposed unconditional overhead on all callers even when no profiler was active; moving profiling to dedicated opcodes (op_profile_will_call, op_profile_did_call) emitted only when JSGlobalObject::supportsProfiling() is true gave a measured 22.2% speedup on empty-function-call benchmark and 2.9% on V8. (sourced)
