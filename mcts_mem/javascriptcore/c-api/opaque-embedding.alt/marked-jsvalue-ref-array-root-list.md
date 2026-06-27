- A dedicated `MarkedJSValueRefArray` root list stores API argument references as `JSValueRef`-sized entries.
- The marked container is specialized to API value references rather than arbitrary marked value/cell/reference element types.

## Moves

- 2026-03-30 (4bd1aab3) replaced by [[opaque-embedding]]: A dedicated JSValueRef array could only store API values as JSValueRef-sized entries, while MarkedVector needed to be a Vector-like root container for JSValue, JSCell-derived pointers, and JSC API reference types including 32-bit pointer-sized storage. (code)
