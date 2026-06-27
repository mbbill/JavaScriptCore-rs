- Profiled arithmetic bytecodes stored BinaryArithProfile*/UnaryArithProfile* fields in their metadata.
- UnlinkedCodeBlock::allocateSharedProfiles sized arithmetic profile arrays by counting metadata entries for opcodes with arithmetic profiles.
- CodeBlock::binaryArithProfileForPC and unaryArithProfileForPC read the profile pointer from the opcode metadata.

## Moves

- 2021-09-26 (d1cb45f8) replaced by [[metadata-table]]: Arithmetic bytecodes carry profile table indices instead of metadata pointers so each instruction can find its BinaryArithProfile/UnaryArithProfile while saving metadata memory. (code)
