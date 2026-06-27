- B3::Value stored a single Opcode as its operation identity.
- ChillDiv and ChillMod were separate opcodes parallel to Div and Mod.
- ValueKey stored only Opcode for CSE and materialization identity.

## Moves

- 2016-09-29 (40a45a42) replaced by [[b3]]: B3 needed multidimensional operation identity because a one-dimensional Opcode enum would require combinatorial opcode growth for independent flags such as chillness, trapping loads, and memory-ordering modes, while subclass fields would not participate in Value::accepts and ValueKey by default. (sourced)
