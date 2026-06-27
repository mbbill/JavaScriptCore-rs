- OP_CHAR opcode used for all single-char matches regardless of codepoint
- case-insensitive match always went through full Unicode case fold
- matchframe carried 'min', 'minimize', 'op' fields saved across recursive calls
- matchframe carried 'rrc' field duplicating recursive result code

## Moves

- 2007-11-04 (f26e598e) replaced by [[yarr]]: Generic single-character match opcodes applied full Unicode logic even for pure ASCII patterns; adding OP_ASCII_CHAR and OP_ASCII_LETTER_NC specialized opcodes skipped surrogate-pair checks and used efficient ASCII case comparison, yielding 2.6% overall / 32.5% regexp SunSpider speedup. (sourced)
