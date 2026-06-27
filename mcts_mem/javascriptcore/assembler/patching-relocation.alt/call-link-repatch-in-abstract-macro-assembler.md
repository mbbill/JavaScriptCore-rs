- Shared AbstractMacroAssembler code implemented linkCall and repatchCall for every backend.
- x86-64 far-call handling appeared as platform ifdefs inside the shared layer.

## Moves

- 2009-07-22 (fc2a660c) replaced by [[patching-relocation]]: x86-64 far calls are implemented as load-to-r11 then call-r11, so repatchCall must patch the pointer load at offset -3 rather than the call itself — this is an arch-specific invariant that cannot be expressed in the shared AbstractMacroAssembler. (sourced)
