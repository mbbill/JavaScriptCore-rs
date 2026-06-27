- Old range matching chose a median range, recursively emitted lower ranges, linearly emitted singleton matches below the chosen range, then tested <= hi before continuing above the range.
- Old single-range matching emitted two boundary branches, LessThan begin and GreaterThan end.
- Old class matching kept BMP and Unicode matches/ranges in separate paths and had an ASCII-case-folding special case for singleton matches.

## Moves

- 2024-06-27 (34b0b047) replaced by [[character-classes]]: Dense nearby character-class members are tested by subtracting the minimum and masking against an immediate bit vector instead of emitting O(n) equality branches or an all-or-nothing range binary search. (code)
