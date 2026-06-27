- BooleanUse was used for boolean consumers and implied speculative boolean checking.
- UseKind had known-int/cell/string forms but no known-boolean non-checking use.

## Moves

- 2015-08-21 (9a65a6f7) replaced by [[speculation]]: Branch and LogicalNot fed by an effectful boolean-producing comparison needed to be representable as non-exiting uses, but BooleanUse implied speculation/type-check machinery. (code)
