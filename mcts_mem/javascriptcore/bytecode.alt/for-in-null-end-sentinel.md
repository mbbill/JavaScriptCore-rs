- slow_path_enumerator_next wrote jsNull() to the property-name register when computeNext returned no name.
- ForInNode::emitBytecode ended the loop by testing the property-name register with is_undefined_or_null.
- A comment said choosing undefined or null pleased the DFG abstract interpreter without distinguishing null and undefined as types.

## Moves

- 2021-09-27 (8e47e3c2) replaced by [[bytecode]]: For-in iteration now returns a preallocated sentinel JSString cell at end-of-iteration so EnumeratorNextUpdatePropertyName remains string-typed instead of being polluted by null/Other. (code)
