- Named function expressions that need their name in scope emit bytecode to push the name on every execution.
- The dynamic non-strict-eval function-name scope predicate is carried through function executable metadata.

## Moves

- 2014-02-03 (82e9d20d) replaced by [[scope-chain-and-activation]]: We used to emit bytecode to push a name into local scope every time a function that needed such a name executed; now, we push the name into scope once on the function object, and leave it there. (sourced)
