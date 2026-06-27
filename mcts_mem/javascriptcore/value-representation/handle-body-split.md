- Early KJS separated stack-visible handles from GC-managed implementation bodies.
- Value and Object handles wrap ValueImp/ObjectImp bodies, letting C++ stack scope keep objects alive without making every value a heap allocation.
- Heap bodies opt into collector deletion separately from handles that still protect scope-local values.
- Primitive singleton values are represented without ordinary per-use heap objects once the immediate representation can encode them.
- Destructor and finalization paths must not assume another body referenced by a dying object is still valid.

## Facts

- 2002-03-31 (c7ee67c4) pitfall: ObjectImp destructors must not call setGcAllowed on referenced prototype, internal-value, or scope bodies because those objects may already have been collected (code).

## Moves

- 2002-03-22 (9491afaa) replaced [[kjso-unified-value-object]]: The monolithic KJSO class (both value and object in one) was replaced by a Value/ValueImp and Object/ObjectImp split using the handle/body (smart-pointer wrapper) pattern, separating GC-managed heap objects from stack-allocated handles that control refcounting. (code)
