- Owners that add managed references must remove those references manually from their deallocation paths.
- `JSManagedValue` does not own a complete record of owners to unregister itself.

## Moves

- 2014-02-07 (1c4d8d7f) replaced by [[objective-c-embedding]]: The JSManagedValue now records its owners and unregisters itself on dealloc, so owners no longer need bespoke dealloc code to balance addManagedReference:withOwner: calls. (code)
