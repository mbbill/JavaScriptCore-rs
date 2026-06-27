- EncodedJSValue was one machine pointer word.
- JSValue stored a JSCell pointer or an immediate bit pattern in the pointer-sized word.
- Non-integer doubles on 32-bit required JSNumberCell allocation.

## Moves

- 2009-07-30 (74d77a72) replaced by [[value-representation]]: The old single-pointer encoding on 32-bit could not store a 64-bit double directly as an immediate (only integers and special-value sentinels fit in 32 bits); the new 32+32 tag/payload union holds doubles natively and removes the need for heap-allocated JSNumberCell for every non-integer number on 32-bit. (code)
