- Kind stored an Opcode plus one extra union field for chillness or raw bits.
- Validation rejected extra bits on load and store opcodes.
- Value effects did not mark trapping operations as sideways exits.

## Moves

- 2016-10-01 (c1aed292) replaced by [[b3]]: A single chill extra bit could not represent memory accesses whose traps are observable, so B3::Kind became a bag of flags with a traps bit that affects effects analysis, store elimination, validation, and Air lowering. (code)
