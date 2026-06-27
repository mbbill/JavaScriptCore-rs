- Private name environments track separate IsUsed and IsDeclared bits.
- Parser use of a private name mutates the private-name scope stack instead of flowing through ordinary variable-use propagation.
- Used-but-undeclared private names are copied through a private-name-specific environment path.

## Moves

- 2021-07-01 (49338f8f) replaced by [[scope-chain-and-activation]]: Private-name uses are tracked through the ordinary usedVariables mechanism so parser backtracking, source-provider-cache restoration, and runtime scope-chain lookup use the same rollback and capture propagation model as variables. (sourced)
