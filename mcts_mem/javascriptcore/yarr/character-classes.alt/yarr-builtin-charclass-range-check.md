- Built-in classes are constructed from hand-written match and range vectors.
- Startup allocates CharacterClass objects for digit, space, word, and inverse built-ins.
- JIT membership tests use recursive binary-search comparison over ranges.

## Moves

- 2010-04-20 (7eaea7d0) replaced by [[character-classes]]: Hand-written range/match vector constructors and binary-search JIT dispatch replaced by Python-generated 256-byte bitmap tables and a single branchTest8 memory read, enabling O(1) character class membership tests for ASCII built-ins. (code)
