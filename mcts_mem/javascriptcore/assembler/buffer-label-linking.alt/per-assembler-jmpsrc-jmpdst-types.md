- Each assembler backend defined its own JmpSrc and JmpDst classes.
- AbstractMacroAssembler Jump and Call wrapped those per-backend source and destination types.
- Some backends overloaded the per-assembler types with call, data-label, jump-type, or condition state.

## Moves

- 2011-05-01 (b2d50241) replaced by [[buffer-label-linking]]: Per-assembler JmpSrc/JmpDst classes predated the MacroAssembler abstraction; having them per-assembler caused code duplication, prevented AssemblerBuffer from providing a richer shared label type, and their semantic meaning was already undermined (JmpSrc overloaded for Call, JmpDst for data labels; ARMv7 JmpSrc carrying extra jump-type/condition data that could not fit cleanly in the base). (sourced)
