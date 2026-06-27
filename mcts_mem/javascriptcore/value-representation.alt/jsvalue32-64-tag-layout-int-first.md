- Int32Tag and CellTag occupied the highest tag values.
- NullTag and UndefinedTag were lower separate tags.
- Null-or-undefined checks needed two equality tests or equivalent combined branching.

## Moves

- 2010-10-27 (1de93bac) replaced by [[value-representation]]: Placing NullTag (0xffffffff) and UndefinedTag (0xfffffffe) as the two highest unsigned tag values lets op_jeq_null / op_jneq_null be compiled as a single AboveOrEqual/Below unsigned comparison instead of two equality checks ORed together, reducing 4 instructions to 1 on ARM 32-bit. (sourced)
