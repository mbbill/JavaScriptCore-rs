- ARM64 branch lowering used generic compare/test branch forms.
- Chained BitAnd/BitOr comparisons were represented as materialized booleans or extra control flow.
- NZCV flags were not carried across chains of compare Values.

## Moves

- 2026-01-12 (2cd6a734) replaced by [[lower-to-air]]: ARM64 conditional-compare chains avoid the materialized boolean/control-flow shape for BitAnd/BitOr compare chains by preserving NZCV flags through CompareOnFlags, CompareConditionallyOnFlags, and BranchOnFlags. (code)
