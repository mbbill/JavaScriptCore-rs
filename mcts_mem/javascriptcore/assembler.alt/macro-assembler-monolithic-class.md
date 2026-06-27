- MacroAssembler was a single class directly owning an X86Assembler backend.
- Shared address, label, jump, patch, and scratch-register types were nested in that x86-bound class.
- x86 and x86-64 behavior lived in the same monolithic MacroAssembler surface.

## Moves

- 2009-02-05 (422224dd) replaced by [[assembler]]: The monolithic MacroAssembler hard-coded X86Assembler as the backend and duplicated x86/x86-64 logic in the same class; the expressivity wall was that adding a non-x86 backend (ARM, MIPS) would require forking the entire class, whereas templating AbstractMacroAssembler on AssemblerType isolates platform-agnostic data types and lets MacroAssemblerX86Common share code between x86 and x86-64 without duplication. (sourced)
