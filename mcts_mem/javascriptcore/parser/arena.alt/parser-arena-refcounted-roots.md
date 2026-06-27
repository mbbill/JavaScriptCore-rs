- Parser roots were ref-counted arena objects and special-cased out of the arena's ref-counted object list.
- Root teardown used explicit destroyData-style cleanup before the arena could go away.

## Moves

- 2014-12-05 (086e0611) replaced by [[arena]]: Once each parse tree had a clear root node type, parse-tree ownership no longer needed a type that could be either refcounted or arena-allocated and could instead be managed with unique_ptr and normal C++ destructors. (sourced)
