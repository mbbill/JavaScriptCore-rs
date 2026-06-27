- Non-x86_64 platforms defaulted to a pointer-sized JSVALUE32 encoding.
- Immediate type information was packed into pointer tag bits.
- Dispatch paths depended on JSImmediate machinery for object conversion and prototype behavior.

## Moves

- 2009-08-02 (39566916) replaced by [[value-representation]]: JSVALUE32_64 (64-bit JSValue with type-tag in high 32 bits and payload in low 32 bits) became the default on all non-x86_64 platforms, replacing JSVALUE32 (immediate-encoded JSValue that bit-packs type into pointer tag bits), because 32_64 can represent all value types without special-casing immediates and enables a simpler value ABI. (sourced)
