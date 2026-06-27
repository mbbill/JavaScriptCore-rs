- Grouped nearby singleton/range characters into a small immediate CharacterBitSet and tested membership with biased subtract, variable shift of 1, and branchTest against the mask.
- Fell back to recursive grouping of singleton matches when they did not fit within MaximumCharacterClassSizeForBitTest.
- Unified ASCII and Unicode matches/ranges before generating the bitset/range matching code.

## Moves

- 2024-07-27 (63dab201) replaced by [[character-classes]]: The bitset optimization path was reverted after matchCharacterClassSet sometimes crashed when using it. (sourced)
