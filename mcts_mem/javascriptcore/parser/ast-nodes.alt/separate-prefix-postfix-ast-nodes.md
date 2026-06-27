- Prefix and postfix update expressions used separate node classes for resolve, bracket, dot, and error targets.
- Shared update-expression metadata was duplicated across those classes.

## Moves

- 2012-09-05 (2dd3aaab) replaced by [[ast-nodes]]: Four separate node classes per prefix/postfix direction (resolve/bracket/dot/error) held redundant fields and prevented access to m_subscriptHasAssignments when emitting bracket access, which required the unified node with a dispatching emitBytecode. (sourced)
