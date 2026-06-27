- Air can represent taking a stack-slot address with UseAddr without implying a load or store.
- Stack-slot allocation uses interference/coloring information and special handling for escaped or indexed stack-slot references.
- Stack load/store address selection uses tiered target addressing rather than one frame-pointer form plus an absolute fallback.

## Facts

- 2015-10-29 (135dff2c) statement: UseAddr evaluates an address without loading from it or storing to it, making it distinct from Use and Def of address operands (sourced).
- 2016-01-07 (24914c61) rationale: ARM64 FP-relative direct addressing has only a signed 9-bit immediate while SP-relative addressing can use unsigned scaled 12-bit immediates, so SP-relative forms give stack slots more encodable range (sourced).
- 2021-06-02 (7d1c3079) measurement: unifying B3 and Air stack slots was reported to save 16 bytes per spill slot and 40 bytes per B3-locked slot (sourced).
- 2023-02-22 (6d88e268) measurement: tiered Air stack-slot addressing was reported to make generated code for JetStream2 Wasm benchmarks about 30% smaller on ARMv7 and about 40% smaller on ARM64 (sourced).
- 2026-03-20 (c03651a4) measurement: sorted-sweep stack-slot assignment reduced the cited worst-case testair workload from about five minutes to a few seconds when many temporaries interfered (sourced).

## Moves

- 2015-10-29 (135dff2c) replaced [[air-non-escaping-stack-slots]]: UseAddr is only used by Lea, and the stack allocation phase now understands that StackSlots may escape and factors this into its analysis. (sourced)
- 2016-01-22 (abd7613c) replaced [[b3-dominating-memory-cse]]: B3 CSE changed from requiring one dominating memory match to accepting a set of matches that cover all predecessor paths, using anonymous stack slots so FixSSA can synthesize the needed Phi graph. (code)
- 2021-06-02 (7d1c3079) replaced [[mirrored-b3-and-air-stack-slots]]: Every B3::StackSlot became an Air::StackSlot with copied information, and keeping separate objects was harder to free safely because FTL::State could retain a B3 slot that Air later modified. (sourced)
- 2023-02-22 (6d88e268) replaced [[air-stack-slot-fp-or-absolute-addressing]]: Air stack load/store address selection switched from frame-pointer-or-absolute fallback to a tiered FP, SP, FP+index, then absolute strategy because avoiding absolute-address computation significantly reduces generated code size on ARM. (sourced)
- 2026-03-20 (c03651a4) replaced [[air-stack-slot-assignment-candidate-rescan]]: Air stack allocation changed from trying candidate offsets and rescanning all interfering slots to sorting assigned interferences by frame offset and doing one downward sweep past overlaps, reducing the assignment algorithm from O(n²) to O(n log n). (code)
