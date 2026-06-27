- B3 Procedure retained variables, blocks, CFG caches, values, tuples, stack slots, and constants across Air generation.
- Air generation appended PC-to-origin map entries from each instruction origin unconditionally.

## Moves

- 2021-06-12 (7e0ab48d) replaced by [[lower-to-air]]: B3 drops most Procedure state after lowering to Air to reduce memory while preserving origins only for PC-to-origin mapping, dumping/disassembly, and origin-dependent Air operations. (sourced)
