- JS-to-wasm calls convert raw wasm returns through a runtime switch over the signature return type.
- The call site expects tag-register state instead of emitting return boxing into the wrapper.

## Moves

- 2018-10-01 (d2545449) replaced by [[js-boundary]]: The return-type switch at call site interpreted the raw result at runtime using a runtime switch over signature.returnType(); since returnType() is known at compile time, the conversion can be emitted as JIT code into the JSToWasm glue, eliminating the runtime dispatch and the need for tag registers at the call site. (code)
