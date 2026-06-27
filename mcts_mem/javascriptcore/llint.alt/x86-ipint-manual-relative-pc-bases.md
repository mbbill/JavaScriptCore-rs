- x86 IPInt initializes relative PC bases manually with call/pop sequences.
- Opcode dispatch labels are expressed relative to a synthetic base and added to PL or PC.
- x86 dispatch loads IPInt dispatch base data from protected config storage instead of materializing local label addresses directly.

## Moves

- 2026-02-28 (5e96ef3f) replaced by [[llint]]: Adding x86 pcrtoaddr let IPInt compute PC-relative label addresses directly like ARM64, eliminating per-entry manual relative-PC base setup and special x86 dispatch-base loads. (code)
