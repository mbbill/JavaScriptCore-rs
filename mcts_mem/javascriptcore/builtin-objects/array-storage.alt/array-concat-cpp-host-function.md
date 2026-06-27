- Array.prototype.concat was a C++ host function with JSArray-specific spread behavior.
- The only memcpy fast path was a host-side array fast path without full Symbol.isConcatSpreadable and species observability.

## Moves

- 2016-04-13 (1d8504dd) replaced by [[array-storage]]: Supporting Symbol.isConcatSpreadable required Array.prototype.concat to perform spec-level observable property and species operations, so the host C++ concat was replaced by a JS builtin while DFG/FTL intrinsics and C++ memcpy helpers preserved fast paths. (code)
