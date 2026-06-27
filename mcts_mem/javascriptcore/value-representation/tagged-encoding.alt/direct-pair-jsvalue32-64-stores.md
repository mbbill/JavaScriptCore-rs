- JSVALUE32_64 stores wrote tag and payload words directly.
- Concurrent readers could observe a tag from one value and payload from another.
- Value-profile readers attempted to tolerate mixed observations without an explicit invalid state.

## Moves

- 2024-01-31 (55815dc7) replaced by [[tagged-encoding]]: Direct tag/payload stores were replaced for concurrent 32_64 JSValue locations because ARMv7 cannot atomically update the pair and tolerating spliced JSValues produced unusable observations in value profiles. (code)
