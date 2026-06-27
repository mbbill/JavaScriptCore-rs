- Typed array views carry a mode byte recording whether vector storage is fast, oversize, ArrayBuffer-backed, resizable, or growable-shared.
- Direct views store their vector pointer on the view; ArrayBuffer-backed views use the Butterfly to retain the buffer.
- ArrayBuffer and typed-array storage pointers are caged and, for typed-array vectors, poisoned by element kind.
- Length, byte length, and offset are size_t-scale values supporting WebAssembly-sized backing stores.

## Facts

- 2015-10-16 (4ada3763) pitfall: TypedArray construction and set from another typed array must use the internal view length, not observable `[[Get]]` of `length`. (code)
- 2020-07-29 (02191369) statement: Fast and oversize typed-array vectors live in the Primitive Gigacage, while wasteful typed arrays and DataView do not own their backing memory. (code)
- 2022-11-09 (7a292520) rationale: Optimized tiers initially OSR-exit on resizable typed-array/data-view modes until profiling and ArrayMode support can safely specialize them. (code)
- 2022-11-18 (973ef455) rationale: Resizable ArrayBuffer reuses the WebAssembly.Memory and growable SharedArrayBuffer virtual-address resizing path. (sourced)

## Moves

- 2013-08-15 (93a48aa9) replaced [[typed-array-split-webcore-jsc-impl]]: Old design split typed array implementation between WebCore and jsc-shell (two incompatible versions), made arrays invisible to JIT, required 7 allocations per array (two JS objects, two GC weak handles, three malloc), and tracked native views rather than JS wrappers for neutering — making the common single-buffer/single-view case pay for a multi-view data structure. (sourced)
- 2016-03-21 (fa3ed404) replaced [[arraybuffer-nullable-create]]: ArrayBuffer allocation split into non-null create APIs that CRASH on allocation failure and nullable tryCreate APIs so OOM-capable callers must opt into the type that can represent failure. (code)
- 2017-08-31 (96c10153) replaced [[raw-arraybuffer-data-pointers]]: ArrayBuffer and typed-array storage pointers were represented as CagedPtr/CagedBarrierPtr so the Primitive Gigacage invariant is encoded in the pointer fields instead of being documented by FIXME comments on raw void* storage. (code)
- 2018-01-31 (323ad281) replaced [[typedarray-caged-vector-pointer]]: TypedArray vector storage was changed from a merely caged pointer to a per-JSType poisoned caged pointer so each TypedArray kind uses a distinct poison selected from a masked power-of-two table. (code)
- 2018-07-11 (91823615) replaced [[typed-array-impl-methodtable-dispatch]]: Central JSArrayBufferView dispatch by view type was sufficient without spending a MethodTable slot because getTypedArrayImpl was only overridden by typed arrays and DataView. (sourced)
- 2021-10-17 (a233fa74) replaced [[uint32-typed-array-sizing]]: Typed-array and ArrayBuffer lengths had to exceed the uint32_t/Int32 representation because WebAssembly memories can reach 4GB, which requires size_t storage plus Int52-specialized DFG/FTL nodes and profiling for large indices. (code)
- 2022-11-16 (fe4f0a4c) replaced [[typed-array-resizability-enum]]: Growable SharedArrayBuffer views and auto-length views needed mode states beyond the old resizable/non-resizable enum, while non-resizable and fixed-growable views still needed direct raw-field fast paths. (code)
