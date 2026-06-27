- ScopedArguments stored overflow entries inline after the object cell.
- Overflow metadata and entries were not in a separately poisonable allocation.

## Moves

- 2018-03-22 (292200f7) replaced by [[builtin-objects]]: ScopedArguments needed pointer poisoning and index masking, which the inline-tail storage representation could not provide for the overflow pointer and header without moving them into a poisonable auxiliary allocation. (code)
