- Opcode field width matched operand width (TypeBySize<Width>::unsignedType m_opcode)
- size() computed as Traits::opcodeLengths[id] * operandSize + padding
- Argument index started at 1 (accounting for opcode slot)

## Moves

- 2019-12-25 (24b088b7) replaced by [[instruction-format]]: In wide16/wide32 instructions, the opcode was also emitted at 16/32 bits, wasting space because opcodes always fit in 8 bits; always emitting a narrow (1-byte) opcode saves one byte per operand-slot in every wide instruction. (sourced)
