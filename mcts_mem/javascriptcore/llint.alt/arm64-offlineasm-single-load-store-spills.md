- ARM64 and ARM64E offlineasm lower register spills and Wasm argument transfers as repeated scalar loads and stores.
- The offline assembler has no first-class pair load/store validation for adjacent spill slots.

## Moves

- 2022-06-23 (79eb5e92) replaced by [[llint]]: ARM64/ARM64E LLInt register spills and Wasm argument transfers can be encoded as ldp/stp pairs rather than repeated scalar ldr/str sequences when the offline assembler has explicit loadpair/storepair operations and pair-address validation. (code)
