- Air stack allocation handled locked slots and anonymous slots, but did not support escaping a StackSlot.
- Operand roles distinguished Use, Def, and UseDef; an address operand meant load or store rather than evaluated address value.

## Moves

- 2015-10-29 (135dff2c) replaced by [[stack-slots]]: UseAddr is only used by Lea, and the stack allocation phase now understands that StackSlots may escape and factors this into its analysis. (sourced)
