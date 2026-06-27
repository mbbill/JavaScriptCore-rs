- Tail-call slow paths prepare a separate frame and bulk-copy pointer-sized slots into the caller's frame.
- The slow path masquerades as the caller by restoring caller frame and return address before jumping.

## Moves

- 2015-09-18 (29d273d9) replaced by [[call-frame-layout]]: CallFrameShuffler carries per-argument ValueRecovery and frame-shuffle metadata so fixed-arity tail calls and polymorphic call stubs can rewrite the current frame instead of bulk-copying a prepared frame. (code)
