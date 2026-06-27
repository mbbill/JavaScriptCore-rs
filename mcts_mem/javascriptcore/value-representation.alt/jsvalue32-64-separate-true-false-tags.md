- True and false each had distinct JSVALUE32_64 tag constants.
- Conditional branch opcodes tested boolean cases with separate tag comparisons.
- Boolean values were not represented as one BooleanTag plus payload.

## Moves

- 2011-04-07 (810b982f) replaced by [[value-representation]]: Merging TrueTag (0xfffffffb) and FalseTag (0xfffffffa) into a single BooleanTag (0xfffffffe) with boolean value in the payload word (same pattern as Int32Tag+payload) lets jfalse/jtrue use a single ranged branch (BooleanTag..Int32Tag) to fast-path both booleans and integers, eliminating two separate tag comparisons per conditional-branch opcode; combined with a payload boolean convention, this keeps boolean materialization simple. (sourced)
