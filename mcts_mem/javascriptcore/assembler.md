- Portable JIT code emission is a layered assembler: CPU backends encode machine instructions, MacroAssembler layers expose composite operations, and the shared abstraction carries the label, jump, call, address, and operand vocabulary.
- Backend selection is compile-time rather than runtime; unsupported architectures are removed from the live target set instead of kept as dormant dispatch cases.
- Emitted code remains buffer-relative until linking and is copied into dedicated executable memory before execution.
- Branch range, call reachability, patchability, and executable-memory permissions are explicit assembler concerns rather than hidden in callers.
- Detailed backend support, buffer/linking, executable memory, and branch/repatch mechanics live in [[cpu-backends]], [[buffer-label-linking]], [[executable-memory]], and [[patching-relocation]].

## Facts

- 2008-11-29 (cc61c4c2) rationale: MacroAssembler was introduced so WREC could compile independently of the bytecode JIT; ENABLE_WREC no longer required ENABLE_JIT in Platform.h. (sourced)
- 2009-02-04 (9f3dad7a) rationale: Scale values changed from literal multiplicands to a plain enum because multiplicand values leaked x86 SIB encoding into the portable MacroAssembler API. (sourced)
- 2009-05-15 (00357420) rationale: floating-point MacroAssembler support was limited to SSE2-class hardware; unsupported platforms report supportsFloatingPoint() false and leave FP methods as assertion-only stubs. (sourced)
- 2009-08-27 (8bc44fb5) pitfall: x86-64 pointer operations could not assume a 4GB zero page on every port; sign-extended 32-bit immediate encodings for pointers needed platform-aware range checks. (sourced)

## Moves

- 2009-02-05 (422224dd) replaced [[macro-assembler-monolithic-class]]: The monolithic MacroAssembler hard-coded X86Assembler as the backend and duplicated x86/x86-64 logic in the same class; the expressivity wall was that adding a non-x86 backend (ARM, MIPS) would require forking the entire class, whereas templating AbstractMacroAssembler on AssemblerType isolates platform-agnostic data types and lets MacroAssemblerX86Common share code between x86 and x86-64 without duplication. (sourced)
- 2010-04-22 (7c2c0dab) replaced [[arm-call-via-pc-write]]: Writing to PC for calls and returns does not update the link register correctly and confuses the ARM return stack predictor on ARMv5+; BLX/BX instructions satisfy the predictor and correctly set the link register, improving branch prediction on ARMv5+ hardware. (sourced)
- 2016-03-03 (31fde45f) replaced [[branch-based-move-double-conditionally]]: ARM64 can use FCSEL to select floating-point values directly from flags, while x86 benefits mainly from allowing conditional-double-move destinations to alias an input. (code)
- 2016-04-18 (375eee88) replaced [[x86-avx-fp-instruction-selection]]: AVX was disabled because using it while other code is not careful with float register bits can make execution 10x slower, and a massive regression was seen on real code. (sourced)
- 2017-01-03 (d6ead802) removed: SH4 JIT backend: SH4-specific JSC assembler, MacroAssembler, register metadata, and offlineasm backend code was removed because it had not compiled since at least r189884 and nobody maintained the architecture. (sourced)
- 2023-10-18 (4a0ec7da) replaced [[armv7-bkpt-breakpoint]]: ARMv7 breakpoints switched to UDF because BKPT can hang under gdb instead of trapping. (sourced)
