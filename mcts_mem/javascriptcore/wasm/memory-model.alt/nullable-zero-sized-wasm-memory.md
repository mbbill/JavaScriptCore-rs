- Zero-sized Wasm memories expose a null memory base. (`Wasm::Memory`)
- JavaScript ArrayBuffer creation allocates a temporary caged byte for empty memories.
- Generated memory caging paths preserve null-tolerant handling.

## Moves

- 2023-01-02 (ef906728) replaced by [[memory-model]]: Zero-sized Wasm memory now has a non-null base pointer so frequent generated caging paths can pass mayBeNull=false and skip null handling. (code)
