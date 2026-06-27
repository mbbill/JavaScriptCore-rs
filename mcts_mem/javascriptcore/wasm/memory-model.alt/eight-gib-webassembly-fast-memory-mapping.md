- Fast memory reserves an 8GiB virtual mapping that covers pointer maximum plus offset maximum. (`fastMemoryMappedBytes`)
- Signaling memory emits no explicit bounds checks in address preparation.
- Bounds-check values carry the pinned size register and offset without a redzone limit.

## Moves

- 2017-04-13 (6ffeeac1) replaced by [[memory-model]]: Fast-memory mappings shrank from 8GiB to 4GiB plus a configurable redzone; signaling mode relies on PROT_NONE for the 32-bit range and emits explicit WasmBoundsCheck only when register-plus-immediate accesses can exceed the redzone. (code)
