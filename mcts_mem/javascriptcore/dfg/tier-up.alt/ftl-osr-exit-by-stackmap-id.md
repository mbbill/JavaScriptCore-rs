- FTL OSRExit stored static exit metadata and one stackmap ID during lowering.
- Finalization matched exits to stackmap records by patchpoint ID after LLVM optimization.

## Moves

- 2015-10-19 (9335f1ca) replaced by [[tier-up]]: LLVM can duplicate or remove OSR-exit stackmap intrinsics, so one logical lowering-time descriptor may correspond to zero, one, or more concrete stackmap records and each generated OSRExit must point at a specific record index. (sourced)
