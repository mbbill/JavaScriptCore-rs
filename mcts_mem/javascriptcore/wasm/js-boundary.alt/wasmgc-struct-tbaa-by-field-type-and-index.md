- WasmGC struct-field alias keys are split by field value representation and field index.
- OMG struct loads and stores do not carry the RTT that defines inherited fields into the alias key.

## Moves

- 2026-01-16 (4758c065) replaced by [[js-boundary]]: Struct-field alias keys now use the RTT that defines an inherited field plus the field index because unrelated WasmGC struct types can share the same field offset and value type while inherited A.a/B.a/C.a must still alias. (code)
