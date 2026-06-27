- Scope chains are represented by the same List abstraction used for argument lists.
- Scope-chain operations inherit List refcounting and marking behavior.
- Context stores and returns scope chains as List values.

## Moves

- 2002-11-22 (f2cbe708) replaced by [[scope-chain-and-activation]]: List was used for scope chains but carries ref-counting and mark() overhead tuned for argument lists; separating ScopeChain allows independent optimization and removes needsMarking complexity from List; ScopeChain initially starts as a doubly-linked copy of List. (sourced)
