- Int32Tag was placed at the highest unsigned tag value.
- NullTag and UndefinedTag were below other immediate tags.
- Null/undefined tests needed multiple comparisons rather than one unsigned range check.

## Moves

- 2010-10-27 (1de93bac) replaced by [[tagged-encoding]]: Placing NullTag (0xffffffff) and UndefinedTag (0xfffffffe) as the two highest unsigned tag values lets op_jeq_null / op_jneq_null be compiled as a single AboveOrEqual/Below unsigned comparison instead of two equality checks ORed together, reducing 4 instructions to 1 on ARM 32-bit. (sourced)
