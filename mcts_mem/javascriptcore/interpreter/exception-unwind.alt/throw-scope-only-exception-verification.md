- Exception verification is modeled around ThrowScope only.
- Checking, clearing, and catching exceptions do not share one scoped protocol.
- Throw and catch paths duplicate discipline for pending-exception state.

## Moves

- 2016-09-07 (1f253c16) replaced by [[exception-unwind]]: Checking and clearing exceptions needed a common scoped protocol instead of a throw-only scope, so JSC replaced ThrowScope-only verification with ExceptionScope plus CatchScope and funnels throwException, clearException, and exception checks through scope objects. (code)
