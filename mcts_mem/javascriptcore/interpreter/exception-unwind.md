- JavaScript abrupt completion and exceptions are represented in VM/interpreter state, not by propagating C++ exceptions across JS frames.
- Throws record the thrown value and associated metadata in VM exception state; catch scopes and exception scopes make check/clear/throw discipline explicit.
- Unwinding uses StackVisitor plus bytecode/JIT/Wasm handler metadata to find catch targets, restore callee saves, notify debugger/profiler hooks, and redirect execution to catch machinery.
- Termination exceptions use the same propagation machinery but are treated as uncatchable internal control flow with explicit deferral and preservation rules.

## Facts

- 2008-07-19 (a1904bb5) rationale: toObject-failure exceptions use a sentinel error stub so expression range information can be added when the error propagates through Machine::throwException. (code)
- 2008-10-06 (baa0ad2d) pitfall: after ExecState became a typed Register*, dynamicGlobalObject lookup made exception unwinding O(N^2) in recursive depth until JSGlobalData cached it with RAII scope state. (sourced)
- 2009-01-06 (7cce4104) pitfall: using the ScopeChain RAII wrapper during exception unwind double-dereferenced the top ScopeChainNode; unwind now manipulates the raw node pointer. (code)
- 2011-02-16 (a114355a) pitfall: uncaught-exception reporting cannot use a torn-down call frame and must report through the saved global object's globalExec. (sourced)
- 2011-05-20 (9f520769) pitfall: interpreter returnVPC points after a call while JIT records the call start, so handler lookup must subtract one bytecode slot only to land inside the throwing call instruction. (code)
- 2013-07-25 (9f83fa13) rationale: JIT slow paths return and then check for exceptions because shared slow-path calling convention does not let the slow path jump directly to JIT exception dispatch. (code)
- 2014-02-13 (b07591a4) rationale: FTL stack-overflow handling unrolls to the caller before handler lookup because stack-overflow exceptions belong to the caller frame. (sourced)
- 2014-07-18 (b8e45e53) pitfall: activation and arguments tear-off during exception unwind must handle frames before op_enter has initialized registers or after activation has already torn off. (code)
- 2014-08-16 (fc9787da) rationale: arbitrary stack unwinding uses StackVisitor or threads a VMEntryFrame pointer through callerFrame because VMEntryRecords form a singly linked list. (sourced)
- 2014-08-22 (52f9008d) pitfall: exception unwinding must carry VMEntryFrame as well as CallFrame because stack-overflow setup and catch/rethrow paths update topVMEntryFrame. (code)
- 2015-09-05 (57a78eb6) pitfall: stack-overflow unwinding during native reentry must stop at the top VM entry frame so native stack frames and their cleanup are not skipped. (code)
- 2015-09-10 (16f8901b) rationale: a VM-wide callee-save buffer lets unwind translate saved registers across tiers instead of assuming all tiers save the same registers in the same locations. (code)
- 2015-09-17 (96333e76) pitfall: once in-frame handlers can bypass genericUnwind, debugger throw notification has to happen at VM::throwException rather than during unwinding. (code)
- 2016-08-30 (63095c3f) rationale: simulated throws are set in ThrowScope destruction rather than at throwException because normal throw paths return immediately and would otherwise need an explicit clear. (code)
- 2016-09-26 (4d7a208e) pitfall: exception-unwinding code that only inspects and routes an existing exception must use a CatchScope, not a ThrowScope. (sourced)
- 2018-10-11 (14fef92c) rationale: ExceptionScope records currentStackPointer at construction because ASAN use-after-return mode can heap-allocate scope objects, making this-pointer order unusable for stack-position checks. (code)
- 2018-11-14 (4abeb1bb) rationale: DeferExceptionScope uses RAII SetForScope on m_exception and m_lastException so VM exception state is restored across early returns. (code)
- 2019-05-22 (c4168a7f) pitfall: createError must clear exceptions thrown while resolving rope strings before returning its own OOM error, preserving DECLARE_CATCH_SCOPE invariants. (code)
- 2021-04-08 (c8036597) rationale: TerminationException bypasses debugger notification because termination is internal propagation detail, not an ordinary exception event. (sourced)
- 2021-04-10 (342fdbaa) pitfall: servicing termination at exception-check sites requires a short RAII deferral scope around initialization code that is not exception-safe, but that scope must not wrap long blocking work. (sourced)
- 2022-02-22 (87f99cc5) pitfall: ShadowRealm remote-function unwinding marks boundary crossing but must still use ordinary unwind to notify debugger, copy callee saves, and stop at entry frames. (code)
- 2025-01-20 (c6956e26) pitfall: termination-request flags must not be cleared at VM exit while NeedTermination remains pending, or clients cannot observe the TerminationException thrown by trap handling. (code)
- 2025-11-17 (d079117b) rationale: callee-save copying and debugger-unwind notification are shared in UnwindFunctorBase so exception unwinding and JSPI suspension walkers use one frame-unwind implementation. (sourced)
- 2026-01-15 (67abaaa3) pitfall: exception-clearing sites must use tryClearException and propagate failure so termination exceptions cannot be swallowed while handling ordinary exceptions. (code)
- 2026-01-22 (5715596e) statement: TopExceptionScope is for top-of-JS-stack sites where exceptions must not propagate further, and most code should use ThrowScope because termination may require rethrowing. (code)

