- JSObject stored a Butterfly barrier field directly after the JSCell header.
- All JSObject subclasses inherited butterfly accessors and a nullable butterfly slot.
- Wasm GC objects inherited the slot even though their fast paths initialized it to null.

## Moves

- 2026-03-28 (55783b94) replaced by [[object-model]]: Wasm GC objects cannot have properties, prototypes, or structure transitions, so their inherited m_butterfly slot was always null and wasted one pointer per allocation. (code)
