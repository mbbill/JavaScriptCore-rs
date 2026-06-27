- Custom DOM accessors in ICs call the opaque custom accessor through C++.

## Moves

- 2016-10-17 (f1503ba7) replaced by [[inline-caches]]: Custom DOM accessors in inline caches needed the DOMJIT::Patchpoint environment so Baseline GetById ICs and DFG/FTL GetById cases could inline DOM access instead of always calling the opaque custom accessor. (sourced)
