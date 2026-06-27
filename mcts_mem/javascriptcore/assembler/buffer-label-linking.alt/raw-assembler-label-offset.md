- AssemblerLabel publicly stored a raw uint32_t offset with UINT32_MAX as the unset sentinel.
- Backend relocation, linking, call-return, and label-difference code read and wrote m_offset directly.

## Moves

- 2021-02-02 (48f29377) replaced by [[buffer-label-linking]]: AssemblerLabel now hides its offset behind accessors so ARM64E can store m_offset as a pointer-authenticated tagged integer while non-ARM64E keeps the raw uint32_t layout. (code)
