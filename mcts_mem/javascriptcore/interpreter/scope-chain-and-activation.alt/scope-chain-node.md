- Each JSScope object is paired with a separate ScopeChainNode JSCell.
- The wrapper stores next scope, object, global object, and global this pointers.
- Closure scope entry requires allocating both the scope object and its chain node.

## Moves

- 2012-08-30 (3d62590a) replaced by [[scope-chain-and-activation]]: ScopeChainNode was a separate JSCell wrapper (holding next/object/globalObject/globalThis pointers) that was paired with every JSScope object, requiring two heap allocations per closure scope entry; merging its data into JSScope eliminates the wrapper object and one allocation per scope. (sourced)
