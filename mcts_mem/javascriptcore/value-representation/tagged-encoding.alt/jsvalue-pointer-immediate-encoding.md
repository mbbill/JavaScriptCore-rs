- JSValue fit in one pointer-sized word.
- Only integers and special sentinels were immediate on 32-bit.
- Non-integer numeric values were boxed in heap number cells.

## Moves

- 2009-07-30 (74d77a72) replaced by [[tagged-encoding]]: The old single-pointer encoding on 32-bit could not store a 64-bit double directly as an immediate (only integers and special-value sentinels fit in 32 bits); the new 32+32 tag/payload union holds doubles natively and removes the need for heap-allocated JSNumberCell for every non-integer number on 32-bit. (code)
