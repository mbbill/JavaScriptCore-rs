- JS-to-wasm calls scan the signature for i64 arguments and returns at runtime before entering wasm.
- i64 mismatch throws from the runtime call-site stub.

## Moves

- 2018-11-19 (7522c43a) replaced by [[js-boundary]]: Moving I64 argument/return type check from the runtime call-site stub to the compiled JSToWasm wrapper encodes the check once at compile time rather than on every invocation, and is a prerequisite to removing callWebAssemblyFunction entirely. (code)
