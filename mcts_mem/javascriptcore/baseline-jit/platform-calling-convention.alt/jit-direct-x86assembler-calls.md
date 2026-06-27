- JIT code emits direct X86Assembler calls through the m_assembler macro.

## Moves

- 2008-12-05 (1ff382e8) replaced by [[platform-calling-convention]]: The JIT was ported from direct X86Assembler calls (via the '__ m_assembler.' macro) to the MacroAssembler abstraction layer to enable future cross-platform portability; the commit notes no change in performance. (sourced)
