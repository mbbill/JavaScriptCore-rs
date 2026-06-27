- WebAssembly memory accesses always form checked addresses through explicit bounds-check nodes. (`WasmBoundsCheckValue`)
- Memory allocation maps only the currently valid byte range plus protected guard state.
- Compiled code is not separated by signaling-versus-bounds-checking memory mode.

## Moves

- 2017-03-03 (20b7da21) replaced by [[memory-model]]: Fast memories reserve 2^32 plus offset virtual address space and rely on signal-handled trapping loads/stores so WebAssembly memory accesses can omit explicit bounds checks in Signaling mode. (code)
