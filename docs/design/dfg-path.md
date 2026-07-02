# Design — The path to the optimizing JIT (DFG), where R lives (ratified 2026-06-30)

Outcome of the post-baseline-win strategic assessment (workflow
strategic-assessment-post-baseline-win, 4 read-only surveys → synthesize →
anti-anchor critique; verdict endorse-with-adjustments, on the R-critical-path).
All load-bearing claims below were re-verified against source by the orchestrator.

## The one fact that sets the direction

The native baseline JIT is now a measured NET WIN over the **interpreter**
(geomean ~1.086 execoff/interp on the 5 compute benches; scoreboard LATE-5). But
the interpreter is ~500–6000× slower than C++, so a 1.086× interpreter is still
R ≈ 1e-3. **R ≥ 1.0 lives ONLY in the optimizing JIT (DFG → FTL/B3).** Every
near-term unit is therefore a *precursor* to that tier; the tiebreaker among
precursors is which unblocks the parity-bearing tier soonest WITHOUT entrenching
a divergence (Prime Focus #2).

## DFG readiness (verified)

`src/dfg/` EXISTS but is **descriptor-only** scaffolding (~1.7k lines): it
reserves the ownership shape for IR graph / speculation / OSR / invalidation
"without compiling or executing optimized code" (dfg/mod.rs:1). ~29 abstract
`DfgNodeKind`s + a `Bytecode(Opcode)` passthrough vs JSC's 497 node types
(graph.rs:101 vs DFGNodeType.h). There is **no** bytecode→DFG parser, no
lowering, no codegen, no SSA/CFA/Fixup, no SpeculativeJIT, no live OSR. Of the
DFG's hard prerequisites:

- **Value representation** — DONE + LIVE (faithful NaN-box, repr.rs; RuntimeValue
  = JsValue is the live interpreter value).
- **Bytecode source** — DIVERGENT. The live interpreter runs a type-specialized
  `CoreOpcode` stored as a **Vec-by-ordinal** (opcode.rs / instruction.rs). The
  faithful flat **packed byte stream** the DFG must lower from exists but is
  additive + UNWIRED (`#![allow(dead_code)]`, instruction_stream.rs). **This is
  the #1 representation divergence.** (Note: `BytecodeIndex` is ALREADY
  byte-offset-faithful — code_block.rs:74 `from_offset`/`offset()`,
  checkpoint-packed — so the *index* the parser/OSR/profiles key on is correct;
  only the *storage* + opcode specialization diverge.)
- **Profiling data structures** — faithful ports (profiling.rs: ValueProfile.h /
  ArithProfile.h) and **metadata-displacement-keyed** (metadata_table_displacement,
  value_profile_bucket_storage_index) so the keying **survives the cutover**. But
  runtime POPULATION is ~5% wired: only call-result value profiles are recorded
  live (vm/mod.rs:15108); ArithProfile has ZERO live callers; ArrayProfile.observe
  writes only a test field.
- **SpeculatedType lattice** — a faithful uint64-bitset port exists
  (speculated_type.rs) but is UNWIRED and competes with TWO divergent reps
  (dfg/speculation.rs enum; profiling.rs bare-u64 set) — a flagged 3-way divergence.
- **OSR exit / baseline-as-bailout** — descriptor-only; no live exit, no frame
  reconstruction. The baseline is NOT yet a sound bailout landing tier:
  `emit_prologue` is prologue-only (no OSR entry), `codeBlock`@2 left unwritten
  (function_emitter.rs:1534), whole-function decline sink `other =>
  UnsupportedOpcode` (function_emitter.rs:2936) means a DFG exit may have no
  landing site. The crypto/LoadCallee revert is direct evidence the wrong-answer
  risk concentrates HERE (seed/parked-pointer hazards).

## The dependency order to R ≥ 1.0

