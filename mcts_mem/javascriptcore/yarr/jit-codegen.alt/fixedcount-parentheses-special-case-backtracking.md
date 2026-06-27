- FixedCount parentheses use special matchAmount decrement and reentry cases to handle backtracking separately from other parenthesis quantifiers.

## Moves

- 2026-02-04 (4a3cd3e0) replaced by [[jit-codegen]]: FixedCount parentheses stopped distinguishing backtrackable from non-backtrackable cases and conservatively routes FixedCount groups through ParenContext/content-backtracking because the previous matchAmount decrement and m_reentry special cases were too error-prone. (code)
