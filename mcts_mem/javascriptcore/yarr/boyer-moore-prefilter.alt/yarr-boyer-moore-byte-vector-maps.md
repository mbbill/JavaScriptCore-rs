- Boyer-Moore candidate maps are stored as byte vectors with one byte per map entry.
- YarrCodeBlock owns byte-vector maps and reuses them by equality.
- Generated code indexes the stored byte vector for candidate testing.

## Moves

- 2021-08-02 (77a1ed01) replaced by [[boyer-moore-prefilter]]: Bitmap-backed Boyer-Moore candidate maps were chosen because they were neutral on jquery-todomvc-regexp while being 8x smaller than byte vectors. (sourced)
