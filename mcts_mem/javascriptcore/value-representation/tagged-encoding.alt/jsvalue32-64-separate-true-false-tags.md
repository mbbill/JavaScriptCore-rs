- TrueTag and FalseTag were separate tag constants.
- Conditional branches compared against the two boolean tags separately.
- Boolean payload was not used to distinguish true from false under one tag.

## Moves

- 2011-04-07 (810b982f) replaced by [[tagged-encoding]]: Merging TrueTag (0xfffffffb) and FalseTag (0xfffffffa) into a single BooleanTag (0xfffffffe) with boolean value in the payload word (same pattern as Int32Tag+payload) lets jfalse/jtrue use a single ranged branch (BooleanTag..Int32Tag) to fast-path both booleans and integers, eliminating two separate tag comparisons per conditional-branch opcode; combined with a payload boolean convention, this keeps boolean materialization simple. (sourced)
