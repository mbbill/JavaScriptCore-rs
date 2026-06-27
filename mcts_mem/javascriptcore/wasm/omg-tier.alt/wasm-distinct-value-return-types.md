- Wasm value types and function return types are separate enum classes from B3::Type.
- Signatures store a distinct wasm return-type enum and wasm argument-type vector.

## Moves

- 2016-09-01 (dc14113f) replaced by [[omg-tier]]: WASM primitive types were made aliases of B3::Type so a Vector of WASM types can be converted to a Vector of B3 types without translation. (code)
