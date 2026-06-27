- LinkBuffer and RepatchBuffer were nested classes in AbstractMacroAssembler.
- Users named the buffers through the MacroAssembler scope.

## Moves

- 2009-07-22 (2e878a27) replaced by [[patching-relocation]]: Moving LinkBuffer and RepatchBuffer out of AbstractMacroAssembler into standalone headers was the first step in enabling per-architecture linkCall/repatchCall implementations that cannot live inside the shared base class. (sourced)
