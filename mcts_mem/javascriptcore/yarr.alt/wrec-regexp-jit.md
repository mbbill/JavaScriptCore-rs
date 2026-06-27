- WREC provides the regexp JIT implementation behind ENABLE(WREC).
- WRECParser translates regexp syntax into MacroAssembler-driven x86 machine code generation.
- WREC carries its own character-class, escape, and quantifier representation.

## Moves

- 2010-02-27 (7faa2fd9) removed: All builds had switched to YARR, making WREC dead code; the commit message states 'All builds should have switched to yarr by now.' (sourced)
