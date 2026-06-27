- Wasm floating min/max lowers to expanded comparison, select, bitwise, and NaN-preserving control flow.
- ARM64 cannot select native fmin/fmax from a first-class B3 min/max opcode.

## Moves

- 2021-12-16 (3a6ead11) replaced by [[omg-tier]]: Represent Wasm min/max as B3 FMin/FMax so ARM64 can select fmin/fmax while non-ARM64 lowers the same opcode to the old semantic control flow. (code)
