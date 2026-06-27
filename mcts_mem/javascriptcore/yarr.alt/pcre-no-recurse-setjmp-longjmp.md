- RMATCH macro: allocate heapframe via pcre_stack_malloc on every call
- setjmp(frame->Xwhere)==0 to save return address in jmp_buf
- longjmp(frame->Xwhere,1) in RRETURN to resume caller
- all heapframes heap-allocated (no stack pool)
- jmp_buf Xwhere field in heapframe struct

## Moves

- 2007-02-06 (49f7c83a) replaced by [[yarr]]: setjmp/longjmp in the PCRE NO_RECURSE heap-frame path caused a 25-30x regexp slowdown on macOS 10.5 vs 10.4; Shark profiling identified setjmp overhead as the root cause; GCC computed-goto (&&label, goto *ptr) eliminates the setjmp save/restore entirely. (sourced)
