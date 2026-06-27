- Opaque-root writes used a mutator barrier that inserted roots into Heap::m_opaqueRoots.
- Weak-set constraint execution merged opaque roots recorded by visitors before visiting weak sets.
- The heap cached mutatorShouldBeFenced state for opaque-root barrier decisions.

## Moves

- 2017-01-18 (0d9d5577) replaced by [[heap]]: Opaque roots can change when visitChildren changes its mind and they have no write barriers, so JSObject-to-OpaqueRoot edges must be evaluated as output constraints participating in the marking fixpoint rather than as mutator-barriered roots. (sourced)
