- RegExpGlobalData owns the mutable output vector for normal matching.
- RegExp::matchInline resizes a caller-supplied vector before executing YARR code.
- StringReplaceCache copies the global output vector when caching match state.

## Moves

- 2026-02-03 (e9964c3c) replaced by [[match-results]]: RegExp can determine the needed offset-vector size, so normal VM-thread matching reuses a vector owned by the RegExp instead of allocating or moving per-match vectors, while concurrent matching still receives caller-provided storage. (code)
