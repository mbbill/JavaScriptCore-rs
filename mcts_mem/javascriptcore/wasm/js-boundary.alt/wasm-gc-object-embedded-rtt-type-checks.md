- Every WebAssemblyGCObjectBase stores an RTT pointer used by generated type checks.
- BBQ and B3 type checks load the object's RTT pointer and compare RTT display entries against the target RTT pointer.

## Moves

- 2026-03-27 (e40eb8ad) replaced by [[js-boundary]]: After realm-less interning made Wasm GC Structure identity unique per RTT, type checks could use object StructureID and an inlined StructureID display instead of an RTT pointer stored in every object. (code)
