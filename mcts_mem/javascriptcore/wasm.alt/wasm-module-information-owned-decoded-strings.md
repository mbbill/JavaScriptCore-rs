- Module metadata owns decoded JavaScript string names rather than raw bytes. (`ModuleInformation`)
- Module source bytes are retained separately by the JavaScript module wrapper.

## Moves

- 2017-04-05 (5c40c80b) replaced by [[wasm]]: ModuleInformation became ThreadSafeRefCounted with raw source bytes and byte-vector names so parsed module metadata could be shared across threads instead of owning JS Strings and ArrayBuffer state tied to one thread. (code)
