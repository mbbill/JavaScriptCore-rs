- BBQ stack maps, callee state, and memory-import callee-group copying branch around IPInt-only metadata.
- Wasm validation and tiering can run with no required IPInt metadata substrate.

## Moves

- 2025-08-20 (0a2b0683) replaced by [[bbq-tier]]: IPInt metadata and callees became a required wasm validation/tier substrate even when execution immediately tiers to BBQ, so users of stack maps and callee state no longer branch on useWasmIPInt. (code)
