- Non-ASCII Wasm names are validated by converting UTF-8 into a temporary UTF-16 buffer before storing the original bytes.
- The parser computes and then discards a UTF-16 representation solely for validity checking.

## Moves

- 2024-04-28 (f5b6bcdb) replaced by [[parser-validation]]: Wasm name validation uses checkUTF8 because it is more efficient than converting non-ASCII UTF-8 into a UTF-16 buffer and then discarding the buffer. (code)
