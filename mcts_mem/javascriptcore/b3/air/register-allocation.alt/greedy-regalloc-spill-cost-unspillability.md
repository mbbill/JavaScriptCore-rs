- Greedy TmpData stored one spillCost field.
- Physical registers, fast temporaries, and synthetic spill temporaries used an unspillable sentinel cost.
- Coalesced groups summed subgroup spill costs, allowing a sentinel to contaminate group cost.

## Moves

- 2025-02-25 (4c15f539) replaced by [[register-allocation]]: Unspillability had to be represented separately from use/def spill cost because coalesced groups should aggregate use/def cost without inheriting the unspillable property of short individual tmps. (sourced)
