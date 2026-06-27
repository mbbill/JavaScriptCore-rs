- OMG reparses validated wasm into B3 and lowers through Air as the optimizing tier. (`OMGCallee`)
- B3 types, values, variables, phis/upsilons, patchpoints, stackmaps, and abstract heaps are the tier's representation boundary.
- Direct-call inlining reparses IPInt-profiled callees into the caller and limits growth by inline depth, callee byte size, and accumulated caller byte size.
- Wasm memory addressing, reference checks, SIMD, exceptions, and tail calls become explicit B3/Air forms for later optimization and stackmap generation.
- WasmGC struct/array accesses use NumberedAbstractHeap and StructureID/RTT metadata; aliasing and type checks follow wasm type identity instead of physical offsets alone.

## Facts

- 2016-09-07 (92cad735) statement: WASM B3 continuation blocks are allocated lazily; unreachable fallthroughs and returns do not create orphaned BasicBlocks that B3::validate rejects. (code)
- 2016-09-09 (1bdd3ac4) rationale: WASM if lowering allocates the full taken/notTaken/continuation diamond even for if-without-else, relying on B3 cleanup to remove unnecessary blocks rather than using a separate partial shape. (sourced)
- 2017-03-02 (c1ed08e3) pitfall: B3WasmAddress lowering must fall back when Arg::isValidIndexForm(1, offset, width) is false because ARM does not support every base + index*1 + offset form that x86 accepts. (sourced)
- 2021-12-14 (35e8b712) rationale: Wasm B3 compilation estimates block execution counts from natural-loop depth before SSA repair so the register allocator has non-uniform block frequencies instead of treating all blocks equally. (sourced)
- 2023-03-03 (fa0d2ca3) rationale: inlinee arguments and results are bridged with B3 variables while the inlinee skips its prologue, epilogue, and stack check because the caller already owns the machine frame and entry obligations. (code)
- 2023-03-03 (fa0d2ca3) rationale: Wasm inlining is limited by inline depth, callee byte size, and accumulated caller byte size rather than only by call kind, so reparsing-based inlining cannot grow OMG compilation unboundedly. (code)
- 2024-11-05 (9affbf98) rationale: the implementation deliberately chooses bitwise-select semantics for relaxed laneselect on both ARM64 and x86_64; the message cites native ARM64 bsl support and says JSC's AVX requirement makes SSE4.1 top-bit pblend behavior inapplicable, while vpblend optimization is deferred. (sourced)
- 2025-05-19 (19188223) rationale: OMG IR must use pointer-width and Wasm-reference abstractions rather than hard-coded Int64/Const64Value so the 32-bit OMG generator can follow the 64-bit generator without duplicating type conditionals. (sourced)
- 2025-06-24 (d934b727) pitfall: when the tail-call frame shuffle may clobber tmp, any argument source that is tmp must read from the early tmpSpill rather than the register, because later constant materialization and debug moves can overwrite tmp before the argument move list is executed. (code)
- 2025-11-18 (11eaa391) pitfall: OMG tail-call frame setup must save the MacroAssembler scratch register before any code path may use it as scratch, address that spill relative to the original SP, restore it before moving SP to the callee frame, and disallow scratch use after restore while the register may still be a live input. (code)

## Moves

- 2016-09-01 (dc14113f) replaced [[wasm-distinct-value-return-types]]: WASM primitive types were made aliases of B3::Type so a Vector of WASM types can be converted to a Vector of B3 types without translation. (code)
- 2016-09-07 (92cad735) replaced [[wasm-b3-block-continuation-pair-stack]]: Loops and branches require control frames to distinguish a block continuation from a loop backedge target and to unify branch result values at arbitrary control levels. (code)
- 2016-12-15 (1ed42e5a) replaced [[wasm-boolean-side-channel-errors]]: Returning Expected results makes parser, validator, and compiler helpers return either their success value or an error together instead of synchronizing boolean failure state with side-channel error strings. (code)
- 2016-12-22 (e8254436) replaced [[wasm-implicit-function-top-control]]: An explicit TopLevel BlockType replaced the parser's empty-control-stack end-of-function special case because top-level return semantics then share the same branch target representation as ordinary blocks. (code)
- 2017-03-30 (019f5194) replaced [[inline-wasm-b3-constants]]: Wasm B3 constants were pooled per function and inserted into the root basic block so repeated constants share one B3 value instead of being emitted at each use site. (code)
- 2017-04-14 (867a2ba7) replaced [[wasm-control-result-variable-values]]: Wasm control-flow result merging no longer needed B3 variables because the generated control-flow edges ensured each upsilon dominated its phi. (code)
- 2021-12-16 (3a6ead11) replaced [[wasm-fp-minmax-expanded-select]]: Represent Wasm min/max as B3 FMin/FMax so ARM64 can select fmin/fmax while non-ARM64 lowers the same opcode to the old semantic control flow. (code)
- 2023-02-02 (3eb123c1) dropped: default-enabled wasm tail calls: Wasm tail calls were disabled because wasm calls did not yet adjust the stack pointer after calls and B3/Air does not support changing StackSlot offsets. (code)
