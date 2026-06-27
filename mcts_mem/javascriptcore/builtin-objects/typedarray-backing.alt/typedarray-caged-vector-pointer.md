- Typed-array vector storage used a caged pointer without per-element-kind poisoning.
- JIT and interpreter clients uncaged the vector after loading a common field.

## Moves

- 2018-01-31 (323ad281) replaced by [[typedarray-backing]]: TypedArray vector storage was changed from a merely caged pointer to a per-JSType poisoned caged pointer so each TypedArray kind uses a distinct poison selected from a masked power-of-two table. (code)
