- MarkStack owned one MarkStackArray plus opaque roots and weak-reference harvesters directly.
- Heap::markRoots drained a single visitor after adding roots.
- MarkStackArray was one growable contiguous stack with no segment donation or stealing.

## Moves

- 2011-11-01 (5e28ce2e) replaced by [[heap]]: Marking work can be split among marker threads by keeping per-thread local stacks plus a shared steal/donate stack and making mark-bit updates atomic. (code)
