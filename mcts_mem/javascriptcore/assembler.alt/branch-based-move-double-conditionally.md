- Conditional double moves used branch-around-move sequences in the shared MacroAssembler layer.
- The destination also acted as one select arm for Air conditional-double-move operations.

## Moves

- 2016-03-03 (31fde45f) replaced by [[assembler]]: ARM64 can use FCSEL to select floating-point values directly from flags, while x86 benefits mainly from allowing conditional-double-move destinations to alias an input. (code)
