- Block signatures store a function-signature pointer even for inline single-result blocks. (`BlockSignature`)
- Inline value-type block results allocate synthetic function signatures and retain generated type definitions.

## Moves

- 2026-01-15 (5ad2efd1) replaced by [[wasm]]: Block signatures that are just a single result type no longer need synthetic FunctionSignature/TypeDefinition allocation or lookup because BlockSignature can now store either a module FunctionSignature pointer or the inline result Type directly. (code)
