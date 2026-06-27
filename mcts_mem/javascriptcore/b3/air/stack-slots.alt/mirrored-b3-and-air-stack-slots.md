- B3 and Air had separate StackSlot objects.
- LowerToAir translated B3 stack slots to Air stack slots through a map.
- FTL state could retain a B3 stack slot after Air had modified its mirror.

## Moves

- 2021-06-02 (7d1c3079) replaced by [[stack-slots]]: Every B3::StackSlot became an Air::StackSlot with copied information, and keeping separate objects was harder to free safely because FTL::State could retain a B3 slot that Air later modified. (sourced)
