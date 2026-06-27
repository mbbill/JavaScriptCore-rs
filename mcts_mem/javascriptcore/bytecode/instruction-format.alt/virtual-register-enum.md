- enum VirtualRegister { InvalidVirtualRegister = 0x3fffffff }
- free functions operandIsArgument/operandToArgument/operandToLocal/argumentToOperand in Operands.h
- COMPILE_ASSERT sizeof VirtualRegister == sizeof int

## Moves

- 2013-09-26 (027ced83) replaced by [[instruction-format]]: VirtualRegister enum could not encapsulate operand-classification methods (isLocal/isArgument/toLocal/toArgument), which were scattered as free functions in Operands.h; a class enables those predicates to live on the type itself. (Rolled out by 13385, re-landed at 13395.) (code)
