- Character classes split stored matches and ranges at the ASCII boundary.
- Char8 JIT handling builds temporary data by copying ASCII storage and clipping Unicode-side data to 8-bit.
- Any-character is represented as separate ASCII and non-ASCII ranges.

## Moves

- 2026-06-01 (bd24d557) replaced by [[character-classes]]: Yarr character classes changed their storage split from ASCII/non-ASCII to Latin1/non-Latin1 to match the engine's Char8/Char16 execution boundary and avoid copying/clipping Unicode-side data for 8-bit input. (code)
