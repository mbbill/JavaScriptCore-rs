- IPInt prologue OSR asks for a call-style replacement after running tier-up heuristics.
- The prologue can lock the CalleeGroup and look for a replacement while the current invocation is already executing.

## Moves

- 2025-09-02 (92f56d6e) replaced by [[interpreter-tier]]: Wasm compilation and installation finish on the compiler thread, so an IPInt prologue that is already running cannot discover a main-thread-finalized replacement the way JS prologue OSR can. (sourced)
