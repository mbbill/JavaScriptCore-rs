# Rust JavaScriptCore — Status

A faithful C++→Rust rewrite of JavaScriptCore. **Goal:** JetStream 3 Octane
parity with local C++ `jsc` — `R = geomean(Rust)/geomean(C++ jsc) ≥ 1.0`, same
machine/inputs/scoring, all 15 benches passing first. **R is undefined until all
15 complete + validate** (zero throws / wrong answers); a partial suite yields no
geomean and parity must not be claimed.

**Recovery / where to read (a fresh session reads these, in order):**
1. `CLAUDE.md` — the contract (method, roles, principles, this read-order).
2. `README.md` (this file) — current status snapshot (bounded ~200 lines).
3. `docs/ROADMAP.md` — the plan: JIT-anchored dependency order + the % workload
   tracker + keystone status + what's next and why.
4. `docs/design/*.md` — durable keystone designs (JSStack, GC/R4, the scoreboard).
5. `git log` — the decision log (detailed per-batch evidence; the durable history).

The scoreboard instrument: `tools/octane-parity/run_cpp_baseline.sh` (C++ jsc) +
`run_rust_baseline.sh` (Rust), both through the identical Octane harness. The C++
baseline is re-measured on the machine, never assumed.

---

## Scoreboard (2026-06-28, iters=2/wc=1) — the only thing that defines "done"

**R = UNDEFINED.** Completion gate not met: 3/15 fail. 12/15 Rust complete+validate.

- **PASS (12)** `r_i = Rust/C++`: code-load 0.060 · regexp 2.4e-3 · navier 2.0e-3 ·
  crypto 1.3e-3 · gbemu 9.7e-4 · splay 9.2e-4 · richards 7.4e-4 · Box2D 6.8e-4 ·
  earley-boyer 5.4e-4 · delta-blue 5.2e-4 · pdfjs 3.3e-3 · raytrace 1.7e-4.
  Partial geomean ≈ 1.3e-3. (C++ jsc baseline: crypto 1611, richards 1240, navier
  1184, delta-blue 1072, code-load 962, regexp 750, splay 700, raytrace 690,
  earley-boyer 663, Box2D 462, pdfjs 261, gbemu 136, typescript 36, zlib 38.)
- **FAIL (3, all JIT-gated):** mandreel + octane-zlib (asm.js — DNF/timeout under the
  interpreter; need the JIT to *complete*); typescript (a latent **pure-interpreter
  value-divergence** throw — `TypeError: undefined is not an object` in the TS
  compiler; correctness bug, and also too-slow).

**Verdict (proven, not asserted):** compute-bound `r_i` sit at 5e-4–2e-3 (~500–6000×
slower than C++). The gap — and the completion gate itself (asm.js) — is **gated on
the optimizing JIT**. The interpreter cannot reach a defined R, let alone parity.

## Progress — ~40% by effort (full table in docs/ROADMAP.md)

Done: interpreter/runtime/parser/builtins (12/15 validate), the faithful foundation
(value/GC-arena/Structure/strings/profiling/bytecode), all 3 original throwers fixed,
the call-link O(N²)→O(1) correction, and the **full assembler codegen layer — the
engine emits, relocates, and executes ARM64 machine code under W^X**. The R scoreboard
exists. To do (~60%, all parity-bearing, ~0% started): the JIT — JSStack substrate
cutover + GC/R4 (the GC the JIT assumes) + the **baseline JIT** (per-opcode codegen) +
**DFG** + **FTL/B3**. The baseline JIT is where R first moves off ~0.001; DFG+FTL take
it to ≥1.0. See docs/ROADMAP.md.

---

## Subsystem status (condensed; legend below)

**Octane harness & correctness**
- [done] JetStreamDriver load order, shell globals, iteration, validation, scoring, probe surface.
- [done] All 3 original throwers fixed (faithful, C++-verified): regexp (full Yarr engine wired,
  simple_exec deleted, checksum validates), Box2D (Number/Math constants), gbemu (`new Function`),
  pdfjs (abstract-equality ToPrimitive). call-link per-site rewire landed earley-boyer + Box2D.
