- ArrayBuffer and typed-array length and offset APIs used uint32_t or Int32-sized compiler nodes.
- Optimized typed-array bounds checks assumed Int32-scale indices.

## Moves

- 2021-10-17 (a233fa74) replaced by [[typedarray-backing]]: Typed-array and ArrayBuffer lengths had to exceed the uint32_t/Int32 representation because WebAssembly memories can reach 4GB, which requires size_t storage plus Int52-specialized DFG/FTL nodes and profiling for large indices. (code)
