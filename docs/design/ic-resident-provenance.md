# Design — IC-resident provenance: retiring the per-attempt telemetry-log cluster

Status: **DRAFT, ratifiable**. Synthesizes three measured-and-deferred efforts into one
coordinated initiative: the call-link pipeline (Unit 2c, paused in `d0bd496`), the
property-IC attachment cluster (Units 4/5, deferred in `58325a3`), and the
materialization/readiness split (Unit 1's evidence-backed revert). All three share one
root cause and one fix: **JSC never logs inline-cache provenance — it stores it, in
place, on the site object itself, and mutates it under a lock.** The Rust tree instead
grew ~15 VM-global `Vec<...Record>` logs cross-referenced by hand-rolled `u64` ordinals,
because the resident structs (`CallLinkInfo`, `StructureStubInfo`) were ported with only
their *steady-state* fields, not JSC's *in-flight-attempt* fields. This document adds the
missing fields, faithfully, and specifies the fold that lets every log disappear.

## Why this cluster, why now

Three prior batches independently hit the same wall and each correctly refused to patch
around it:

- **`d0bd496`** (call-link, Unit 2b) deleted a Rust-only 7-ordinal *identity* chain but
  paused the full fold: "every pipeline stage cross-references ordinals across 4 vecs
  inside the validators... a faithful atomic collapse rewrites ~15+ functions + 33
  tests." It ratified two things this document now executes: the fold happens, and
  `Option<CallLinkAttachmentPlan>` lives on `CallLinkInfo` (`src/bytecode/ic.rs`).
- **`26d48a6`** (property observations, Unit 3) proved the pattern works: two unbounded
  `.push()`-per-access logs became update-or-insert-by-site state
  (`property_inline_cache_evolution_states`), matching `ValueProfile`/`ArrayProfile`'s
  "last-wins bucket, never a log" shape. It is the load-bearing precedent for this
  document's method, including its failure mode: two consumers genuinely needed a
  historical snapshot, not latest-state, and were fixed with an immutable descriptor
  captured once at the plan record — closer to JSC's self-contained `AccessCase` than an
  ordinal re-fetch. This document's design must survive the same adversarial check.
- **`58325a3`** (Units 4/5) investigated `property_inline_cache_attachment_records` (11
  consumers, one hard `.expect()`) and `structure_stub_access_case_links` (memoizes
  *rejected* attempts) and explicitly deferred both, writing: "the faithful fix is a
  coordinated resident-provenance initiative... queued as its own designed effort, not
  forced piecemeal." This document is that effort. It also resolves 58325a3's open
  question — see "the crux finding" in the `PropertyInlineCache` section below — with a
  finding that changes the shape of the fix `58325a3` assumed.

## C++ ground truth (read directly from `/Users/bytedance/Dev/WebKit/Source/JavaScriptCore`)

**Naming note:** the classic JSC names `StructureStubInfo.h`/`PolymorphicAccess.h` (used
by `mcts_mem` and by prior commit messages) do not exist as separate files in this
checkout — upstream JSC merged/renamed them. The class that plays `StructureStubInfo`'s
role is now `class PropertyInlineCache` (`bytecode/PropertyInlineCache.h:131`, with
`resetByGC : 1` at line 475 — the exact field the Rust `StructureStubInfo::reset_by_gc`
already mirrors, confirming the mapping). `PolymorphicAccess` and `InlineCacheCompiler`
now live together in `bytecode/InlineCacheCompiler.h`/`.cpp`. Cite the real files below;
the Rust type names (`StructureStubInfo`, `AccessCase*`) keep the classic names the
project's docs already use, which is fine — the mapping is 1:1, just cross-referenced
here once so nobody goes looking for a file that no longer exists.

### `CallLinkInfo` — one struct per call site, mutated in place

`bytecode/CallLinkInfo.h:58` (`class CallLinkInfo`). Relevant fields: `enum class Mode`
(`:69`, Init/Monomorphic/Polymorphic/Virtual — **no separate "Direct" mode**; direct
calls are a *different* class, `DirectCallLinkInfo`, `:391+`, with its own
`m_codeBlock`/`m_target` — noted below as an adjacent, out-of-scope divergence),
`m_hasSeenShouldRepatch : 1` (`:300`, exposed via `seenOnce()`/`setSeen()`/`clearSeen()`
at `:163-175`), `m_codeBlock` (`:310`, "weakly held, cleared whenever
`m_monomorphicCallDestination` changes"), `m_monomorphicCallDestination` (`:311`). There
is no bounded *or* unbounded history: exactly these fields, mutated in place.

`bytecode/RepatchInlines.h`, `linkFor` (read in full): one function, no persisted
intermediate stage. It takes the **live callee** as a normal argument (`calleeFrame->
guaranteedJSValueCallee()`), resolves the callee's executable/codeBlock/entry pointer
right there, then does exactly one `switch (callLinkInfo->mode())`:

```
Init:                  !seenOnce() ? setSeen() : linkMonomorphicCall(...)
Monomorphic/Polymorphic: isCall ? linkPolymorphicCall(...) : setVirtualCall(...)
Virtual:                (no-op; already virtual)
```

It **always returns `codePtr`** for this one invocation regardless of whether linking
happened — the call proceeds either way; linking is a side effect performed once, not a
gate the current call waits on. There is no "readiness" record, no "descriptor" record,
no "boundary validation" record, no "install recheck" record: the live callee/executable/
codeBlock ARE the evidence, read fresh every call, and the mutation is the one committed
fact. This is the shape Unit 2c/`d0bd496` already pointed at; this document specifies it
precisely enough to implement.

### `PropertyInlineCache` (a.k.a. `StructureStubInfo`) — bounded resident buffering, not a rejection log

This is the crux finding. `PropertyInlineCache::considerRepatchingCacheImpl`
(`bytecode/PropertyInlineCache.h:248-342`, read in full) is what decides whether a
property-IC miss gets a new `AccessCase` at all:

```
everConsidered = true;
if (countdown == 0) {
    repatchCount++;                                  // saturating
    if (repatchCount > Options::repatchCountForCoolDown()) {
        repatchCount = 0;
        countdown = coolDown(initialCoolDownCount, numberOfCoolDowns++);  // exponential backoff
        bufferingCountdown = 0;
        return true;                                  // trigger generation now
    }
    if (bufferingCountdown == 0) return true;          // don't buffer forever
    bufferingCountdown--;
    if (!structure) return true;
    isNewlyAdded = dedup structure.id() into m_bufferedStructures;  // Vector<StructureID>
    return isNewlyAdded;                                // false if already buffered THIS cycle
} else {
    countdown--;
    return false;                                       // not yet time to repatch
}
```

`m_bufferedStructures` (`:438`, `Variant<monostate, Vector<StructureID>,
Vector<tuple<StructureID, CacheableIdentifier>>>`) is a **bounded, per-cycle dedup set**,
not a permanent memo. Its own doc comment (`:434-437`) states the design intent
explicitly: *"it's always safe to clear this. If we clear it prematurely, then if we see
the same structure again during this buffering countdown, we will create an AccessCase
object for it. That's not so bad — we'll get rid of the redundant ones once we
regenerate."* `clearBufferedStructures()` (`:346-357`) is called after every successful
stub regeneration.

`PolymorphicAccess::addCases` (`InlineCacheCompiler.cpp:8401-8484`, read in full) confirms
the other half: it dedups a newly-proposed case only against the **currently buffered/
installed list** (`m_list`, via `AccessCase::canReplace`), never against a history of
past rejections. If `casesToAdd` ends up empty it returns `MadeNoChanges` — "tell the
caller to just keep doing what they were doing before" — no record is kept of what was
tried and failed. `AccessGenerationResult::Kind` (`InlineCacheCompiler.h:54-60`) is
`MadeNoChanges | GaveUp | GeneratedNewCode | GeneratedFinalCode |
ResetStubAndFireWatchpoints` — five outcomes of the CURRENT attempt, nothing historical.

**Conclusion: JSC does not memoize permanently-rejected access-case attempts.** It
re-attempts, gated by a bounded buffered-structure dedup set (cleared every regeneration
cycle) plus a per-site repatch cooldown that escalates exponentially
(`repatchCount`/`numberOfCoolDowns`/`countdown`). `58325a3`'s two refuting tests assumed a
resident-only design must either (a) memoize rejections forever or (b) re-attempt
permanently-failing candidates on every safepoint with no throttle — both were reasonable
fears given the *fields being tested didn't exist yet*. The faithful answer is (c): port
the countdown/cooldown/buffering fields C++ actually has, and re-attempts become
bounded and correct BY CONSTRUCTION, matching JSC exactly. This resolves the blocker.

The resident fields `PropertyInlineCache` carries for this (`PropertyInlineCache.h:463-
477`) and that Rust's `StructureStubInfo` (`bytecode/ic.rs:927-943`) currently lacks
**entirely**:

```cpp
CacheType m_cacheType { CacheType::Unset };
uint8_t countdown { 1 };
uint8_t repatchCount { 0 };
uint8_t numberOfCoolDowns { 0 };
uint8_t bufferingCountdown;
Variant<monostate, Vector<StructureID>, Vector<tuple<StructureID,CacheableIdentifier>>>
    m_bufferedStructures;      // guarded by m_bufferedStructuresLock
bool resetByGC : 1 { false };  // Rust already has this
bool tookSlowPath : 1 { false };
bool everConsidered : 1 { false };
```

The actual `AccessCase` **payload** (structure/offset/kind/watchpoint set) is NOT one of
these fields — it lives in the separate `PolymorphicAccess::m_list:
Vector<RefPtr<AccessCase>>`, populated by `addCases` and consumed by `compile()`
(`InlineCacheCompiler.h:268`, the regenerate-equivalent: takes the buffered list +
existing stub, produces a new `AccessGenerationResult` and installs the resulting code).
Rust already has this half faithfully: `InlineCacheStub.cases: Vec<AccessCaseDescriptor>`
(`src/jit/ic.rs:3655-3666`) is the resident `PolymorphicAccess::m_list` analog — real
generated-stub metadata (`id`, `kind`, `owner_slot`, `code: Option<JitCodeId>`, `tier`),
not a VM-global log. `StructureStubInfo.access_cases: Vec<AccessCaseRef>`
(`bytecode/ic.rs:940`) already references it by id (confirmed: `AccessCaseRef(stub.id.0)`
at `tiering.rs:17913` wraps an `InlineCacheStub`'s own id, not a log ordinal). **So the
case-payload skeleton is already resident and already faithful.** What's missing is only
the countdown/buffering/considered fields above — the "should we even try" gate that
`structure_stub_access_case_links` currently fakes with a growing per-attempt log.

### Watchpoint dependents (composes with, does not overlap, this design)

`property_load_guard_watchpoint_materializations` / `_invalidations` /
`_event_dispatches` (`src/vm/tiering.rs:237-242`) are explicitly out of this document's
scope — a code comment at `tiering.rs:5485-5492` already names the fix: *"Unbounded
growth is the known pre-existing divergence; the permanent fix is the watchpoint
dependents-list unit (redesign Unit 6), which retires this log."* That comment also notes
today's `.find()`-by-ordinal consumer is `property_inline_cache_clear_requests_for_
watchpoint_dispatch` (`tiering.rs:6106-6187`), which chains three ordinal scans
(invalidation → materialization → attachment filter) to build clear requests. Unit 6's
job is to give `StructureStubInfo`/`AccessCase` a direct `dependent_watchpoint_sets`-
shaped field (JSC's `AccessCase` holds its own `additionalSet()`/watchpoints — not
independently re-verified here, in Unit 6's scope, not this document's). **This document
reserves the composition point**: the new `StructureStubInfo` fields below do not touch
watchpoint-dependents state, and the property-IC fold's clear path is written to consume
"the site's current dependents" through whatever accessor Unit 6 lands, not through an
ordinal.

## The three log clusters → resident field map

### A. Call-link pipeline (8 vecs in `src/vm/tiering.rs:220-228` → `CallLinkInfo`)

| Log vec (current) | Struct + fields (read directly) | Resident replacement |
|---|---|---|
| `call_observations: Vec<VmCallObservationRecord>` (`:220`) + `vm_owned_call_target_validation_records` (`:221`) | per-call "what callee did we actually see" | **Deleted.** Not a field at all — becomes the live `callee: JSValue`/`ObjectId` **parameter** threaded straight into the one atomic attempt function, exactly like C++ `linkFor`'s `calleeFrame->guaranteedJSValueCallee()`. `d0bd496`'s "authorization re-derivation TRACED" finding already said this: callee/executable/outcome describe *which callee this call actually invoked* — an argument, never a log. |
| `call_link_readiness_records: Vec<VmCallLinkReadinessRecord>` (`:222`; fields: `ordinal, owner, frame, bytecode_index, opcode, bytecode_snapshot, observation_ordinal, call_link_descriptor_ordinal: Option<u64>, blockers: CallLinkReadinessBlockers`) | "is this site ready to link" | **Deleted as a record.** `blockers` becomes the return value of a `CallLinkInfo::readiness_blockers(&self, live_target: ...) -> CallLinkReadinessBlockers` method — computed fresh from live state every call (JSC never caches readiness either: `linkFor`'s switch IS the readiness check). |
| `call_link_descriptor_records: Vec<VmCallLinkDescriptorRecord>` (`:223`; carries a `CallLinkInfoDescriptor` + `lifecycle: MetadataOnly \| RetiredByClear{clear_ordinal, attachment_ordinal}`) | a snapshot of site+target metadata | **Deleted.** `CallLinkInfoDescriptor` (`src/jit/ic.rs:3754`) becomes a value computed on the stack inside the attempt function, never stored with a lifecycle of its own — there is nothing to "retire by clear" once nothing outlives the attempt. |
| `call_link_boundary_validation_records: Vec<VmCallLinkBoundaryValidationRecord>` (`:224`; `Accepted{target, boundary, descriptor, remaining_blockers} \| Rejected(...)`) | ABI/boundary compatibility check | **Deleted as a record**, kept as a **function** (`validate_call_boundary(descriptor, target) -> Result<...>`) called inline by the atomic attempt, mirroring `linkFor`'s inline entrypoint-kind checks (`isHostFunction()`, arity, `entrypointFor(...)`) — those are plain conditionals in C++, not a persisted validation record. |
| `call_link_attachment_plan_records: Vec<...>` (`:225`, wraps `Box<CallLinkAttachmentPlan>` from `src/jit/ic.rs:3780`) | the plan about to be committed | **`Option<CallLinkAttachmentPlan>` computed and consumed within the SAME attempt call — never a durable Vec entry.** See "Open question 2" on whether the full `CallLinkAttachmentPlan` struct (including its `stub: InlineCacheStub`) needs to sit as a literal field on `CallLinkInfo`, or whether `CallLinkInfo`'s own fields (`target`, `mode`, `flags`) already ARE the plan's committed projection, matching `linkFor`'s "no separate plan object" reality. |
| `call_link_attachment_install_rechecks: Vec<VmCallLinkAttachmentInstallRecheckRecord>` (`:226`) | "is the plan still valid at install time" | **Deleted outright.** This stage exists ONLY because plan and install were two separate ticks mediated by a Vec. `linkFor` runs plan-and-commit as one function under one mutation-authority hold (`CodeBlockMutationAuthority::ConcurrentJsLocker`, `src/bytecode/code_block.rs:495-502` — already ported, already the right lock granularity); once folded, there is no time gap to recheck across. |
| `call_link_inline_cache_attachment_records: Vec<VmCallLinkInlineCacheAttachmentRecord>` (`:227`) | the committed attachment | **`CallLinkInfo` mutated in place IS the attachment.** `set_monomorphic_callee`/`reset_to_unlinked` (already exist, `bytecode/ic.rs:1064-1084`) are the faithful `setMonomorphicCallee`/`reset` analogs. Success/failure is the attempt function's `Result` return, consumed once by the caller, never re-queried later by ordinal. |
| `call_link_inline_cache_clear_records: Vec<VmCallLinkInlineCacheClearRecord>` (`:228`) | the committed clear | **`CallLinkInfo::reset_to_unlinked()`'s return/side-effect IS the clear.** No log entry needed for later lookup — nothing today's code looks up a *historical* clear by ordinal for (verify in Unit R1's audit pass; if a genuine historical consumer exists, it needs the same "immutable snapshot on the SAME record" treatment `26d48a6` used for its two surviving-history cases, not a revived log). |

**The generated-call-link sidecar's legitimate multi-candidate need** (the ratified
`d0bd496` finding: `execute_generated_call_link_sidecar_probe_with_host` genuinely probes
multiple simultaneous candidates at one `bytecode_index`, filtered by callee, because a
call site can be polymorphic) is **not** a case for reviving a VM-global log. JSC's own
answer to "one call site, multiple recently-seen callees" is `CallLinkInfo::Mode::
Polymorphic` plus a bounded `PolymorphicCallStubRoutine` variant list (capped, not
logged) built by `linkPolymorphicCall`. **Recommendation** (Open question 5): re-model the
sidecar's multi-candidate table as `CallLinkInfo`'s own bounded polymorphic list, sized
like JSC's cap, rather than as a side `Vec` keyed by ordinals. This needs one more C++
citation (`linkPolymorphicCall`'s exact cap constant) before it's committed to — flagged
for the orchestrator, not assumed here.

**Diagnostics** (`shell/octane.rs` reads `.len()` on every one of these vecs today, e.g.
`:593-596`) become **write-time cumulative `u64` counters** on `VmTieringIntegration`,
exactly the `26d48a6`/`property_inline_cache_evolution_decision_counts` pattern already
landed: `call_link_attachments_accepted_total`, `call_link_attachments_rejected_total`,
`call_link_clears_total`. `CallLinkInfo::slow_path_count`/`bump_slow_path_count`
(`bytecode/ic.rs:1049-1053`, already faithful to `CallLinkInfo::m_slowPathCount`) is the
existing precedent for "small resident counter, not a log."

### B. Property-IC attachment cluster → new fields on `StructureStubInfo`

Add, on `bytecode/ic.rs`'s `StructureStubInfo` (`:927-943`), the `PropertyInlineCache`
fields identified above, ported field-for-field:

```rust
pub struct StructureStubInfo {
    // ...existing fields unchanged (bytecode_index, key, base_structure, kind,
    // cache_state, code_origin, access_cases, reset_by_gc, ...)...

    /// C++ `PropertyInlineCache::countdown` (PropertyInlineCache.h:468): repatch
    /// once this hits 0; init 1 (patch after first execution).
    pub countdown: u8,
    /// C++ `repatchCount` (:469): saturating count toward the cool-down threshold.
    pub repatch_count: u8,
    /// C++ `numberOfCoolDowns` (:470): exponential-backoff generation counter.
    pub number_of_cool_downs: u8,
    /// C++ `bufferingCountdown` (:471, init `Options::initialRepatchBufferingCountdown()`).
    pub buffering_countdown: u8,
    /// C++ `m_bufferedStructures` (:438): bounded per-cycle dedup set, NOT a
    /// rejection memo — cleared every regeneration (`clearBufferedStructures`,
    /// PropertyInlineCache.h:346-357). Structure IDs only when the site has no
    /// identifier component; (StructureId, PropertyKey) pairs otherwise, mirroring
    /// the C++ Variant<monostate, Vector<StructureID>, Vector<tuple<...>>>.
    pub buffered_structures: PropertyInlineCacheBufferedStructures,
    /// C++ `everConsidered : 1` (:477).
    pub ever_considered: bool,
    /// C++ `tookSlowPath : 1` (:478).
    pub took_slow_path: bool,
    // reset_by_gc already exists (:941) and is the C++ `resetByGC : 1` analog.
}

pub enum PropertyInlineCacheBufferedStructures {
    Unset,
    Structures(Vec<StructureId>),
    StructuresWithKey(Vec<(StructureId, PropertyKey)>),
}
```

Then port `considerRepatchingCacheImpl`'s exact arithmetic as
`StructureStubInfo::consider_repatching(&mut self, structure: Option<StructureId>, key:
Option<PropertyKey>) -> bool`, and `addCases`'s dedup-against-`m_list` (already
`InlineCacheStub.cases`, see above) as the existing IC-materialization path's guard,
unchanged in shape, just no longer logging every attempt to a side Vec.

| Log vec / cluster (current) | Resident replacement |
|---|---|
| `structure_stub_access_case_links: Vec<VmStructureStubAccessCaseLinkRecord>` (`tiering.rs:13014`, memoizes `Rejected{reason}` outcomes permanently) | **Deleted.** Replaced by `consider_repatching`'s `bool` return (transient) plus the bounded `buffered_structures` field. A rejection is a `false` return, nothing persisted beyond the bounded per-cycle set — faithful to the finding above. |
| `property_inline_cache_attachment_records: Vec<VmPropertyInlineCacheAttachmentRecord>` (`tiering.rs:12768`, ~11 consumers, the hard `.expect()` site) | **`StructureStubInfo.access_cases`/`InlineCacheStub.cases` (already resident) become the sole source of truth.** The hard `.expect()` in `build_property_inline_cache_clear_request` (`tiering.rs:6198-6205`: `.find(\|r\| r.ordinal == request.attachment_ordinal).expect(...)`) is replaced by `code_block.side_tables().inline_caches().structure_stubs[structure_stub_index]` — an **infallible Vec index**, exactly how every other `structure_stubs` consumer in `code_block.rs` already reads it (confirmed: `structure_stubs[structure_stub_index]` pattern at `code_block.rs:1747,1804,3391,6786,...`). This turns a fallible O(n) ordinal scan into an infallible O(1) index — a strict simplification, not just a rename. |
| `property_inline_cache_clear_records: Vec<VmPropertyInlineCacheClearRecord>` (`tiering.rs:12252`; already capped to 1024 in `58325a3` as a stopgap) | The retain-limit cap was a correct STOPGAP given the epoch-key consumer at the time; once the attachment side is resident, re-evaluate whether the one remaining production reader (an epoch key needing `len()`/last-ordinal, per `58325a3`'s commit message) can read a resident monotonic `clear_generation: u64` counter on `StructureStubInfo` instead — same shape as the epoch counters below. If so, delete the vec entirely in the same batch that removes the cap; if a genuine bounded-history need survives, keep the capped vec as explicitly-scoped historical diagnostics, not attempt provenance. |
| `property_load_access_case_plans` / `property_store_access_case_plans` / `property_store_access_case_install_rechecks` / `property_load_guard_plans` / `property_load_guard_dependencies` / `property_load_guard_install_rechecks` (`tiering.rs:231-236`) | Same two-tick-split pattern as the call-link pipeline's plan/install-recheck split — collapse plan-then-recheck into one atomic `attempt_property_ic(&mut StructureStubInfo, ...)` call per Unit R3's scope (see below), for the same reason: nothing survives the C++ equivalent (`Repatch.cpp`'s `tryCacheGetBy`/`tryCachePutBy`-style functions run start-to-finish under one lock hold). |

`property_megamorphic_cache_epoch` / `property_megamorphic_projection_generation`
(`tiering.rs:253,258`) are **out of scope** — already documented in-code as "the cache
invalidation epoch for [Rust-only] projections; not JS-visible state," a legitimate,
already-faithful-enough generation counter, not part of this cluster.

### C. Materialization/readiness split (`baseline_executable_materializations`, `baseline_native_entry_readiness_records`)

C++ never logs "is this executable's code ready" — it is a plain pointer/field on the
owning object, checked by identity (e.g. `CallLinkInfo`'s own `m_codeBlock`/
`m_monomorphicCallDestination` being null vs. set, `CallLinkInfo.h:310-311`; the same
shape recurs everywhere JSC tracks "has this been compiled yet"). The Rust side already
has the RIGHT precedent landed and load-bearing: **Unit 1** (`6170b4c`, "vm: entry/
baseline telemetry logs → per-owner Option slots") did exactly this fold for the
sibling `entry_decisions` log, citing C++'s per-`CodeBlock` `ExecutionCounter` scalars
(`ExecutionCounter.h:56-101`, `CodeBlock.h:995-997`) as the faithful shape — never event
logs. It replaced the log with `RuntimeTierState::last_entry_decision:
Option<TierEntryDecisionRecord>` (`tiering.rs:7064-7067`), a field on an **already-
existing owner-keyed state struct** (`state_for(owner) -> Option<&RuntimeTierState>`),
plus a VM-wide `entry_decision_count: u64` cumulative counter (`tiering.rs:186`) for the
"how many, ever" diagnostic. `RuntimeTierState` is the correct host for this unit's new
fields too — no new owner-keyed container needs inventing.

**But `6170b4c` explicitly tried this same fold on `baseline_executable_materializations`/
`baseline_native_entry_readiness_records` and reverted it**, with SCOPE NOTE comments at
the exact two call sites (`tiering.rs:2207-2219` and `:2350-2363`, read in full) giving
the precise reason each field resists a plain latest-wins `Option<Record>`:

1. **`baseline_executable_materializations` — value-equality identity check, not just
   latest state.** `validate_baseline_executable_materialization_for_install`
   (`tiering.rs:2225-2243`) does `.any(|record| record == materialization)` — the caller
   passes back a **specific record by value** and the VM must confirm it still owns
   *that exact attempt* (not just "the current one"), because this is, per the code
   comment, "a stand-in for identity/pointer validation C++ gets for free" (a real
   `CodeBlock*`/materialized-artifact pointer needs no such check in C++ — pointer
   identity IS the check). The test `baseline_materialization_rejects_descriptor_
   mismatches` deliberately creates several REJECTED materializations for the SAME owner
   and later re-validates an EARLIER, non-latest one — a plain `Option<Record>` would
   evict it, turning a real rejection into a spurious "not VM owned" error.
   **Faithful fix**: give each materialization attempt genuine Rust identity instead of
   value-equality-into-a-log — e.g. a small per-owner bounded ring (`ArrayVec`-shaped,
   sized to the realistic in-flight-attempt count, likely ≤ 4-8) on `RuntimeTierState`,
   so a rejected attempt stays checkable by ordinal/identity within its owner's own
   bounded window without being VM-global-unbounded. This is different from `26d48a6`'s
   plain latest-wins fold precisely because the consumer's contract requires "this
   specific past attempt," not "the current one" — the same MIXED-consumer exception
   `26d48a6` itself hit and solved with an immutable snapshot-on-the-record, generalized
   here to a bounded ring instead of one extra field.
2. **`baseline_native_entry_readiness_records` — the P6 two-records-per-call pattern.**
   `install_p6_x86_64_semantic_baseline_native_entry` (`vm/mod.rs`) legitimately records
   **two** readiness records for the SAME owner in one call — first a disabled probe,
   then an enabled one — and isolates the fresh pair via a length-based "mark"
   (`.get(readiness_count_before..)`, i.e. snapshot the Vec's length before the call,
   slice from there after). `p6_semantic_install_side_effect_counts` separately reads
   `.len()` as a true cumulative "how many readiness records ever" counter. **Faithful
   fix**: since the shape is always exactly two fixed roles (disabled-probe result,
   enabled-probe result), model it as **two named `Option<Record>` fields** on
   `RuntimeTierState` (e.g. `last_disabled_readiness`, `last_enabled_readiness`) rather
   than a sliceable Vec — the install function already computes both results in one call
   and can return them as a small struct/tuple directly to its caller instead of writing-
   then-slicing a shared log. The "mark" pattern disappears because there is no shared
   Vec to mark a position in. The cumulative counter becomes a plain `u64` bumped once
   per readiness record produced (2 per P6 install call), same shape as `entry_decision_
   count`.

**Cross-reference, not a reason to skip this unit:** P6
(`BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry`) is itself a
CONFIRMED, already-documented divergence scheduled for deletion
(`docs/design/baseline-call-tier-divergence.md`, "STEP 5 — delete the now-dead divergence
cluster... the x86_64-on-arm64 entry"). Unit R4 should therefore keep the new fields
generic (owner-keyed state + cumulative counters, as above) rather than over-fitting to
P6's exact two-probe shape — when STEP 5 deletes the P6 path, the readiness fields
should degrade gracefully to "one slot instead of two," not need a second redesign.

## Ordinal fields: deleted vs. site-scoped generation counters

Overwhelming majority: **deleted**. Cross-record ordinal joins exist only because two-or-
more Vecs needed to refer to each other; once there is exactly one resident record, there
is nothing to join. Two categories legitimately survive as small resident counters, not
logs:

1. **Existing JSC-faithful counters** — `slow_path_count` (`CallLinkInfo`), the new
   `countdown`/`repatch_count`/`number_of_cool_downs`/`buffering_countdown`
   (`StructureStubInfo`) — these are genuinely resident C++ fields, not ordinals; keep as
   specified above.
2. **Cross-tier staleness detection** — if (and only if) a consumer running under a
   *different* mutation authority (e.g. a concurrent DFG/FTL compiler thread reading a
   `ConcurrentJSLocker`-protected summary snapshot, mirroring how `PolymorphicAccess`
   itself is read concurrently in C++) genuinely needs "has this site changed since I
   last looked," it gets a **single monotonic `u64` generation counter on the site
   struct**, bumped on each committed mutation — the same shape `26d48a6` already landed
   as `VmPropertyInlineCacheEvolutionState::terminal_ordinal`. This is the one place a
   number resembling today's "ordinal" survives, and only where a real concurrent-reader
   consumer requires it (verify per-unit, do not add speculatively).

## Unit decomposition (commit-sized, dependency-ordered)

Each unit deletes its own log cluster in the SAME commit that adds the resident
replacement — no half-migrated state ships (CLAUDE.md commit discipline). Each unit runs
the full pipeline stage sequence (inspect → map → skeleton → port → test → verify) as its
own pipeline, not a shared barrier.

- **Unit R0 — resident skeleton (serial prerequisite, low risk).** Add the new fields to
  `StructureStubInfo` (§B) and confirm/extend `CallLinkInfo`'s field set for the atomic
  attempt function's needs (§A) — additive only, `#[allow(dead_code)]` until wired,
  `cargo check --lib` green with zero behavior change. Unblocks R1-R4 to proceed in
  parallel once landed. This is the Standard Workflow's "Skeleton" stage made explicit as
  its own commit, because it is genuinely shared by both pipelines.
- **Unit R1 — call-link pipeline atomic fold (was Unit 2c).** Collapse the 8 vecs in
  §A onto `CallLinkInfo`; write `attempt_call_link`/`clear_call_link` mirroring `linkFor`
  exactly; delete `call_observations`, `call_link_readiness_records`,
  `call_link_descriptor_records`, `call_link_boundary_validation_records`,
  `call_link_attachment_plan_records`, `call_link_attachment_install_rechecks`,
  `call_link_inline_cache_attachment_records`, `call_link_inline_cache_clear_records`;
  rewrite the ~15+ functions and dispose of the 33 tests using the SAME classification
  scheme `d0bd496`/`26d48a6` used (PURE-BEHAVIOR kept and rewritten against the new API;
  DELETABLE removed outright; MIXED split so the behavioral assertion survives and the
  log-mechanism assertion is dropped) — exact per-test disposition is Unit R1's own audit
  pass output, not invented here (see "Known gaps"). Depends on R0.
- **Unit R2 — property-IC buffering/countdown port.** Add
  `StructureStubInfo::consider_repatching` (§B); delete
  `structure_stub_access_case_links`; this is the unit that resolves the rejection-
  memoization question definitively and should land with a regression test mirroring
  `considerRepatchingCacheImpl`'s exact arithmetic (cooldown escalation, buffered-set
  dedup-then-clear-on-regenerate). Depends on R0. Independent of R1 (different struct).
- **Unit R3 — property-IC attachment/clear fold.** Fold
  `property_inline_cache_attachment_records`/`property_inline_cache_clear_records` and
  the plan/install-recheck pairs (`property_load_access_case_plans`,
  `property_store_access_case_plans`, `property_store_access_case_install_rechecks`,
  `property_load_guard_plans`, `property_load_guard_dependencies`,
  `property_load_guard_install_rechecks`) onto `StructureStubInfo.access_cases`/
  `InlineCacheStub.cases`; fix the hard `.expect()` in
  `build_property_inline_cache_clear_request`; rewrite the ~11+ consumers. Depends on R2
  (shares the site struct's new fields) — sequence R2 before R3.
- **Unit R4 — materialization/readiness split (`6170b4c`'s deferred remainder).** Add a
  bounded per-owner ring to `RuntimeTierState` for `baseline_executable_materializations`
  (identity-checkable rejected attempts survive within the ring) and two named
  `Option<Record>` fields for `baseline_native_entry_readiness_records`'s disabled/
  enabled pair (§C); add the matching cumulative `u64` counters; delete both unbounded
  Vecs once `validate_baseline_executable_materialization_for_install` and
  `p6_semantic_install_side_effect_counts` (`vm/mod.rs`) are rewritten against the new
  fields. Independent of R1-R3, can run in parallel with them once R0 lands.
- **Unit R5 — watchpoint dependents (separate initiative, cross-referenced only).** Not
  implemented by this document. Composes with R2/R3's new `StructureStubInfo` fields by
  adding its own `dependent_watchpoint_sets`-shaped field alongside them — no field-name
  or ordering collision, both additive to the same struct. Flagged here so whoever scopes
  Unit 6 reads this document's §B field list before choosing names.
- **Unit R6 — off-gate hygiene.** Once R1-R4 land, delete the now-dead enum variants that
  only existed to describe log-vec cross-reference errors (e.g.
  `VmCallLinkAttachmentPlanMismatchField`, `VmCallLinkInlineCacheAttachmentRejectionReason`'s
  `*OrdinalMissing`/`*RecordMissing` variants, `VmStructureStubMetadataMismatchField`) once
  nothing constructs them; a pure dead-code sweep, `cargo check --lib` with `#[warn(dead_code)]`
  promoted to confirm zero remaining references before deleting each variant.

**Sequencing:** R0 → {R1, R2 → R3, R4} in parallel → R6. R5 is a separate unit that reads
this document but is not gated by it (per the task brief: "design the fields so they
compose," not "implement Unit 6 here").

## Rollback story

Each unit is independently revertable — this is the same shape `d0bd496` already proved
works: that commit landed 90% of Unit 2b and used a documented, evidence-backed partial
revert (the dedup-key narrowing) for the one sub-piece a real test refuted, with a
SCOPE-NOTE at the code site rather than blocking the whole batch. This document's units
follow the same contract:

- R0 is purely additive; if anything downstream stalls, R0's new fields simply sit
  unused (`#[allow(dead_code)]`) with zero behavior risk — nothing to roll back.
- R1-R4 each delete their own log cluster in the same commit as their replacement, so
  `git revert <unit-commit>` cleanly restores the prior log-based behavior for that one
  cluster without touching the others (the clusters are structurally independent VM
  fields today, and stay independent as separate commits).
- If a unit's consumer rewrite proves incomplete under `cargo test --lib` (as R1's
  dedup-key narrowing attempt was in `d0bd496`), the fallback is the same
  evidence-backed-revert pattern: keep the sub-piece the tests refute, add a SCOPE-NOTE
  at the code site citing the specific failing test(s), and defer only that slice to a
  follow-up — never block the rest of the unit on one refuted sub-decision.
- R6 (dead-code sweep) is the safest possible unit — if a variant turns out to still be
  referenced, `cargo check --lib` fails the batch before it's ever committed.

## Open questions for the orchestrator (serial decisions)

1. **`buffered_structures` location.** Recommended: directly on `StructureStubInfo`,
   matching C++ 1:1 (`StructureStubInfo` derives `Clone, Debug, Eq, PartialEq`, not
   `Copy`, so a `Vec`-bearing field is not a layout regression). Alternative: a side
   small-vec keyed by `structure_stub_index` if some other invariant needs
   `StructureStubInfo` to stay fixed-size. No evidence found that it needs to; flagged
   for confirmation before Unit R0 lands.
2. **What "`Option<CallLinkAttachmentPlan>` lives on `CallLinkInfo`" means structurally.**
   Two readings: (a) literally add a `plan: Option<CallLinkAttachmentPlan>` field to
   `CallLinkInfo`, including its `stub: InlineCacheStub` payload; or (b) `CallLinkInfo`'s
   own existing fields (`target`, `mode`, `flags`) already ARE the plan's committed
   projection once the attempt function computes-and-consumes the plan on the stack,
   matching `linkFor`'s "no separate plan object ever exists" reality. This document
   recommends (b) as more faithful (JSC has no `CallLinkAttachmentPlan`-shaped object at
   all), but the `d0bd496` ratification's exact wording supports either reading — needs
   an explicit orchestrator call before Unit R1 starts, since it changes `CallLinkInfo`'s
   field list.
3. ~~Whether `AccessCaseRef` needs to become an owned payload~~ — **resolved during this
   document's research, not open**: `AccessCaseRef` already wraps an `InlineCacheStub`
   id (`tiering.rs:17913`), and `InlineCacheStub.cases: Vec<AccessCaseDescriptor>`
   (`jit/ic.rs:3661`) already holds the real payload, faithfully mirroring
   `PolymorphicAccess::m_list`. No new type needed; §B's fields are the only gap.
4. **Bounded ring size for `baseline_executable_materializations`'s replacement (Unit
   R4).** This document recommends a small per-owner ring on `RuntimeTierState` (§C) but
   does not derive the exact capacity from evidence — `baseline_materialization_rejects_
   descriptor_mismatches` (the test that forced the `6170b4c` revert) needs to be re-read
   to confirm how many simultaneous rejected attempts per owner it actually exercises;
   size the ring to that plus headroom, not a round number picked here. Whether the ring
   should instead live on the owning `CodeBlock`/executable record directly (matching
   C++'s `m_codeBlock`/`m_jitCode` pattern more literally than `RuntimeTierState`'s
   parallel side-table does) is a smaller version of the same open question — `RuntimeTierState`
   is the pragmatic choice (it already exists, already owner-keyed, already the `6170b4c`
   precedent) but is itself a Rust-only side-table relative to C++'s "the field lives on
   the object," worth flagging rather than silently accepting.
5. **The generated-call-link sidecar's polymorphic-candidate table** (§A): re-model as
   `CallLinkInfo`'s own bounded polymorphic list (this document's recommendation) versus
   keeping a small resident side-table scoped per `bytecode_index` (fewer call sites
   touched, still resident instead of log-shaped, but a second place call-site identity
   lives). Needs `linkPolymorphicCall`'s exact list-size cap cited from C++ before commit.
6. **`property_inline_cache_clear_records`'s epoch-key consumer** (§B table): confirm
   whether it can move to a resident `clear_generation: u64` counter or must keep a
   bounded capped log — the `58325a3` commit message names the consumer only as "an epoch
   key needing `len()`/last-ordinal"; re-derive its exact requirement before deciding.

## Known gaps in this draft (do not skip before implementing)

This document was authored from direct, firsthand inspection of both the C++ JSC source
(`CallLinkInfo.{h,cpp}`, `RepatchInlines.h`, `PropertyInlineCache.h`,
`InlineCacheCompiler.{h,cpp}` — all read in full for the cited ranges) and the Rust tree
(`src/bytecode/ic.rs`, `src/jit/ic.rs`, `src/vm/tiering.rs`, `src/bytecode/code_block.rs`
— struct definitions, consumer call sites, the exact hard `.expect()`, and the `6170b4c`
SCOPE NOTE comments all confirmed by direct reads, not inference). One piece of evidence
remains genuinely open:

- **Exact per-test disposition** for the 33 call-link tests (`d0bd496`'s own count) and
  the property-IC/materialization test sets (PURE-BEHAVIOR/DELETABLE/MIXED counts and
  names) — this document specifies the classification *scheme* (reused from `d0bd496`/
  `26d48a6`) and, for materialization, the one test known by name
  (`baseline_materialization_rejects_descriptor_mismatches`) that drives the ring-size
  decision, but not the full per-test list for any unit. Unit R1/R2/R3/R4 each need their
  own such pass immediately before implementation, the same way `d0bd496`'s pause report
  had one — this is expected, routine per-unit audit work, not a blocker on ratifying the
  design itself.

## Authority

C++: `bytecode/CallLinkInfo.{h,cpp}`, `bytecode/RepatchInlines.h` (`linkFor`, read in
full), `bytecode/PropertyInlineCache.h` (`considerRepatchingCacheImpl`,
`m_bufferedStructures`, read in full), `bytecode/InlineCacheCompiler.{h,cpp}`
(`PolymorphicAccess::addCases`, `AccessGenerationResult`, read in full). mcts_mem:
`javascriptcore/bytecode/inline-cache.md` (the settled Facts/Moves list — "codeblock-bag-
backed-inline-cache-metadata," 2021-10-30, is the direct precedent for CodeBlock owning
fixed IC vectors rather than a Bag, which is why `structure_stubs[index]`/`CallLinkInfo`
are already O(1)-reachable by identity today). Rust precedent: `6170b4c`
(Unit 1: entry-decision log → `RuntimeTierState::last_entry_decision` + cumulative
counter — the reusable owner-keyed-state host for Unit R4, and the source of the exact
evidence for why `baseline_executable_materializations`/
`baseline_native_entry_readiness_records` resisted the same fold), `d0bd496` (call-link
identity fold, ordinal deletion pattern), `26d48a6` (observations → latest-state-per-site,
the historical-snapshot-on-the-record exception), `58325a3` (the deferral this document
resolves). Prior design docs: `docs/design/baseline-property-ic.md` (SQ4 monomorphic
churn cap — the existing, narrower precedent for "resident cap instead of log," distinct
from and complementary to this document's polymorphic buffering fields),
`docs/design/baseline-call-tier-divergence.md` (the CallLinkInfo-collapse decision this
document's §A executes).
