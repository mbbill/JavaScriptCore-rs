- Optimized string-prototype-chain checks watch StringPrototype and ObjectPrototype structure transitions at each use site.
- DFG and FTL attach ad-hoc structure watchpoints for string out-of-bounds and prototype-chain assumptions.

## Moves

- 2022-10-12 (4d0bfb7d) replaced by [[global-object]]: StringPrototype sane-chain checks moved to a JSGlobalObject watchpoint set so DFG/FTL no longer scatter ad hoc transition watchpoints and uDFG can query one global-object condition. (sourced)
