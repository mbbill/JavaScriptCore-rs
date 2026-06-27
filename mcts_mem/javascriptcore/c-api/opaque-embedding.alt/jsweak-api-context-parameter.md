- Weak-reference create, retain, and release operations require a concrete `JSContextRef`.
- The weak-reference API implies an execution context even though it only needs VM access.

## Moves

- 2017-05-11 (84a0d815) replaced by [[opaque-embedding]]: The JSWeak create/retain/release operations only need VM access and do not execute arbitrary JavaScript, so their API boundary should require a JSContextGroupRef rather than a JSContextRef. (sourced)
