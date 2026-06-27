- Rope resolution threaded a soft stack limit through recursive buffer-filling calls.
- A slow path handled recursion when the current stack pointer crossed the limit.

## Moves

- 2024-12-16 (f941f5eb) replaced by [[rope-string]]: Rope resolution adopted signature-matched MUST_TAIL_CALL recursion instead of carrying a soft stack limit because the fast path's recursive calls are intended to be tail calls. (sourced)
