- IsoSubspace allocated a MarkedBlock as its first backing store.
- Each object family using IsoSubspace paid at least one 16 KB block.
- IsoSubspace cell sets tracked block-contained cells only.

## Moves

- 2019-11-09 (e6dbb891) replaced by [[allocator]]: IsoSubspace previously required allocating a full MarkedBlock (16KB) even for object types instantiated rarely, imposing a minimum 16KB per type; adding a lower tier of up to 8 LargeAllocation cells per IsoSubspace avoids the MarkedBlock entirely for sparsely-allocated types, enabling IsoSubspace to be applied more aggressively across the object hierarchy with a measured 0.6% memory reduction on iOS. (sourced)
