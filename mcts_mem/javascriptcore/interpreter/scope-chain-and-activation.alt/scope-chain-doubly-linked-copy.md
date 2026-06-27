- ScopeChain is a doubly linked list copied in full when entering child scopes.
- Function calls copy the callee's scope chain before pushing an activation.
- Scope-chain nodes carry previous and next links plus separate refcount bookkeeping.

## Moves

- 2002-11-22 (2d8935af) replaced by [[scope-chain-and-activation]]: The doubly-linked ScopeChain required copying the full list when entering a function call scope (scope = func->scope().copy() then push activation); a singly-linked list with reference-counted shared tails eliminates all copy allocations because child scopes simply reference the parent tail, measured as 11% gain on iBench. (sourced)
