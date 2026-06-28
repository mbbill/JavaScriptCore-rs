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

## Build order

1. **Bridge-infra** (this design): the `Vm::operation_*` wrappers (D4) + `jit/operations.rs`
   shims (D1) + the `pending` exception word (D3) + a MacroAssembler far-call (move_imm64+blr)
   + a DIRECT test (call `operation_value_add(vm_ptr, a, b)` simulating the JIT call) + Miri on
   the reborrow. (vm/mod.rs + jit/ + ExceptionState — disjoint from the interpreter op work.)
2. **op_add lowering**: the entry trampoline (materialize PinnedVm + x29) + the fast+slow
   op_add in arm64_baseline (box/tag guards + branchAdd32 → slow → the far-call) + emit→
   relocate→execute test. Then template across sub/mul/bit/shift + branch ops + live dispatch.
