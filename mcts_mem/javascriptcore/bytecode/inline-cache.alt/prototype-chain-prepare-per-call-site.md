- generateConditionsFor* traverses chain and flattens on MainThread path
- PolyProtoAccessChain::create traverses chain and flattens independently
- Concurrent path bails if dictionary encountered
- MainThread path checks hasBeenFlattenedBefore() and may return invalid

## Moves

- 2019-10-14 (9291a683) replaced by [[inline-cache]]: Inline caching could use a stale PropertyOffset after flattening an uncacheable dictionary because generateConditions* and PolyProtoAccessChain each flattened independently; a single preparePrototypeChainForCaching() function consolidates the walk so both paths see consistent offsets. (sourced)
