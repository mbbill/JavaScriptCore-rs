- VM owns a fixed 8192-byte regexp pattern context buffer guarded by a lock.
- MatchingContextHolder acquires the VM buffer only for code blocks that use pattern context.
- YarrGenerator prebuilds a freelist over the fixed buffer and fails when the freelist is exhausted.

## Moves

- 2025-12-01 (a721886c) replaced by [[interpreter-dispatch]]: ParenContext allocation moved from a fixed VM buffer to the native stack so nested-parentheses context is limited by stack space instead of an 8192-byte buffer. (code)
