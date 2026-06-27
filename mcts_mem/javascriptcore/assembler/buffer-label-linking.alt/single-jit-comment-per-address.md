- LinkBuffer copied one comment string for each linked instruction address.
- Duplicate comments at the same machine-code address asserted instead of accumulating.

## Moves

- 2022-09-06 (ad2e13b2) replaced by [[buffer-label-linking]]: WASM BBQ disassembly needs to annotate one machine-code address with multiple compiler events rather than rejecting duplicate comments. (code)
