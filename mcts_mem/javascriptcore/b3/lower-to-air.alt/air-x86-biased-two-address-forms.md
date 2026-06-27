- Many Air opcodes were implicitly available on all targets with x86-style two-address forms.
- Shift lowering always copied the left operand into the result and used ecx for variable shift counts.
- Integer Div and Mod lowering unconditionally used x86 helper-register sequences.

## Moves

- 2015-12-23 (2e714c09) replaced by [[lower-to-air]]: B3-on-ARM64 required Air to distinguish x86 two-address and memory forms from ARM64 three-address register forms, so lowering probes target-valid forms instead of always emitting the x86-biased sequence. (code)
