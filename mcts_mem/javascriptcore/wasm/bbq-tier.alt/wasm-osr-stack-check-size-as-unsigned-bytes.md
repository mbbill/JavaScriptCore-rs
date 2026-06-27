- OSR-entry callees store the stack-check size as an unsigned byte count initialized to zero.
- Zero represents both unset state and leaf functions that need no OMG stack check.

## Moves

- 2024-03-26 (4322c3bd) replaced by [[bbq-tier]]: Zero was both the unsigned field's unset value and the computed size for leaf functions where OMG omitted stack checks, so the representation changed to signed sentinels that distinguish unset from not-needed. (code)
