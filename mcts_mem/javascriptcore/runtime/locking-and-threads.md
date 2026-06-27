- Runtime entry uses a coarse JS lock/API-entry discipline for serialized VM access in legacy shared-state and embedder-API compatibility paths.
- Independent context groups use separate VM/global-data instances, allowing unrelated contexts to execute without sharing one process-wide heap and lock.
- The current VM, lock owner, thread stack bounds, and trap target are represented with explicit VM or platform-thread state rather than inferred from unrelated subsystem globals.
- Runtime synchronization uses WTF or platform-thread abstractions across pthread, Windows, and port-specific threading backends.

## Facts

- 2012-09-16 (041a7b02) pitfall: raw pthread-guarded global JS lock code compiled to no-op stubs on non-pthread platforms, causing real synchronization failures on Windows; WTF::Mutex made the lock portable. (sourced)
- 2017-07-19 (6ca93a35) rationale: stack bounds moved into WTF Thread state because JSC stack walkers need non-TLS access to another thread's bounds. (sourced)
- 2019-10-18 (c846cd8d) pitfall: ConcurrentJSLock must not be compiled away when DFG JIT is off because concurrent GC and baseline-produced structures still need synchronization. (sourced)

## Moves

- 2002-12-21 replaced [[per-subsystem-fine-grained-locking]]: Per-operation locks on the GC (collectorLock) and parser (parserLock) were too fine-grained and caused excessive contention; replacing both with a single PTHREAD_MUTEX_RECURSIVE interpreter-level lock eliminated the overhead. (sourced)
- 2008-07-14 (93836480) removed: JSGlobalData per-thread singleton — Per-thread JSGlobalData prevented arbitrary global-data/global-object pairings and forced thread registration through JSLock rather than through the heap; eliminating the thread-instance singleton allows any heap to be used from multiple threads and any JSGlobalObject to be associated with any JSGlobalData. (sourced)
- 2008-07-30 (aebe5fac) replaced [[jsglobaldata-shared-instance]]: The single shared JSGlobalData instance made independent concurrent execution of separate JSGlobalContexts impossible because all contexts shared one heap and required JSLock for every operation; replacing it with per-group JSGlobalData (each JSGlobalContextCreate gets its own group by default) removes the implicit locking requirement and allows truly independent contexts. (sourced)
- 2008-08-20 (98042fa9) replaced [[jsglobaldata-per-context-only]]: A shared JSGlobalData singleton with implicit JSLock was removed in a prior commit but reinstated because too many existing API clients relied on the single-shared-instance and implicit-locking semantics, making backward compatibility the deciding constraint. (sourced)
- 2012-09-16 (041a7b02) replaced [[global-js-lock-pthread-mutex]]: Using a raw pthread_mutex_t (guarded by OS(DARWIN)||USE(PTHREADS)) left non-pthread platforms with no-op stubs causing real synchronization failures on Windows; WTF::Mutex abstracts over platform threading primitives, enabling correct locking on all ports. (sourced)
- 2014-03-04 (3c18fd59) replaced [[separate-api-entry-shims]]: JSLock is now taking on all of APIEntryShim's responsibilities since there is never a reason to take just the JSLock. (sourced)
- 2017-03-01 (119091b3) replaced [[std-thread-id-jslock-owner]]: PlatformThread was chosen because std::thread::id cannot find the corresponding MachineThreads::Thread, suspend or resume threads, or signal a thread for non-polling VM traps. (sourced)
- 2018-08-07 (dfef653c) dropped: JSLock speculation fence — The speculationFence (x86 LFENCE) in JSLock::didAcquireLock and willReleaseLock crashed on processors without SSE2 and was not WebKit's actual Spectre mitigation strategy, so it was removed. (sourced)
- 2024-10-12 (1946d261) dropped: remote debuggable runloop marshalling — Dispatching JSGlobalObjectDebuggable work back to the creation runloop and waiting on a BinarySemaphore could hang if that runloop stopped processing tasks, so remote debuggable operations returned to direct JSGlobalObject access under JSLock. (code)