Precursors (1)–(4) are FAITHFUL, unblocked today, and **parallel-safe with each
other** (the anti-anchor correction: the cutover does NOT gate the others,
because BytecodeIndex + profiling keying already survive it). Do NOT run them as
one serial XL track (Prime Focus #3).

1. **Packed-bytecode-stream LIVE cutover** — correct the #1 representation
   divergence: make instruction_stream.rs the live interpreter/baseline dispatch
   source, freeze the type-specialized Vec-by-ordinal `CoreOpcode`. SERIAL /
   orchestrator-owned. The two HARD couplings must be resolved as the gating
   design decision (NOT deferred): the `Fits<VirtualRegister>` constant-register
   remap (Fits.h:118-156) and the `UnlinkedMetadataTable`/`MetadataTable` wiring
   (flagged instruction_stream.rs:27-37). The first commit-sized wedge MUST pick
   an opcode family that FORCES at least one coupling (a metadata-bearing or
   constant-register-operand opcode) — a metadata-free family is de-risking
   theater (it exercises neither coupling, the real risk).
2. **Canonicalize the SpeculatedType lattice** onto the faithful uint64 bitset
   (speculated_type.rs); retire the divergent enum + bare-u64 set. Parallel-safe,
   M cost; corrects a 3-way divergence while few dependents.
3. **Populate runtime profiles** across the full JSC site set (ArithProfile
   .observeResult, get_by_id/get_by_val/by_val arrays, beyond call-results),
   keyed by the already-faithful metadata-displacement BytecodeIndex. Removes DFG
   hard-dep #2. Parallel-safe (keying survives the cutover).
4. **Baseline-as-bailout soundness** (the OSR-exit landing tier) — the
   highest-UNCERTAINTY long-pole, unblocked today, where the crypto-class
   wrong-answer risk concentrates: real frame headers (codeBlock@2, real Callee),
   eliminate whole-function decline so an exit always has a landing site, OSR-entry
   trampoline, inline write barriers. Audit-first to scope it. Required before
   SPECULATIVE DFG runs (not before the first non-speculative graph).
5. **First DFG bytecode→LoadStore-IR parser** — single-basic-block, non-speculative,
   falling back to baseline otherwise (JSC's own first-DFG scoping, 08a80d90); new
   src/dfg/parser.rs into the existing DfgGraph. Unblocked once (1) lands.
6. **DFG speculation** — prediction injection + Fixup/CFA/SSA/Phi-Upsilon + LIVE
   OSR-exit + tier-up Plan/Worklist. **First tier that actually moves R.**
7. **FTL/B3** — the parity-bearing top tier where R ≥ 1.0 is reached.

## Deferred (with reasons)

- **The default flip + the 5 declined asm.js opcode admits** (the flip survey
  found mandreel/octane-zlib still DNF on UnsignedRightShiftInt32 / ModNumber /
  LogicalNot / BitNotInt32 / standalone LessEqualInt32, + an op_urshift evaluator
  fix): DEFER until a real tier produces measurable per-bench movement. The flip
  only DEFINES R; it never lifts it off the ~1e-3 interpreter floor, and per-bench
  r_i at the floor do not localize the gap (the DFG is already named as where the
  gap lives). **This FALSIFIES the ROADMAP/STATUS "15/15 gate is K2-free / asm.js
  tiers up WHOLE" claim** — corrected in the trackers.
- **Baseline opcode-admit fan-out** (re-admit LoadCallee, cheap value ops, Load*,
  array shims): DEFER — they widen the baseline over the INTERPRETER only, off the
  R path. EXCEPTION (cheap, worth doing standalone): thread `owning_code_block`
  into operation_get/put_global_object_property (vm/mod.rs:3377/3421) to close the
  LAST parked-CodeBlock divergence (same crypto-class hazard as the LoadCallee
  revert) while few dependents.
- **Heavy-bench execoff runs** (pdfjs/typescript/splay) for any verification:
  DEFER behind GC completeness (symbol/bigint leaf U2/U3 + visitWeak U7) or a hard
  RSS cap. Gate all near-term units with cargo test --lib + tiny single-BB probes
  + C++ source comparison, never a full-suite run.

## Open questions (all three resolved 2026-07-01 — see the Ratified section)

- The profiling-state survey reader FAILED (returned a placeholder). → DONE: the
  focused profiling audit ran; its hard finding is below, and F0 (profile
  storage + derivation) landed from it (29d7c40).
- Wedge-family choice for the cutover. → RESOLVED: the raw mov/ret wedge landed
  (50c70fa — forces the Fits constant-band coupling) + correctness hardening
  (e6ae867).
- Does the first non-speculative single-BB parser need populated profiles? →
  ANSWERED: no — type-agnostic; see below.

## Ratified 2026-07-01 (serial decisions from the bailout / dfg-parser / profiling audits)

- **U0 exit-target: the first DFG OSR exit lands in the INTERPRETER** — JSC's own
  exitToLLInt analog (DFGOSRExitCompilerCommon.cpp:461-479: when no baseline code
  exists, JSC exits to the LLInt). This makes baseline whole-function decline
  harmless as an exit-target concern. The track-4 hard gate (U0 + JITCodeMap +
  exit-site frame reification) sits before SPECULATIVE DFG only, NOT before the
  first non-speculative parser.
- **Open question 3 ANSWERED with parser-source evidence:** the first single-BB
  non-speculative parser lowers type-agnostically with SpecNone — none of
  enter/mov/add/sub/mul/ret calls getPrediction, and makeSafe is a sound no-op on
  an absent ArithProfile (DFGByteCodeParser.cpp:1126). So profile POPULATION
  defers behind the parser slice, PROVIDED the slice declines every getPrediction
  opcode.
- **New hard finding (profiling audit):** an EMPTY value profile →
  getPrediction()==SpecNone → addToGraph(ForceOSRExit)
  (DFGByteCodeParser.cpp:1050-1061), which poisons the whole function —
  value-profile population hard-gates the DFG on real code the moment the parser
  widens past the toy slice.
- **Parser architecture:** faithful NodeType enum (landed d935dde); the parser
  owns a mutable working graph; MovHint/delayed-SetLocal ported faithfully.

## Ratified 2026-07-02 (generator track + parser landing + population/GC rounds)

- **Generator track: generate, don't hand-roll (DONE, G1-G3).** `BytecodeList.rb` (run through
  JSC's own `generator/*.rb`) is the LITERAL source of truth for the opcode table, not a
  hand-transcribed Rust table. G1 (`833592d`) grew `OperandKind` to the full faithful 18-variant
  stream-operand set (VirtualRegister/UnsignedImmediate/BoundLabel/ECMAMode/OperandTypes/
  SignedImmediate/GetPutInfo/IndexingType/SymbolTableOrScopeDepth/ResolveType/…) with each kind's
  `Fits<T>` semantics ported from `Fits.h`. G2 (`ee174a7`) built `tools/bytecode-gen/generate.rb`,
  which loads and runs the checkout's generator modules VERBATIM (zero WebKit files modified) and
  emits all 193 opcode rows, verified 193/193 id/name/length + constants against the measuring
  instrument's build artifact. G3 (`7accf10`) `include!`s the generated table into
  `instruction_stream.rs` and makes it the crate's live `OPCODE_TABLE` — the packed stream now
  DECODES every JSC bytecode; the hand-written 11-row table is gone. Remaining (G4, a separate
  serial track): the `CoreOpcode` identity cutover (~8k refs) and per-opcode execution admission —
  decode-only today except the mov/ret wedge.
- **The parser LANDED** (`c164345`, `src/dfg/parser.rs`): the single-BB non-speculative slice
  described above is real, not a plan. It mirrors `ByteCodeParser` fn-for-fn (parse/parseBlock,
  get/getLocalOrTmp/getArgument with `variablesAtTail` reuse, the two-phase MovHint + delayed-
  SetLocal queue, `flushForReturn`, `makeSafe`, `addToGraph` with `clobbersExitState`).
  **The add-predicate question is RESOLVED** (settles the earlier scoping audit's claim): `op_add`
  gates on `hasNumberResult()` being EXACT `NodeResultNumber` (`DFGNode.h:1741-1744`) —
  `GetLocal`/`JSConstant` are `NodeResultJS` and `DoubleConstant` is `NodeResultDouble`, so this
  slice can ONLY emit `ValueAdd`/`ValueSub`/`ValueMul`, never `ArithAdd`. The earlier audit's
  "1.5+2.5 → ArithAdd" claim was WRONG; `Arith*` strengthening is FixupPhase's prediction-driven
  job, out of scope for this slice. Adversarial verify fixed one MEDIUM finding in-batch: `makeSafe`
  must NOT merge flags into `ValueSub` (C++'s Add-arm result-shape labels cover
  ArithAdd/ArithSub/ValueAdd but ValueSub falls to `default: break`, DFGByteCodeParser.cpp:1204-1205).
- **Population round COMPLETE — the `SpecNone→ForceOSRExit` gate is now CLOSED for the wired opcode
  set.** Four parallel units landed: unary arith (`c650d48`), named-loads/scope value profiles
  (`8a2b5e7`), binary arith (`1f53724`), by-val/length value+array profiles (`5f45ab9`). Value,
  array, and arith profiles now record LIVE at LLInt-faithful sites — the hard finding from the
  profiling audit (an empty profile poisons the whole function via `ForceOSRExit`) no longer
  applies to this set. Two prompt-belief corrections proved from the LLInt asm and independently
  verifier-confirmed: LLInt FAST paths for add/sub/mul DO OR arg-observed-type bits (only
  RESULT-shape observation is slow-path-exclusive); `sub`/`mul`/`div` slow paths do NOT call
  `observeLHSAndRHS` (only add/lshift/rshift/bitand/bitor/bitxor do). Remaining, non-hard-gating:
  U8 argument profiles (separate arg-index-keyed model), the getter-resume value-profile write,
  construct-result profiles.
- **GC round COMPLETE — the frozen-value arena precondition moves.** The arena is now leak-free
  end-to-end (object/string/bigint/symbol all reclaim, `354cb89`/`c9c3227`; weak-finalization seam
  `3ad0ab7`; CodeBlock constant-pool rooting `f213265` — see docs/design/gc-r4.md's status-
  correction header). The DFG-GC items that remain OPEN, co-sequenced with the plan/speculation
  units rather than blocking the parser: a frozen-value arena (JSC freezes constants/structures
  the DFG embeds so the GC can't move or collect them mid-compile) and `DfgCommonData` real handles
  (today the parser stubs graph/owner identity, dfg-path.md "Supporting faithful edits" above).

Authority: C++ JSC dfg/ (DFGByteCodeParser.cpp, DFGGraph.h, DFGOSRExit, DFGNodeType.h),
bytecode/ (InstructionStream.h, Instruction.h, BytecodeIndex.h, Fits.h, BytecodeList.rb,
generator/*.rb), profiler/; mcts_mem/javascriptcore/dfg.md,
mcts_mem/javascriptcore/instruction-format.md. Builds on docs/design/scoreboard.md (LATE-5) +
gc-r4.md + baseline-property-ic.md.
