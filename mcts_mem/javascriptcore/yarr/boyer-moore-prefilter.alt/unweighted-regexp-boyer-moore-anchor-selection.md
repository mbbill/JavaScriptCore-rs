- findBestCharacterSequence scored each candidate sequence as (sequence length) * (BoyerMooreBitmap::mapSize - bitmap count).
- findWorthwhileCharacterSequenceForLookahead tried candidate-count limits 4, 8, and 16, and rejected only when the best point was zero.
- The old code explicitly treated all characters equally and carried a FIXME to weight characters differently.

## Moves

- 2023-02-10 (d43e2a23) replaced by [[boyer-moore-prefilter]]: Subject sampling was chosen because rare anchors make Boyer-Moore search effective while frequent anchors can make the extra search code slow. (sourced)
