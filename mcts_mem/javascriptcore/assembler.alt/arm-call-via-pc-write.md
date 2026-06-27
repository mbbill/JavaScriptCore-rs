- ARM calls prepared the link register and then wrote the target into PC.
- Returns moved LR back into PC instead of using branch-and-exchange return instructions.

## Moves

- 2010-04-22 (7c2c0dab) replaced by [[assembler]]: Writing to PC for calls and returns does not update the link register correctly and confuses the ARM return stack predictor on ARMv5+; BLX/BX instructions satisfy the predictor and correctly set the link register, improving branch prediction on ARMv5+ hardware. (sourced)
