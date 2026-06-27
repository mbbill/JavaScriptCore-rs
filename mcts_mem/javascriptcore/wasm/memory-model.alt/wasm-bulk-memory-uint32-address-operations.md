- Bulk-memory helper operations expose 32-bit offsets and counts. (`Memory::fill`)
- Generated OMG bulk-memory calls zero-extend address and count values for the memory32-only path.

## Moves

- 2026-05-04 (c3893af6) replaced by [[memory-model]]: Memory64 bulk-memory support needs uint64 addresses/counts in shared helpers and JIT operations, while memory32 callers must explicitly zero-extend their 32-bit operands before using those widened call interfaces. (code)
