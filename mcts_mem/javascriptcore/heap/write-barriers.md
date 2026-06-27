- Write barriers are explicit operations on stores that may create old-to-young or black-to-white references.
- Barrier entry points are specialized by use kind, including property stores, variable stores, storage-vector updates, and generic C++ or JIT-emitted stores.
- Generational collection records old objects or backing stores in remembered sets for Eden collection.
- Concurrent marking barriers fence stores that race with the collector, preserving the tri-color invariant when the mutator updates already-marked objects.
- JIT code emits the same logical barrier protocol as C++ code, with slow paths for barrier actions that cannot be encoded inline.

## Facts

- 2016-05-20 pitfall: a storage barrier must cover the backing store as well as the JSObject header, because a Butterfly can hold the only edge from an old object to a young cell (code).
- 2017-01-18 (0d9d5577) pitfall: opaque roots cannot rely on ordinary mutator write barriers, since their edges are produced by visitChildren output rather than by a direct store (sourced).
- 2020-07-08 rationale: typed barrier APIs let the JIT select property or variable barrier forms without duplicating the collector's remembered-set invariant in each stub (code).

## Moves

