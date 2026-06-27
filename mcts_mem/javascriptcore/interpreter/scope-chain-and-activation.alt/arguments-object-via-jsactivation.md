- Any function using arguments forces creation of a JSActivation object.
- The arguments object is created through activation state and stored as part of the full scope-chain object.
- usesArguments implies a full scope chain even when no lexical capture needs it.

## Moves

- 2008-10-01 (24a4e2f3) replaced by [[scope-chain-and-activation]]: Using 'arguments' in a function formerly forced creation of a JSActivation (full scope chain object); decoupling arguments storage into OptionalCalleeArguments call frame slot eliminates that cost and yields 19.1% on V8 Raytrace / 6.5% total V8 suite speedup. (sourced)
