- AssemblerBuffer manually placement-constructed AssemblerData inside ThreadSpecific storage.
- DFG, JIT, and Wasm worklist shutdown hooks explicitly cleared assembler and LLInt thread-specific caches.
- Cache-clearing paths tested isSet() before manually destroying cached objects.

## Moves

- 2020-06-02 (946309b4) replaced by [[buffer-label-linking]]: Manual placement construction and thread-stopping clear hooks were removed because ThreadSpecific<T> constructs T on operator*/operator-> and runs destructors when the thread goes away. (sourced)