## Moves

- 2005-07-19 (af8294dc) removed: C++ exception support was removed from JSC; the tryCall/tryGet/tryPut wrapper layer that caught C++ exceptions from KJS host functions became dead code once JSC switched to Completion-based error propagation exclusively. (sourced)
- 2007-12-20 (a54a94ac) replaced [[completion-value-return]]: Returning Completion structs (type+value+target pointer) from every execute() call allocated stack space for three fields on every statement dispatch; storing completion type in ExecState and returning JSValue* directly eliminates the struct overhead, giving 2.4% SunSpider speedup (first attempted in 2663, rolled back, re-applied in 2665 with a bug fix). (sourced)
- 2011-03-01 (b740d069) replaced [[global-exception-deprecatedptr-root]]: The global exception slot in JSGlobalData held a DeprecatedPtr<Unknown> which did not correctly classify the slot as a GC root (exempt from write-barrier requirements); GCRootPtr<T> is introduced as a WriteBarrierBase<T> subclass that uses setWithoutWriteBarrier() unconditionally, making the GC-root nature explicit in the type system while hiding the write-barrier-bypass from non-root slots. (code)
- 2015-06-05 (cc4c6bff) replaced [[thrown-value-only-vm-exception]]: Wrapping the thrown JSValue, captured stack, and debugger notification state in a GC object lets rethrows preserve the original throw stack and lets finally/synthesized-finally rethrows avoid being treated as new uncaught debugger events. (sourced)
- 2015-09-17 (96333e76) replaced [[generic-unwind-uncatchable-filtering]]: Moving uncatchable-exception filtering to op_catch removes the requirement that every catchable exception path pass through genericUnwind, which enables DFG exception checks to jump directly to in-frame handlers. (sourced)
- 2016-09-07 (1f253c16) replaced [[throw-scope-only-exception-verification]]: Checking and clearing exceptions needed a common scoped protocol instead of a throw-only scope, so JSC replaced ThrowScope-only verification with ExceptionScope plus CatchScope and funnels throwException, clearException, and exception checks through scope objects. (code)
- 2021-04-13 (57e02476) replaced [[termination-exception-invisible-cell]]: A JSString termination exception value can be converted to a string by C++ clients that catch termination at the outermost point, while the value remains invisible to JavaScript because TerminationException cannot be caught. (sourced)
- 2021-12-11 (f33b362a) replaced [[legacy-reenterable-termination]]: Clients missing TerminationException catch sites made worker termination error-prone, so workers opt into having VM::throwTerminationException set executionForbidden immediately while legacy clients can still re-enter after termination. (sourced)
