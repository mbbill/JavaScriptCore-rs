- Polymorphic inline caches are represented as linked generated stubs.
- Removing a subsumed case requires walking or regenerating a linear chain.

## Moves

- 2015-09-10 (5481280a) replaced by [[inline-caches]]: The linked-list inline cache representation could not regenerate or remove a previously generated subsumed stub and scaled linearly with cases, while the new single regenerated PolymorphicAccess stub preserves metadata and can use BinarySwitch. (sourced)
