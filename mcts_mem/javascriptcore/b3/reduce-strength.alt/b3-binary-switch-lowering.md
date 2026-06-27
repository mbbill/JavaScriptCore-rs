- LowerMacros lowered Switch by sorting cases and recursively splitting on median cases.
- Small leaves were linearized with Equal branches and jumps.
- Dense switch ranges did not use jump tables.

## Moves

- 2016-07-19 (53a1e5c7) replaced by [[reduce-strength]]: Large dense switch ranges are now lowered to a terminal Patchpoint that emits an immutable jump table, while sparse ranges continue to use the old recursive binary switch lowering. (code)
