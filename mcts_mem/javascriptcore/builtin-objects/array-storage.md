- Arrays begin in contiguous Butterfly-backed indexed storage and transition toward sparse or slow-put storage only when holes, accessors, huge indices, or indexed-property semantics require it.
- IndexingType encodes the selected indexed storage shape and copy-on-write state in the object header.
- Sparse/overflow arrays use ArrayStorage with integer-index side structures instead of the generic string-keyed property map.
- Array builtin fast paths may use memcpy, initialized holes, or direct contiguous access only when species, accessors, holes, and exceptions remain non-observable.

## Facts

- 2007-10-22 (27ab212b) rationale: ArrayStorage is a flexible-array allocation containing metadata, vector slots, and sparse-map pointer together, avoiding a separate metadata allocation. (code)
- 2010-08-03 (b4028c04) pitfall: Biased ArrayStorage pointers require retaining the original malloc base; recomputing it from index bias caused leak-detector-visible false leaks until the allocation base was stored explicitly. (code)
- 2016-02-11 (e0aca2f3) pitfall: Array.prototype.splice can initialize indexes directly only for arrays allocated by JSArray's uninitialized fast path; species-created results may be arbitrary objects and must use ordinary indexed puts. (code)

## Moves

- 2007-10-21 (0cf6079d) replaced [[array-sparse-via-property-map]]: Sparse array indices beyond sparseArrayCutoff were stored in the generic string-keyed PropertyMap requiring Identifier string conversion on every get/put; replaced with a dedicated HashMap<unsigned,JSValue*> keyed by integer index, yielding a 10% SunSpider speedup. (sourced)
- 2010-07-27 (9404802c) replaced [[jsarray-storage-pointer]]: JSArray changed from holding an ArrayStorage* pointer (requiring O(n) memmove of all elements for shift/unshift) to holding a JSValue* m_vector pointer directly to ArrayStorage.m_vector[0], plus an int m_indexBias field tracking pre-allocated JSValue slots before the ArrayStorage header, enabling O(1) memmove of only the header for shift and O(1) pointer-bump for unshift when bias space is available. (code)
- 2016-04-13 (1d8504dd) replaced [[array-concat-cpp-host-function]]: Supporting Symbol.isConcatSpreadable required Array.prototype.concat to perform spec-level observable property and species operations, so the host C++ concat was replaced by a JS builtin while DFG/FTL intrinsics and C++ memcpy helpers preserved fast paths. (code)
