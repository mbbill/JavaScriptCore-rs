- KJSO represented value and object behavior in one handle class.
- Imp stored object state such as properties and prototype behind that unified wrapper.
- Type information lived in object-oriented metadata attached to the unified representation.

## Moves

- 2002-03-22 (9491afaa) replaced by [[handle-body-split]]: The monolithic KJSO class (both value and object in one) was replaced by a Value/ValueImp and Object/ObjectImp split using the handle/body (smart-pointer wrapper) pattern, separating GC-managed heap objects from stack-allocated handles that control refcounting. (code)
