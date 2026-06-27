- Patchpoint and Check values lower to Air Specials backed by stackmap constraints and generation callbacks.
- Callbacks preserve operands through explicit stackmap children rather than reconstructing arithmetic inputs from params.reps.
- Stackmap ValueRep variants encode unconstrained use temperature, lateness, result constraints, and JSValue recovery when needed.
- Stackmap clobbers are modeled at instruction boundaries, with early and late timing visible to register allocation.

## Facts

- 2015-11-02 (601d91d2) pitfall: StackmapSpecial must subtract numIgnoredAirArgs when iterating Air args and add numIgnoredB3Args when mapping to B3 children; mixing those offsets creates indexing bugs between Air operands and B3 children (code).
- 2015-11-04 (fd688e87) rationale: StackmapValue became the superclass of PatchpointValue and CheckValue because stackmap state is common value state for those opcodes (sourced).
- 2015-11-04 (8adefedd) rationale: Check was intended as B3's main OSR exit mechanism because it is a stackmap value whose extra arguments reveal machine representation, and its branch-on-value form supports compare/branch fusion plus out-of-line generation (sourced).
- 2015-11-17 (a28f856a) pitfall: undoing overflowed CheckAdd by subtracting one operand is invalid after register allocation turns Add(x, x) into one source/destination register (sourced).
- 2015-12-04 (04a8cf94) statement: stackmap register clobbers are modeled at instruction boundaries: early clobbers interfere with all inputs, late clobbers interfere with defs/results, and LateUse can model inputs after the instruction (code).
- 2015-12-22 (081b5f9a) statement: ValueRep-to-ValueRecovery intentionally breaks the prior layer boundary; the sourced note treats the small JSValue bridge as acceptable as long as B3 can still compile non-JS clients (sourced).
- 2018-05-01 (385369d3) pitfall: value demotion cannot append a store after a value-producing terminal patchpoint; after critical edges are broken, demoteValues must prepend the store in the terminal successor (code).
- 2018-05-01 (385369d3) rationale: breakCriticalEdges treats value-producing terminals as critical-edge sources instead of inserting new blocks inside demoteValues because clients such as taildup may carry CFG analysis across demoteValues (sourced).

## Moves

- 2015-11-17 (a28f856a) replaced [[b3-check-stackmap-operand-contract]]: The old contract exposed CheckAdd/Sub/Mul operands through params.reps and made clients undo arithmetic, which prevented commutativity and strength-reduction optimizations and failed for add-to-self and multiply input liveness. (sourced)
- 2015-12-04 (04a8cf94) replaced [[patchpoint-implicit-result-temp]]: Patchpoint users need to constrain results to arbitrary stackmap-style representations, such as JS calls requiring the result register while other patchpoints only require some register. (sourced)
- 2015-12-04 (af93be95) replaced [[single-any-value-rep]]: A single Any could not express whether an unconstrained stackmap use should be warm, cold, or late-cold, so ValueRep was split to carry register-allocation temperature and lateness directly. (code)
- 2015-12-22 (081b5f9a) replaced [[b3-js-agnostic-value-representation]]: B3::ValueRep can now turn itself into a ValueRecovery for a JSValue, making tail-call frame shuffling consume stackmap generation parameters directly instead of translating through a separate representation. (sourced)
