- Relational operators shared one relation helper returning -1, 0, or 1.
- Each relational AST node converted the tri-state result into its own boolean branch.

## Moves

- 2007-10-24 (699e8f3c) replaced by [[builtin-objects]]: The shared relation() function returned a tri-state int (-1/0/1) that each caller converted via branching on -1 into bool; replacing with two inline boolean functions (lessThan/lessThanEq) eliminates the tri-state encoding and per-call branching for a measured 0.5-0.6% SunSpider speedup. (sourced)
