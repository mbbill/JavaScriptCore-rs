- FTL OSR-entry triggering waited for or requested a full-function replacement compile first.
- If no replacement existed, OSR-entry compilation was skipped for a DFG function that had never run from normal entry.

## Moves

- 2016-02-26 (8bddc539) replaced by [[tier-up]]: A DFG function used only for OSR entry could waste 8-10 ms waiting for full-function FTL compilation, so triggerOSREntryNow now starts both replacement and OSR-entry FTL compiles when the DFG entry flag says entry never ran. (sourced)
