- ArrayBuffer and typed-array backing pointers were raw void* fields.
- Primitive Gigacage expectations were documented by comments rather than encoded in pointer types.

## Moves

- 2017-08-31 (96c10153) replaced by [[typedarray-backing]]: ArrayBuffer and typed-array storage pointers were represented as CagedPtr/CagedBarrierPtr so the Primitive Gigacage invariant is encoded in the pointer fields instead of being documented by FIXME comments on raw void* storage. (code)
