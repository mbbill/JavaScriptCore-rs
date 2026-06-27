- WTF threading represents platform thread identity, main-thread identity, stack bounds, run-loop timers, and dispatch queues through portable abstractions instead of raw pthread or toolkit objects in JSC code.
- Main-thread identity is initialized independently from generic threading, letting WebKit ports choose process-main-thread or current-thread semantics.
- Main-thread dispatch is bounded and scheduled through platform event-loop hooks to avoid starving UI work.
- Parallel helper work scales from processor count when available rather than a fixed compile-time worker limit.
- VM trap signaling uses a single managed signal-sender thread rather than allocating a sender per trap fire.

## Facts

- 2009-02-12 (6ae8e0c4) pitfall: draining all main-thread dispatch work in one pass let worker floods freeze the UI; time-bounded draining reschedules so input can run between batches. (sourced)
- 2017-06-29 (b8d026a2) pitfall: allowing many VMTrap signal sender threads created data races; one AutomaticThread sender is deallocated when traps are idle. (sourced)

## Moves

- 2009-02-12 (6ae8e0c4) replaced [[main-thread-dispatch-drain-all]]: Draining the entire queue in one shot caused UI freezes when workers flooded the queue; the new algorithm dispatches one item at a time, checks elapsed time against a 50ms threshold, and reschedules if exceeded so user input can be processed between batches. (sourced)
- 2009-05-11 (feae6bb7) replaced [[wtf-thread-identifier-integer]]: The old uint32_t ThreadIdentifier could not hold a native pthread_t (a pointer on 64-bit) or a Windows HANDLE without an indirection through a per-platform ThreadMap of integer-to-native-id; replacing it with a class wrapping PlatformThreadIdentifier eliminates the ThreadMap entirely and allows direct use of native thread ids. (sourced)
- 2010-04-26 (e12b0e8c) replaced [[main-thread-init-coupled-to-threading-init]]: initializeMainThread was previously called inside initializeThreading on all platforms, conflating two distinct concepts; decoupling them and adding initializeMainThreadToProcessMainThread (Mac-only) allows WebKit2 and WebKit1 to both use the same WebCore with different main-thread identity semantics (either the calling thread or the process main thread). (sourced)
- 2010-12-21 (f75765bf) replaced [[stack-bounds-estimated-bound]]: The estimated stack bound (origin minus fixed 128*sizeof(void*)*1024) was replaced with accurate OS-queried stack bounds on Darwin (pthread_get_stacksize_np), Windows (TIB StackLimit), QNX, and generic Unix (pthread_attr_getstack), increasing the size of expressions that can be processed; SOLARIS/OPENBSD/SYMBIAN/HAIKU/WINCE still use the estimate. (sourced)
- 2011-09-10 (f43cbd1b) dropped: single-threaded no-op threading — ENABLE(SINGLE_THREADED) was always false after 9096; ThreadingNone.cpp provided no-op threading stubs for targets without threading support, but those targets no longer existed. (sourced)
- 2011-10-18 (a8bd6a8c) replaced [[fixed-parallel-jobs-thread-limit]]: The fixed two-thread cap was replaced by a lazily computed processor-count cap, with 2 retained only as the fallback for platforms where processor count is unavailable. (code)
- 2017-04-11 (3a455e09) replaced [[glib-gsource-js-run-loop-timer]]: GTK JSRunLoopTimer moved to WTF::RunLoop::Timer while only Cocoa kept the platform-specific timer because Cocoa needs to retarget timers to the WebThread run loop. (sourced)
- 2017-06-29 (b8d026a2) replaced [[per-fire-vmtrap-signal-senders]]: A single AutomaticThread signal sender avoids the data races caused by allowing many VMTrap signal sender threads to exist at once and deallocates itself when traps are idle. (sourced)
