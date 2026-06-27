- MemoryValue, AtomicValue, and Air Arg constructors accepted int32_t offsets directly.
- Wasm lowering passed uint32_t offsets to MemoryValue APIs after rewriting only offsets larger than int32_t::max.

## Moves

- 2017-04-17 (7a86c519) replaced by [[lower-to-air]]: B3 adopted a signed-checked offset type boundary because implicit conversion of unsigned or oversized offsets into int32_t memory offsets could cause implementation-defined behavior. (sourced)
