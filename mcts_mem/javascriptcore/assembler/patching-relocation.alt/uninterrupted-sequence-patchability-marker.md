- Patchable branch emission was inferred from beginUninterruptedSequence/endUninterruptedSequence state in the MacroAssembler.
- Branch sites wrapped in that uninterrupted state could later have locations taken for patching.

## Moves

- 2012-04-25 (a2d9e4a5) replaced by [[patching-relocation]]: Patchability became explicit in the branch/jump type instead of being inferred from uninterrupted-sequence state, so only patchable branches can have locations taken and ARMv7 fixed-width jumps are requested directly at emission sites. (code)
