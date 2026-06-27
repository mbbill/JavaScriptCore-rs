- ReduceStrength is the local optimization pass for cheap canonicalization, CFG cleanup, DCE, pure-value CSE, switch inference/lowering, and arithmetic strength reductions.
- B3 DCE marks must-execute roots and live Phi/Upsilon relationships rather than repeatedly sweeping zero-use values.
- Expensive or globally shaped optimizations are kept separate, moved to macro lowering, or bounded when the fixpoint becomes too costly.
- Division and vector constant reductions introduce explicit B3 operations or data-section forms when target lowering needs a durable IR shape.

## Facts

- 2015-11-06 (2bf461d0) rationale: CFG simplification runs inside the ReduceStrength fixpoint because it reveals further optimization opportunities, especially for Phi elimination (sourced).
- 2015-12-08 (f9713057) rationale: tag constants can be marked important so B3 hoists them and Air can force them into registers, avoiding repeated materialization pressure in FTL-generated code (sourced).
- 2015-12-11 (2146979e) rationale: B3 pure-value CSE deliberately excludes effects-bearing values and memory CSE from the same mechanism, leaving memory equivalence to a separate path (code).
- 2016-01-21 (725f40d6) measurement: adding CSE for pure B3 values was reported as a 5% speed-up on the gaussian-blur microbenchmark under FTL B3 (sourced).
- 2016-01-23 (be4e3d33) rationale: branch folding from a dominating Check is a cheap local substitute for stronger SCCP analysis (code).
- 2017-04-05 (90667eef) measurement: tracking possibly-dead Air instructions avoided repeated full scans in eliminateDeadCode's fixpoint (code).
- 2026-06-16 (0ba6297d) measurement: preliminary analysis found 98% of B3 values converge in the first ReduceStrength run and remaining passes are near-no-ops (sourced).

## Moves

- 2015-11-01 (5bcc8fe4) replaced [[b3-usecount-sweep-dce]]: The old sweep deleted never-referenced values one fixpoint at a time and did not eliminate cycles, while the new pass presumes all values dead, marks must-execute roots and their children live, and iterates Upsilons whose Phis are live. (code)
- 2016-01-09 (0ad352bd) replaced [[b3-negation-as-sub-zero]]: For floating point, Sub(0, 0) produces +0 while true negation produces -0, and representing floating negation as BitXor(x, -0) would force clients to pattern-match different encodings for integer and floating negation. (sourced)
- 2016-04-18 (60e36af4) replaced [[local-double-to-float-reduction]]: Local candidate elimination could not propagate float precision through Phi/Upsilon loops, so the replacement uses backward and forward analyses plus cleanup to convert only double values and phis whose precision and uses allow float form. (code)
- 2016-07-19 (53a1e5c7) replaced [[b3-binary-switch-lowering]]: Large dense switch ranges are now lowered to a terminal Patchpoint that emits an immutable jump table, while sparse ranges continue to use the old recursive binary switch lowering. (code)
- 2016-07-21 (b07597bd) replaced [[b3-linear-branch-chain-for-indirect-switches]]: Chains of branches that test equality on the same value can be inferred into a B3 Switch, turning O(n) dispatch into O(log n) or O(1) when cases are dense. (sourced)
- 2016-09-30 (f906ce1e) replaced [[exact-constant-placement]]: Exact constant placement could not reuse a nearby address constant or a negated add/sub constant, so moveConstants began rewriting memory offsets and flipping Add/Sub to use the most dominant equivalent constant. (code)
- 2017-04-05 (001eb863) replaced [[b3-fixssa-global-eager-conversion]]: Local SSA conversion, dead Set removal, sparse variable maps, cached reaching definitions, and lazy mapping reduce the amount of global SSA work needed after wasm made fixSSA a top compile-time cost. (sourced)
- 2017-04-05 (90667eef) replaced [[air-dead-code-full-fixpoint-scan]]: Tracking only instructions that might still be dead avoids repeatedly processing instructions already proven live during the eliminateDeadCode fixpoint. (code)
- 2022-12-15 (5ec60738) replaced [[inline-const128-lane-materialization]]: Materializing Const128 takes many instructions, so B3 now loads non-zero vector constants from a data section like double and float constants. (sourced)
- 2025-03-21 (a4cc57da) replaced [[div-strength-reduction-expanded-multiply-shift]]: Div/Mod strength reduction needs the upper extended bits of multiplication explicitly in the IR so targets with native high-multiply operations can lower it directly instead of materializing a widened multiply plus shift. (code)
- 2026-06-16 (0ba6297d) replaced [[b3-reduce-strength-fixpoint]]: The fixpoint loop was wasteful: preliminary analysis showed 98% of B3 values converge in the first run and remaining passes are near-no-ops; replacing with a bounded single-pass (at most two passes) reduces compile time for large graphs while per-value inner retry loops (maxReductionAttempts=8) ensure local convergence within a single walk. (sourced)
