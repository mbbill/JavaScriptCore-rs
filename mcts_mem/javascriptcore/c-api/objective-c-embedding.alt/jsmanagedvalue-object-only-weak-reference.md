- `JSManagedValue` stores only weak object references.
- Primitive and string JS values cannot be represented directly by the managed weak-value wrapper.

## Moves

- 2013-10-08 (b67599d6) replaced by [[objective-c-embedding]]: Weak<JSObject> could not represent non-object JSValues, while WeakValueRef can carry primitives, strings, or objects and a weak global object for reconstruction. (code)
