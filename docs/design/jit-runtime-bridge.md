# Design — the JIT↔runtime bridge (how emitted code calls back into Rust)

Status: **ratified (2026-06-28)**; the serial ownership ruling for every baseline-JIT
slow path + the later IC/call tiers. Designed by a read-only audit; the orchestrator's
call, settled by evidence (below).

## The problem

A baseline-JIT slow path (e.g. op_add's type-guard failure) must call the faithful
runtime. In C++ it emits a C-ABI call to a free `operation*(JSGlobalObject*, EncodedJSValue,
EncodedJSValue)`; the operation reaches the VM via `globalObject->vm()` and runs `jsAdd`
(JITOperations.cpp:4860), setting `vm.m_exception` on throw. The Rust evaluators
(`arithmetic_binary_result` interpreter/mod.rs:9242, `numeric_binary_result` :20424,
`bitwise_binary_result` :20480, `compare_primitive_relational` :9309) are `&mut self`
METHODS on the `CoreOpcodeDispatchHost` (mod.rs:3204) that also take `&mut Heap` — not free
functions. JIT-emitted code has only a register. How does an `extern "C"` shim reach a
callable `&mut host`+VM soundly?

## The ruling (D1 — the load-bearing invariant)

**The Vm crosses the JIT boundary as a RAW `*mut Vm`.** During execution exactly one
`&mut Vm` is live (the driver's). To call JIT code the driver parks that `&mut Vm` as a
`*mut Vm` for the JIT-call region and does NOT touch the Vm until the call returns; the
slow-path shim reborrows exactly one `&mut *vm` for the operation's duration and drops it
before returning to JIT code. Sound under Stacked/Tree Borrows: the driver is BLOCKED inside
the call-into-JIT, so its `&mut Vm` is dormant (never read/written while JIT runs) and the
reborrow never aliases a live parent `&mut`. This is the canonical "pass `&mut` as a raw
pointer through a C callback and reborrow" pattern — the SAME expose-provenance/raw-pointer
discipline `jsstack.rs` already uses for the Register arena. **Unsafe is confined to the
entry trampoline + the per-op shim** (one audited `#![allow(unsafe_code)]` island; the rest
of `jit/` stays `deny`). **Miri-verified** on the live reborrow.

## The reach mechanism (D2 — (c) now → (b) later)

The Vm owner already exists (vm/mod.rs: `heap` :898 + `exceptions` :905 + the
`CoreOpcodeDispatchHost` :1166), reached today by `&mut` threaded through the dispatch loop
(no thread-local, no current-VM global). The baseline ABI already RESERVES a VM-pointer
register: `BASELINE_PINNED_VM_REGISTER {role: PinnedVm}` (jit/abi.rs:573, required :357).

- **(c) NOW — pass `*mut Vm` from the PinnedVm register.** The entry trampoline materializes
  `*mut Vm` into the PinnedVm register; the slow path passes IT as the shim's arg0. No
  object-model change. Tiny documented divergence: this is JSC's `globalObject->vm()`
  PRE-RESOLVED (one deref elided). Smallest sound delta; uses the already-reserved register.
- **(b) FAITHFUL END-STATE — `globalObject->vm()`.** Pass `globalObject` in x0 (the faithful
  operation ABI) + a `*mut Vm` back-pointer on the global-object cell (mirror
  JSGlobalObject::m_vm); the shim does `(*global_object).vm()`. Deferred: the global-object
  cell (realm.rs `GlobalObject` :27 carries only IDs, no back-pointer) gains `m_vm` when the
  object model stabilizes (post-GC-rewrite) — ICs/builtins need `globalObject->vm()` anyway.

Convergence cost: the shim signatures change from `(*mut Vm, …)` to `(*GlobalObject, …)` — a
mechanical migration. Marked transitional; correct toward (b) when the global-object cell is
ready. (a) thread-local current-VM is REJECTED (least aligned with JSC's explicit-arg ABI).

## The shim shape (D3 exception word, D4 split-borrow wrappers)

`extern "C" fn operation_value_add(vm: *mut Vm, op1: u64, op2: u64) -> u64` (mirrors
operationValueAdd `EncodedJSValue(JSGlobalObject*,EncodedJSValue,EncodedJSValue)`,
JITOperations.h:419):
1. `let vm = unsafe { &mut *vm };` (D1 reborrow).
2. decode `JsValue::from_encoded(EncodedJsValue(op1))` → RuntimeValue (repr.rs:561/460; the
   int32 fast path already boxed faithfully NumberTag|u32).
3. call a SAFE wrapper `Vm::operation_value_add(op1,op2)` that split-borrows the disjoint
   `host`+`heap` fields and runs the EXISTING `arithmetic_binary_result` verbatim (D4 — the
   evaluators are UNTOUCHED; the disjoint-field borrow is safe Rust).
4. Ok → `result.encoded().0` (repr.rs:565).
5. Err(ExecutionError) → write the **fixed exception word** (D3) + return JSValue::empty bits.

**D3 — the exception word:** add a single fixed-offset `pending: EncodedJsValue` (0 = none)
to `ExceptionState`/`Vm` as the `VM::m_exception` analog. The shim sets it on Err; the JIT
fast path, after the call, emits `branchTestPtr(NonZero, AbsoluteAddress(&vm.exceptions.pending))`
→ an exception stub (first cut: bail to interpreter/unwind). The interpreter's existing
Result→ExceptionState path stays (a pre-existing divergence from C++'s single m_exception
word; converge later). Before the call, stamp topCallFrame + CallSiteIndex so a GC/throw
during the op attributes correctly.

## D5 — host reach (LANDED, ratified 2026-06-28)

D4 wrongly assumed the Vm owns the host. It doesn't: `CoreOpcodeDispatchHost` (the object/
string/bigint/symbol stores + the evaluators) is a SEPARATE allocation the driver threads in.
So the Vm carries `jit_host: *mut CoreOpcodeDispatchHost` (vm/mod.rs), a RAW pointer the driver
parks via `set_jit_host(&mut host)` immediately before the JIT-call region and `clear_jit_host`
after — same D1 discipline. The shim reborrows BOTH `&mut *vm` and `&mut *vm.jit_host` (DISJOINT
allocations — Vm owns only the raw ptr, not the host — so the two `&mut` never alias);
`Vm::operation_value_add` split-borrows the Vm's disjoint fields + the real host and runs the
evaluator verbatim. Miri-passed (`-Zmiri-tree-borrows`) on the real-host test (`"ab"+"cd"` lands
in the REAL string store — definitive proof the parked host, not a transient, is reached).
Faithful-enough bridge; the end-state unifies the host INTO the Vm (a later interpreter refactor)
and replaces this raw ptr with direct ownership.

## Build order

1. **Bridge-infra** — **DONE** (verified ACCEPTABLE): D4 wrappers + `jit/operations.rs` shim
   (D1+D5 reborrows, the one audited unsafe island) + the D3 `jit_pending` word + the
   MacroAssembler far-call + the direct real-host test + Miri.
2. **op_add lowering** (NEXT): the entry trampoline (materialize the PinnedVm `*mut Vm` +
   `set_jit_host` the active host + x29; `clear_jit_host` after) + the fast+slow op_add in
   arm64_baseline (box/tag int32 guards + branchAdd32 → slow → `far_call(operation_value_add)` +
   `branchTestPtr(NonZero, jit_pending_address())`) + emit→relocate→execute test. Then template
   across sub/mul/bit/shift + branch ops + live dispatch.

   **Hard forward prerequisites the op_add unit MUST satisfy** (from the bridge verify — not
   bridge defects, but required before real JS flows through the slow path):
   - **Thread the real active-frame CodeBlock** before OBJECT operands reach the slow path
     (`{} + 1` → ToNumber needs the live CodeBlock; bridge-infra proved the PRIMITIVE AddInt32
     path never reads it, so the placeholder is sound only for primitives).
   - **Materialize a real TypeError** for an engine `Fail(ExecutionError)` (replace the 0x8
     non-empty sentinel the shim currently writes; a real `Throw(value)` is already faithful).
   - **Bake `jit_pending_address()` against a pin-stable Vm** (the address must not move after
     the JIT bakes it as an AbsoluteAddress).
   - **The trampoline upholds the D1 park-and-don't-touch discipline** (the driver must not read/
     write the parked `&mut Vm`/`&mut host` while JIT code runs).
