- JavaScript execution uses a private reserved JSStack separate from the native thread stack.
- Call entry stubs receive a top-of-stack pointer and lay out CallFrames inside the private reservation.
- Stack limit checks, sanitization, and conservative roots use JSStack-specific bounds.

## Moves

- 2014-01-29 (a3ac51de) replaced by [[call-frame-layout]]: The old private JSStack representation could not make non-LLInt-C-loop execution use the native thread stack while still estimating stack usage, sanitizing stack memory, and checking VM stack limits from the thread stack origin. (code)
