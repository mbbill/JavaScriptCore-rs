- Module metadata represents at most one declared or imported memory. (`ModuleInformation`)
- Instance state stores one JavaScript memory object and one cached Wasm memory pointer.
- Section parsing rejects more than one memory definition or import-definition combination.

## Moves

- 2026-02-09 (bdf26416) replaced by [[memory-model]]: Add support for instantiating multiple memories in wasm, but not for executing instructions that use memories other than index 0. (sourced)
