- MarkStack stored the parallel marking work list, opaque-root set, weak-harvester list, and unconditional-finalizer list.
- SlotVisitor subclassed MarkStack to add drain, copy, weak-reference harvesting, and finalizer operations.

## Moves

- 2012-09-10 (6e39cc19) replaced by [[heap]]: SlotVisitor was a thin subclass of MarkStack that added copying/parallel-drain logic; merging them into a single class eliminates the inheritance indirection and allows all GC visitor state to live in one place. (sourced)
