- The iterated allocator selected spill candidates mainly by interference degree.
- Cold and warm uses did not separately weight spill decisions.
- Stackmap cold uses could protect a value from spilling like ordinary dynamic uses.

## Moves

- 2015-12-01 (b2352c8c) replaced by [[register-allocation]]: Spill selection now scores candidates by interference degree divided by frequency-weighted warm uses and defs so hot values are less likely to be spilled while cold stackmap uses do not protect a value from spilling. (code)
