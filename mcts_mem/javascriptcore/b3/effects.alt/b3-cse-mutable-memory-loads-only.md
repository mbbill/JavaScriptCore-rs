- MemoryValue carried heap range, fence, and control-dependence metadata for memory CSE.
- CSE invalidated every cached MemoryValue whose range overlapped an intervening write.

## Moves

- 2025-09-22 (570a3530) replaced by [[effects]]: The old memory-effect model could only invalidate prior loads on overlapping writes, while the new Mutability bit represents loads whose result is stable across clobbers and lets CSE keep them. (code)
