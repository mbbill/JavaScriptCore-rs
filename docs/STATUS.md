# Status — per-subsystem (agent working tracker)

The detailed current-state tracker, per subsystem. `README.md` is the human progress
view; this is the agent's working status tree. The *plan* is `docs/ROADMAP.md`; keystone
*designs* are `docs/design/`; the decision *log* is `git log`. Keep this STATUS, not
history; record where each subsystem stands, not what happened.

Legend: `[done]` implemented+verified for the stated scope · `[wip]` partial/expanding ·
`[missing]` not yet reliable · `[risk]` exists, needs fidelity/structure review ·
`[deferred]` intentionally later · `[frozen]` quarantined salvage.

## Octane harness & correctness
- [done] JetStreamDriver load order, shell globals, iteration, validation, scoring, probe surface.
- [done] All 3 original throwers fixed (faithful, C++-verified): regexp (full Yarr engine wired,
  simple_exec deleted, checksum validates), Box2D (Number/Math constants), gbemu (`new Function`),
  pdfjs (abstract-equality ToPrimitive). call-link per-site rewire landed earley-boyer + Box2D.
- [missing] typescript pure-interpreter value-divergence throw (`undefined is not an object` in the
  TS compiler); differential-trace vs jsc to localize the op that returns undefined.

## Faithful foundation (built; mostly unwired behind dead_code)
- [done] value → JSVALUE64 NaN-boxing (lossless double + immediates).
- [done] S4 cell arena (MarkedSpace/MarkedBlock/BlockDirectory/FreeList/PreciseAllocation, miri-proven)
  + SlotVisitor STW marking core — collector RUN-gated on R3/R4.
- [done] Structure leaf ports + Structure cell (StructureID/StructureIdTable/TypeInfoBlob/PropertyTable).
- [done] StringImpl Stage A (8/16-bit Latin-1/UTF-16, O(1) index).
- [done] profiling: ArithProfile + ExecutionCounter (faithful bitfields) + SpeculatedType u64 bitset.
- [done] bytecode: faithful packed instruction-stream core (Vec<u8>, byte-offset index, width-aware).

## Assembler / codegen (PROVEN end-to-end: emit → relocate → execute)
- [done] AbstractMacroAssembler operands + RegisterID + ARM64 encoder (byte-oracle-proven).
- [done] LinkBuffer Label/Jump/Call + byte-exact in-place relocation.
- [done] W^X executable memory (MAP_JIT + pthread_jit_write_protect; emit→finalize→call returns 42);
  unsafe scoped to jit/unsafe_platform_boundary.rs (forbid→deny).

## JSStack execution substrate (native-thread-stack — see docs/design/jsstack.md)
- [done] B1 types + offset table + provenance gate; B2 live arena reservation + entry seeding + stack
  guard (byte-identity cross-check vs the live model passes). Fixed the jit/abi.rs callee-slot defect.
- [missing] B3 dual-write bridge → B4/B6 megafile read-flip + CallFrameId retirement → B7 wire the encoder.

## GC / value cutover (toward R4 — see docs/design, the arena cell identity the JIT emits)
- [done] the arena + marking core (above), unwired.
- [missing] POD object-model rewrite (retire the fat CoreObjectCell) → R3 shadow oracle → R4 flip
  (gate = technical verification: shadow cross-check + miri + adversarial verify) → running collector.

## Baseline JIT / DFG / FTL (parity lives here; ~0% started)
- [missing] wire arm64_baseline to emit per-opcode via the encoder/finalize (retire the byte-blob /
  re-interpreter shim) + the bytecode-stream cutover + profiling wiring + tier-up.
- [missing] DFG (bytecode→SSA→speculation→SpeculativeJIT+OSR); FTL + B3 + Air + register allocation.

## Structural fidelity
- [done] Phase E: interpreter/mod.rs 41k→33k, all 4 runtime-class stores split to interpreter/*_store.rs.
- [wip] vm/mod.rs (74k) still oversized; existing Rust-only files/types need dedicated structure review.

## Runtime semantics (interpreter-level, broadly working for Octane)
- [done] objects/structures/transitions/Butterfly; LLInt monomorphic Get/Put ICs; calls/constructs/
  BoundFunction; typed arrays (8 Number ctors); Math/Number/String/Array breadth; Yarr regexp engine.
- [missing] full AccessCase taxonomy (multi-hop/transition/megamorphic); full ArrayProfile/ArrayMode;
  full String.prototype + ropes; Date, modules/microtasks; [deferred] Wasm.

## [frozen]
- ARM64 native-entry admission-proof cluster (cfg off-by-default; retained as JIT/GC salvage).
