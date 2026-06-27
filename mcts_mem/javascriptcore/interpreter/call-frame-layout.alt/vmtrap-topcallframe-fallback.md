- Trap installation falls back to sanitizedTopCallFrame when the sampled PC is not proven to be in JIT or LLInt code.
- The fallback may inspect call-frame state while the target thread is in C code.

## Moves

- 2018-02-14 (6be642a9) replaced by [[call-frame-layout]]: Trap installation may malloc, so it now refuses to run unless the sampled PC proves the thread is in JIT or LLInt code rather than falling back to topCallFrame while the thread may be in C code holding the malloc lock. (code)
