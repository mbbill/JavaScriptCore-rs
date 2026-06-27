- additionalReadSizeSentinel was 0x4 and meant 'first read not yet classified'.
- BodyAlternativeBegin initialized firstCharacterAdditionalReadSize to the sentinel at the alternative reentry label.
- Unicode read paths conditionally changed the sentinel to 0 for BMP or to 1 for non-BMP and intentionally did not change it after the first read.

## Moves

- 2025-08-18 (d96ab2fa) replaced by [[character-classes]]: Sentinel-based first-read tracking could silently fail across skipped alternatives or backtracking control-flow changes, so each alternative now initializes the additional read size to 0 and non-BMP reads write 1 directly. (sourced)
