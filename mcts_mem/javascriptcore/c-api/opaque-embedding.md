- API-created globals are JSC global objects and participate in VM locking, GC protection, run-loop timers, watchdogs, remote-inspection defaults, and context-group lifetime through the same `VM` object space.
- Host API calls hold the VM API lock before touching engine state, and callbacks temporarily drop that lock while running embedder code.
- `JSGlobalContextRetain` protects the global object and refs the VM; `JSGlobalContextRelease` unprotects the global, reports abandoned graphs when the last protection drops, and derefs the VM.
- C API weak references and weak-object maps are VM/context-group scoped: they expose non-retaining references to API clients without making weak dereference an execution-capable context operation.
- API argument lists that may outlive immediate stack slots are represented as GC-visible marked containers rather than unrooted raw `JSValueRef` arrays.
- API boundary diagnostics distinguish client-owned lifetime failures from engine write-barrier and root-marking bugs.

## Facts

- 2020-03-15 (003d0374) pitfall: variable-length `JSValueRef` argument arrays are both stack-exhaustion hazards and invisible to GC when spilled API values need rooting. (code)
- 2020-04-19 (2ea571e8) pitfall: public API calls that propagate an exception must not convert or return an unchecked nominal result afterward. (sourced)
- 2024-10-28 (596c3527) pitfall: constructors that install weak handles must retain the VM while locking it, because weak-handle allocation is API-lock-owned and the VM may otherwise be destroyed while a client recovers a weak value. (code)
- 2026-04-14 (3572482f) statement: dangling-reference diagnostics classify unprotected `JSValueRef` storage, destroyed `JSContext` use, and cross-`JSContextGroup` use as API-client ownership bugs distinct from missing engine write barriers. (code)

## Moves

- 2008-07-23 (26073881) replaced [[opaquejsclass-context-data-on-class]]: per-context `JSClassRef` data moved out of the context-free class descriptor because one `JSClassRef` can be used in multiple contexts and context groups; context-specific static-entry strings and prototype caches must be looked up through the current global object. (sourced)
- 2013-03-22 (c1225a67) replaced [[opaque-jsclass-data-per-globaldata]]: Sharing JSClassRef prototype cache across all JSGlobalObjects in a JSGlobalData caused the first GlobalObject that created the prototype to be retained indefinitely; moving the cache into JSGlobalObjectRareData scopes it to the owning context and fixes the leak. (sourced)
- 2014-04-15 (b217437c) replaced [[objc-external-object-graph-no-remembered-set]]: Objective-C external-object graph marking gained an external remembered set because Eden collection must revisit old native owners that have acquired young managed references. (sourced)
- 2017-05-11 (84a0d815) replaced [[jsweak-api-context-parameter]]: The JSWeak create/retain/release operations only need VM access and do not execute arbitrary JavaScript, so their API boundary should require a JSContextGroupRef rather than a JSContextRef. (sourced)
- 2020-03-15 (003d0374) replaced [[stack-vla-jsvalue-ref-argument-arrays]]: Variable-length JSValueRef argument arrays let user-controlled argument counts consume C++ stack space and do not give the GC an explicit root list for spilled API values, so API argument storage moved to a stack-only object with inline capacity, caged heap spillover, and heap marking registration. (code)
- 2026-03-30 (4bd1aab3) replaced [[marked-jsvalue-ref-array-root-list]]: A dedicated JSValueRef array could only store API values as JSValueRef-sized entries, while MarkedVector needed to be a Vector-like root container for JSValue, JSCell-derived pointers, and JSC API reference types including 32-bit pointer-sized storage. (code)
