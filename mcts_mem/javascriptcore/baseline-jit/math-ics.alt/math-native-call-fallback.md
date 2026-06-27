- Math.floor, ceil, round, abs, exp, and log use the generic native-call path.

## Moves

- 2011-06-30 (a0ff9963) replaced by [[math-ics]]: Calling Math.floor/ceil/round/abs/exp/log through the generic native call path required boxing/unboxing and full C calling convention overhead; profiling on real web content showed these functions matter enough to justify dedicated thunks that fast-path integer arguments and use XMM registers directly, roughly doubling performance. (sourced)
