- CPU(X86) selected MacroAssemblerX86 as a live backend.
- The backend exposed 32-bit pointer-sized call, branch, far-jump, patching, and repatching helpers.

## Moves

- 2021-08-20 (21ea32f9) removed: MacroAssemblerX86 was deleted, eliminating the CPU(X86) MacroAssembler target selection and leaving x86-family support through MacroAssemblerX86Common and MacroAssemblerX86_64 only. (code)