- [missing] typescript pure-interpreter value-divergence throw (differential-trace vs jsc to localize).

**Faithful foundation (built; mostly unwired behind dead_code)**
- [done] value → JSVALUE64 NaN-boxing (lossless double + immediates).
- [done] S4 cell arena (MarkedSpace/MarkedBlock/BlockDirectory/FreeList/PreciseAllocation, miri-proven)
  + SlotVisitor STW marking core — collector RUN-gated on R3/R4.
- [done] Structure leaf ports + Structure cell (StructureID/StructureIdTable/TypeInfoBlob/PropertyTable).
- [done] StringImpl Stage A (8/16-bit Latin-1/UTF-16, O(1) index).
- [done] profiling: ArithProfile + ExecutionCounter (faithful bitfields) + SpeculatedType u64 bitset.
- [done] bytecode: faithful packed instruction-stream core (Vec<u8>, byte-offset index, width-aware).

**Assembler / codegen (PROVEN end-to-end: emit → relocate → execute)**
- [done] AbstractMacroAssembler operands + RegisterID + ARM64 encoder (byte-oracle-proven).
- [done] LinkBuffer Label/Jump/Call + byte-exact in-place relocation.
- [done] W^X executable memory (MAP_JIT + pthread_jit_write_protect; emit→finalize→call returns 42);
  unsafe scoped to jit/unsafe_platform_boundary.rs (forbid→deny).

**JSStack execution substrate (the running JIT's frame model; native-thread-stack — see docs/design)**
- [done] B1 types + offset table + provenance gate; B2 live arena reservation + entry seeding + stack
  guard (byte-identity cross-check vs the live model passes). Fixed the jit/abi.rs callee-slot defect.
- [missing] B3 dual-write bridge → B4/B6 megafile read-flip + CallFrameId retirement → B7 wire the encoder.

**GC / value cutover (toward R4 — the arena cell identity the JIT emits)**
- [done] the arena + marking core (above), unwired.
- [missing] POD object-model rewrite (retire the fat CoreObjectCell) → R3 shadow oracle → R4 flip
  (gate = technical verification: shadow cross-check + miri + adversarial verify) → running collector.

**Baseline JIT / DFG / FTL (parity lives here; ~0% started)**
- [missing] wire arm64_baseline to emit per-opcode via the encoder/finalize (retire the byte-blob /
  re-interpreter shim) + the bytecode-stream cutover + profiling wiring + tier-up.
- [missing] DFG (bytecode→SSA→speculation→SpeculativeJIT+OSR); FTL + B3 + Air + register allocation.

**Structural fidelity**
- [done] Phase E: interpreter/mod.rs 41k→33k, all 4 runtime-class stores split to interpreter/*_store.rs.
- [wip] vm/mod.rs (74k) still oversized; existing Rust-only files/types need dedicated structure review.

**Runtime semantics (interpreter-level, broadly working for Octane)**
- [done] objects/structures/transitions/Butterfly; LLInt monomorphic Get/Put ICs; calls/constructs/
  BoundFunction; typed arrays (8 Number ctors); Math/Number/String/Array breadth; Yarr regexp engine.
- [missing] full AccessCase taxonomy (multi-hop/transition/megamorphic); full ArrayProfile/ArrayMode;
  full String.prototype + ropes; Date, modules/microtasks; [deferred] Wasm.

**[frozen]** ARM64 native-entry admission-proof cluster (cfg off-by-default; retained as JIT/GC salvage).

---

Legend: `[done]` implemented+verified for the stated scope · `[wip]` partial/expanding ·
`[missing]` not yet reliable · `[risk]` exists, needs fidelity/structure review ·
`[deferred]` intentionally later · `[frozen]` quarantined salvage.
