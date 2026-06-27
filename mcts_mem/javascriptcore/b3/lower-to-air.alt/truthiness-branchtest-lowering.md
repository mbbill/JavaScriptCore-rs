- Branch(value) lowered as a truthiness test against zero.
- Check(value) selected only simple width-keyed BranchTest CheckSpecials.
- Relational comparison fusion, operand flipping, and sub-32-bit load comparison branches were not represented.

## Moves

- 2015-11-05 (95975288) replaced by [[lower-to-air]]: Truthiness-only BranchTest lowering could not represent fused relational comparisons, operand/condition flipping, sub-32-bit load comparison branches, or keyed CheckSpecials for arbitrary fused branch opcodes. (code)
