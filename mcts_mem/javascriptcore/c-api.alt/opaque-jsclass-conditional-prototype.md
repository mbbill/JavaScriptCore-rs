- `OpaqueJSClass` creates a prototype only for classes with static functions or an explicit parent class.
- Classes without those triggers can lack a cached prototype object.

## Moves

- 2010-01-22 (56810cc5) replaced by [[c-api]]: Always creating a prototype class for every OpaqueJSClass (not just when staticFunctions or parentClass is present) ensures prototype chains are always correctly hooked up and instanceof works on all API classes, not just those with static functions. (sourced)
