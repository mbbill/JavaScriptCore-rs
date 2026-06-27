- Boyer-Moore maps store a 128-entry masked character bitmap.
- The map records whether any added character differed from its masked position.
- Small exact candidate sets are lost once merged into the masked bitmap.

## Moves

- 2021-08-07 (28852ca9) replaced by [[boyer-moore-prefilter]]: The masked 128-bit bitmap could say a mask was effective but could not preserve a small exact candidate set for unmasked character search, while the new representation carries up to two exact candidates and invalidates when the set grows larger. (code)
