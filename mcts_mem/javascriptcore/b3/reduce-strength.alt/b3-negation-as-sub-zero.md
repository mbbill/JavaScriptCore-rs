- Integer negation built a zero constant and emitted Sub(0, value).
- Floating negation also used Sub(+0.0, value).
- Air lowering recognized Sub(0, x) for integer negation rather than a first-class B3 Neg opcode.

## Moves

- 2016-01-09 (0ad352bd) replaced by [[reduce-strength]]: For floating point, Sub(0, 0) produces +0 while true negation produces -0, and representing floating negation as BitXor(x, -0) would force clients to pattern-match different encodings for integer and floating negation. (sourced)
