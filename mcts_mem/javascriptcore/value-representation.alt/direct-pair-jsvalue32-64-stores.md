- Concurrently visible JSVALUE32_64 locations were updated by storing tag and payload directly.
- Readers could observe a mixed pair when hardware did not update the two words atomically.
- Value-profile consumers had to tolerate whichever tag/payload pair they observed.

## Moves

- 2024-01-31 (55815dc7) replaced by [[value-representation]]: Direct tag/payload stores were replaced for concurrent 32_64 JSValue locations because ARMv7 cannot atomically update the pair and tolerating spliced JSValues produced unusable observations in value profiles. (code)
