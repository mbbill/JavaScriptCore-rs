- OSR exits stored exception-handler state as side booleans separate from the exit kind.
- Generic unwind and catch behavior were passed as separate flags into frame adjustment.

## Moves

- 2016-01-29 (6a41d375) replaced by [[osr]]: Exception-handling behavior needed to participate in exit-count and jettison policy, so encoding exception checks and generic-unwind arrivals as ExitKind values replaced separate per-exit boolean side flags. (code)
