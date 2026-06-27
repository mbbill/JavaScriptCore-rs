- Heap::markRoots visited weak handles to an opaque-root fixpoint, then harvested weak references once.
- SlotVisitor::harvestWeakReferences removed each weak-reference harvester as it invoked it.
- Resetting shared mark-stack state did not retain a harvester list for repeated fixpoint passes.

## Moves

- 2011-11-15 (0e4f3b5f) replaced by [[weak-references]]: Weak reference harvesters can add new mark work, so weak handles and harvesters are repeated until the visitor stack is empty. (code)
