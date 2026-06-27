- Every counted OSR exit incremented speculativeFailCounter and decremented speculativeSuccessCounter.
- Reoptimization was triggered only after speculativeFailCounter exceeded a large-fail threshold and desiredSpeculativeSuccessFailRatio * failCounter exceeded successCounter.
- Lack of prediction and ForceOSRExit sites terminated speculative execution with ExitKind::Uncountable rather than a distinct inadequate-coverage kind.

## Moves

- 2012-04-08 (7c57afe0) replaced by [[metadata-table]]: Inadequate-coverage OSR exits indicate code that may be profitably optimized after enough executions, so they are counted separately and trigger reoptimization by count rather than by the ordinary success/fail ratio. (code)
