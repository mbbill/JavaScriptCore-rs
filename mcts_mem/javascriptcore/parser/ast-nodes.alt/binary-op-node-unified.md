- Binary operator node classes carried an operator field and branched inside evaluation.
- Multiplicative, additive, shift, and relational families shared generic node implementations.

## Moves

- 2007-10-23 (a10d7639) replaced by [[ast-nodes]]: Unified nodes carrying a runtime oper char/enum field branch on the operator in every evaluate() call; splitting into dedicated classes eliminates those branches and allows each evaluate() to be a direct inlinable implementation, yielding a measured 0.8-1.0% SunSpider speedup even before further optimization. (sourced)
