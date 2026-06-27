- Global declaration instantiation creates global function and var bindings while checking declarations.
- A failed later declaration check can happen after earlier bindings have already mutated the global object.
- Eval setup uses property lookup and put paths to materialize missing variable-object bindings.

## Moves

- 2023-09-05 (14f1a47b) replaced by [[scope-chain-and-activation]]: Global declaration instantiation needs separate CanDeclareGlobalX checks before any CreateGlobalXBinding side effect, because creating one binding before a later failed check is observable after catching the error. (sourced)
