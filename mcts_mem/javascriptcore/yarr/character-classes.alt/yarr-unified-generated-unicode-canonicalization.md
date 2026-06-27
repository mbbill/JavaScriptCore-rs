- YarrCanonicalizeUnicode.cpp was a checked-in generated source containing both canonicalization ranges and unicodeCharacterSetInfo arrays.
- YarrCanonicalizeUnicode.js generated tables itself and included a canonicalizeUnicode path for Unicode ranges.
- Consumers included YarrCanonicalizeUnicode.h and selected CanonicalMode at lookup time, but the file naming and data source did not distinguish legacy UCS2 from ES6 Unicode data.

## Moves

- 2016-03-08 (1d5ebf95) replaced by [[character-classes]]: Unicode regexp canonicalization moved from a checked-in unified table to separate legacy UCS2 tables plus a build-generated Unicode table from Unicode CaseFolding.txt so Unicode-mode matching follows ES6 Canonicalize data independently of non-Unicode behavior. (sourced)
