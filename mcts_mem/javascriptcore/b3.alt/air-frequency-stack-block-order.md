- Non-rare successors were sorted by frequency and pushed onto one fast worklist.
- The same worklist continued fallthrough chains and chose the next block after a chain ended.
- Rare successors were deferred to a slow worklist and skipped if already emitted.

## Moves

- 2025-11-11 (01f76792) replaced by [[b3]]: The old ordering used one LIFO worklist for both fallthrough-chain continuation and broken-chain restart, while the new ordering separates those choices and injects explicit triangle, diamond, and exclusive-successor CFG layouts. (code)
