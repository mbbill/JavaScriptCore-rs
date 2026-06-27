- Old restore path called clearParenContextHeadSlotsInRange after restoreParenContext to null Greedy/NonGreedy parenContextHead slots in the restored frame range.
- Old NestedAlternativeEnd had a null returnAddress fallback for Greedy/NonGreedy restored states and FixedCount incomplete contexts were skipped without freeing to avoid stale outer snapshots.

## Moves

- 2026-04-23 (282d55d7) replaced by [[jit-codegen]]: FixedCount ParenContext backtracking changed from restoring snapshots then clearing potentially stale parenContextHead slots to marking the current context incomplete and reusing or freeing incomplete contexts, which bounds FixedCount{N} to at most N ParenContext allocations without restoring stale freed heads. (code)
