- `JSVirtualMachine` wraps a `JSContextGroupRef` and records external Objective-C ownership edges for the garbage collector.
- `JSContext` owns one `JSGlobalContextRef`, retains its `JSVirtualMachine`, and stores exception state as Objective-C wrapper state rather than as a context-global C API slot.
- `JSValue` retains its `JSContext` and protects its underlying `JSValueRef`.
- `JSWrapperMap` belongs to the `JSGlobalObject` and maps between Objective-C objects/protocol-exported classes and JS wrappers, synthesizing JS constructors, prototypes, methods, and accessors from `JSExport` protocol metadata.
- `JSManagedValue` stores a weak cross-heap value reference plus owner bookkeeping; owners report edges to `JSVirtualMachine`, and the value is reconstructed only if its weak global and weak value are still live.
- Objective-C block and method callbacks are represented as JS callback functions/cells, not generic host objects.

## Facts

- 2013-01-11 (5837e573) pitfall: `JSValue` originally held a weak `JSContext` and `JSWrapperMap` used associated objects to cache wrappers, so contexts could die under live values and wrappers could keep Objective-C objects alive until context destruction; the fix made values retain contexts and moved wrapper caching into a weak GC map. (sourced)
- 2013-01-30 (628729ea) pitfall: storing prototypes and constructors as strong `JSValue*` objects in exported class info retained `JSContext` through `JSValue`, forming a cycle; weak JS object handles break the Objective-C retain cycle. (sourced)
- 2013-03-07 (e5633bdc) rationale: `JSManagedValue` is strong only when its owner is alive and reachable as an opaque root in the GC graph, avoiding event-handler/value cycles while preventing dangling cross-heap references. (sourced)
- 2014-04-15 (b217437c) pitfall: generational collection must rescan old Objective-C owners that gain young external references or a newly allocated `JSManagedValue` reachable only through the native graph can be collected. (sourced)
- 2015-10-16 (7dd08ecd) statement: `JSManagedValue` is the supported way to store a `JSValue` in Objective-C or Swift objects exported to JavaScript; storing `JSValue` directly creates a retain cycle, so reachability must be reported through `addManagedReference:withOwner:`. (sourced)
- 2018-03-28 (2a9fb646) rationale: once Objective-C and GLib both needed the same weak-value union, storage moved into shared `JSC::JSWeakValue` while each API binding supplies its own weak-handle owner. (code)
- 2024-10-28 (596c3527) pitfall: `JSManagedValue::value` must retain and lock the originating VM, tolerate a destroyed VM, and enter the API boundary before converting the weak value back to `JSValueRef`. (code)
- 2024-11-10 (74fba0c2) pitfall: `JSContext` stores its exception as `RetainPtr<JSValue>` instead of `Strong<JSObject>` because field initialization can run before the context has initialized the VM. (sourced)

## Moves

- 2013-01-17 (50834c77) replaced [[jscontext-value-protection-via-context]]: JSContext's m_protectCounts/protect/unprotect mechanism existed so the context could unprotect values before going away; once JSValue retains the context (keeping context alive as long as any value lives), the context-side lifecycle management is dead code and JSValue can protect/unprotect itself directly in init/dealloc. (sourced)
- 2013-01-31 (6db3421f) replaced [[objc-jscontext-exception-as-jsvalue-property]]: Storing the exception as a retained `JSValue*` ObjC property created a strong reference cycle because JSValue holds a strong reference back to its owning JSContext; replacing with a JSC::Strong<JSC::JSObject> GC handle breaks the cycle since it does not participate in ObjC ARC/retain. (sourced)
- 2013-03-14 (df22c6d6) replaced [[objc-callback-function-c-api-object]]: Implementing ObjCCallbackFunction as a JSClassRef C-API object gave it typeof 'object' instead of 'function', and Function.prototype.toString failed; subclassing JSCallbackFunction (a JSCell) gives the correct JS type and prototype chain membership. (sourced)
- 2013-03-22 (8cd345a1) replaced [[jsmanagedvalue-embedded-owner]]: The owner-embedded JSManagedValue API duplicated ownership tracking that JSVirtualMachine already provides; consolidating into JSVirtualMachine addManagedReference:withOwner: is the single authoritative mechanism for keeping managed references alive. (sourced)
- 2013-10-08 (b67599d6) replaced [[jsmanagedvalue-object-only-weak-reference]]: Weak<JSObject> could not represent non-object JSValues, while WeakValueRef can carry primitives, strings, or objects and a weak global object for reconstruction. (code)
- 2014-02-07 (1c4d8d7f) replaced [[owner-managed-jsmanagedvalue-unregistration]]: The JSManagedValue now records its owners and unregisters itself on dealloc, so owners no longer need bespoke dealloc code to balance addManagedReference:withOwner: calls. (code)
- 2017-05-22 (7b06ba41) replaced [[objc-wrapper-map-owned-by-jscontext]]: Objective-C wrapper caches need to survive JSContext wrapper object deallocation for the same JSGlobalContextRef, so ownership moved from JSContext to JSGlobalObject and per-call JSContext is passed explicitly. (code)
- 2018-05-30 (645b08bf) replaced [[synchronous-vm-shrink-footprint-spi]]: deleteAllCode frees less memory while JavaScript is on the stack because it is implemented to do work only when the VM is idle. (sourced)
- 2018-06-13 (284ea734) dropped: synchronous JSVirtualMachine shrinkFootprint SPI — The synchronous shrinkFootprint SPI was removed after clients moved to shrinkFootprintWhenIdle, leaving only the idle/asynchronous VM footprint-shrink entry point. (sourced)
