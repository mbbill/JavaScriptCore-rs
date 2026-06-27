- CharacterClass records non-BMP membership with a single boolean.
- Any class containing non-BMP characters takes a generic isBMP branch in generated code.
- Pure-BMP and pure-non-BMP classes are not distinguishable in the representation.

## Moves

- 2019-03-29 (71a9b2c0) replaced by [[character-classes]]: A single bool could not distinguish BMP-only, non-BMP-only, and mixed character classes, so the JIT always emitted the expensive generic isBMP branch; replacing with a 2-bit enum enables fixed-width advance code paths for pure-BMP and pure-non-BMP classes, eliminating the branch entirely. (code)
