- Wasm linear memory can be compiled in a signaling mode that reserves a stable 32-bit virtual range and turns out-of-bounds accesses into traps through the fault handler. (`MemoryMode`)
- Bounds-checking memory keeps explicit checks and also reserves stable maximum backing for shared growable memories.
- Generated code treats memory mode as part of code identity; tier-up status and callee groups are separated by memory mode.
- Memory backing is owned by buffer handles that can update all attached instance caches when a grow operation changes base, size, or mapped capacity.
- Shared-memory wait/notify interoperates with JavaScript Atomics through the shared waiter-list manager.
- Zero-sized memories have a non-null base pointer and generated caging paths use the non-null memory case.
- Multimemory keeps a specialized memory-0 fast path while nonzero memories load base and bounds from per-memory instance state.
- Memory64 is represented as an AddressType and forces 64-bit address/count arithmetic in allocation, access, grow/size, atomics, and bulk-memory helpers.

## Facts

- 2017-03-03 (20b7da21) statement: fast memories are cached, and the last allocated memory mode is used as an import heuristic to reduce OS TLB churn and recompiles caused by compiling for the wrong memory mode (sourced).
- 2017-04-13 (6ffeeac1) pitfall: WebAssembly memory offsets are unsigned 32-bit while B3 immediates are signed 32-bit, so offsets above INT32_MAX must be folded into the pointer before lowering to avoid about a 2GiB out-of-bounds window (code).
- 2018-02-01 (4e88586c) measurement: using zeroed virtual allocation for WebAssembly Memory was reported as a large WasmBench compile-time speedup on iOS because JSC stopped dirtying memory only to zero it (sourced).
- 2018-02-23 (31ce0827) rationale: caching memory pointer, size, and indexing mask on each Wasm instance lets generated entry/context-switch code load memory state directly from the instance (code).
- 2020-11-18 (80581efa) statement: shared bounds-checking memory reserves the maximum virtual range up front, maps only the active prefix, and grows by protecting more of the reserved range while compiled and LLInt code keep stable base and size values (code).
- 2020-11-18 (80581efa) pitfall: shared WebAssembly memory needs the access-fault handler even in bounds-checking mode and must accept faults from Wasm LLInt code as well as JIT callees because inactive reserved pages trap until grown (code).
- 2022-12-31 (06118192) rationale: for signaling memory and shared bounds-checking memory, JS-to-Wasm IC code embeds the memory base pointer and mapped capacity as immediates because those values will not change (code).
- 2023-01-02 (ef906728) statement: the non-null base for zero-sized Wasm memory is Gigacage::basePtr when primitive Gigacage is enabled; otherwise it is a page-sized aligned allocation that is immediately decommitted (code).
- 2026-02-09 (bdf26416) statement: multi-memory preserves a memory-0 fast path and cached memory-0 size/base fields to avoid regressing single-memory code while nonzero memories fetch base and bounds from per-memory cached pairs (code).
- 2026-02-27 (65324979) pitfall: Memory64 must not use signaling fast memory in BBQ; memory creation now requires non-64-bit AddressType before selecting Signaling mode and the BBQ signaling path asserts memory is not Memory64 (code).

## Moves

- 2017-03-03 (20b7da21) replaced [[wasm-explicit-bounds-check-memory]]: Fast memories reserve 2^32 plus offset virtual address space and rely on signal-handled trapping loads/stores so WebAssembly memory accesses can omit explicit bounds checks in Signaling mode. (code)
- 2017-04-13 (6ffeeac1) replaced [[eight-gib-webassembly-fast-memory-mapping]]: Fast-memory mappings shrank from 8GiB to 4GiB plus a configurable redzone; signaling mode relies on PROT_NONE for the 32-bit range and emits explicit WasmBoundsCheck only when register-plus-immediate accesses can exceed the redzone. (code)
- 2017-10-03 (26ecac57) replaced [[js-vm-owned-wasm-memory]]: Wasm::Memory stopped requiring VM/JS so non-JS embedders can supply their own memory-pressure, synchronous-reclamation, and growth-success behavior while keeping Memory as the generated-code source of truth. (sourced)
- 2020-11-18 (80581efa) replaced [[wasm-bounds-checking-memory-active-size-base]]: Shared WebAssembly.Memory can grow on one thread and become immediately accessible on other threads without updating their cached base pointer or bounds-checking size. (sourced)
- 2022-12-24 (aa3a6446) replaced [[wasm-atomics-parkinglot-wait-notify]]: Wasm atomics moved onto WaiterListManager so JS Atomics and Wasm atomics share the same waiter lists for interoperable wait/notify on shared memory. (sourced)
- 2023-01-02 (ef906728) replaced [[nullable-zero-sized-wasm-memory]]: Zero-sized Wasm memory now has a non-null base pointer so frequent generated caging paths can pass mayBeNull=false and skip null handling. (code)
- 2025-12-09 (597e0085) replaced [[wasm-memory-vm-local-instance-cache-update]]: Tracking instances in Wasm::Memory only updated instances remembered by the current thread, so growth of shared memory moved cache invalidation to the BufferMemoryHandle's ThreadSafeWeakHashSet of InstanceAnchor objects shared by all threads. (sourced)
- 2026-02-09 (bdf26416) replaced [[wasm-single-memory-module-instance]]: Add support for instantiating multiple memories in wasm, but not for executing instructions that use memories other than index 0. (sourced)
- 2026-02-27 (65324979) replaced [[wasm-memory64-boolean-addressing]]: Memory address width became an explicit AddressType carried into Memory allocation so i64 memories can be forced away from signaling fast memory at creation time. (code)
- 2026-05-04 (c3893af6) replaced [[wasm-bulk-memory-uint32-address-operations]]: Memory64 bulk-memory support needs uint64 addresses/counts in shared helpers and JIT operations, while memory32 callers must explicitly zero-extend their 32-bit operands before using those widened call interfaces. (code)
