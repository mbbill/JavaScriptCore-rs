- `OpaqueJSString` stores characters in a manually allocated 16-bit buffer.
- All public strings pay 16-bit storage and manual memory-management costs.

## Moves

- 2012-10-03 (40a1198a) replaced by [[c-api]]: OpaqueJSString stored a manually heap-allocated UChar* buffer which always forced 16-bit storage even for 8-bit strings; replacing it with a WTF::String member preserves 8-bit string encoding and eliminates manual memory management (new[]/delete[]). (sourced)
