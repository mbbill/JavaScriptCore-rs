- Class creation passes static-value tables, static-function tables, callbacks, and parent class as separate arguments.
- Callback table shape has no single versioned struct envelope.

## Moves

- 2006-07-16 (c81ff31a) replaced by [[c-api]]: Packing all class-creation parameters into a single versioned struct (JSClassDefinition) enables ABI-stable forward migration and allows adding new fields (e.g., className) without breaking callers. (sourced)
