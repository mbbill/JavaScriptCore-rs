- A JavaScript call frame is a fixed header plus contiguous Register slots; the frame pointer addresses the caller-frame slot and locals/temporaries are addressed as register offsets.
- Program, eval, function, native, and Wasm frames share the call-frame chain and overload selected header slots only when the frame kind makes the alternate meaning unambiguous.
- The VM entry record preserves previous topCallFrame/topEntryFrame state across native boundaries and supplies sentinel frames for stack walking, root finding, and unwinding.
- Stack introspection is centralized in StackVisitor, which walks CallFrame and VMEntryFrame state instead of exposing mutable iterator objects.
- Tail calls and varargs frame rewrites use explicit frame-shuffle metadata instead of bulk-copying prepared frames.

## Facts

- 2008-01-14 (72f77c42) pitfall: ExecState::mark assumed ScopeChain marking covered every activation, but some activations existed outside any scope chain, so ExecState had to mark its activation directly. (code)
- 2008-01-16 (d7f28c6f) pitfall: ExecState::mark walked m_callingExec but skipped m_savedExec when they differed, missing cross-window eval activations during GC. (sourced)
- 2008-03-20 (a73cf80b) rationale: functions that neither call eval nor create closures can embed the root scope-chain node in ExecState because that node cannot escape. (sourced)
- 2008-03-20 (ff627dc1) measurement: inlining ExecState construction/destruction and global-object activation helpers gave a 1.014x SunSpider speedup. (sourced)
- 2008-03-21 (2984cc16) pitfall: the inline ScopeChainNode refcount shortcut leaked nodes above it because the ScopeChain destructor decremented to 1 instead of releasing; FunctionExecState now pops the inline node explicitly. (code)
- 2008-07-01 (4498f3e1) measurement: replacing registerBase-plus-offset tuples with direct Register* pointers in call-frame headers yielded a 0.8% SunSpider speedup. (sourced)
- 2011-09-27 (6d3b1f0d) rationale: topCallFrame became the anchor for error stack trace capture and the jsc stack() command. (sourced)
- 2011-09-27 (6d3b1f0d) pitfall: stack-trace collection must identify host-call-frame flags before reading caller frames, code blocks, return addresses, or source URLs. (code)
- 2013-11-21 (a49e6ea6) rationale: the VM initializes its native stack limit from the thread that instantiated the VM so parser and bytecode generator work can run before the first JS entry. (sourced)
- 2013-11-21 (a49e6ea6) rationale: VMEntryScope uses a smaller required stack budget while handling stack-overflow errors, preserving enough room for minimal JS execution during error creation. (code)
- 2014-02-01 (b22d989d) measurement: lazy debugger activation materialization improved Octane with WebInspector from 3295 to 7070 while leaving Octane without WebInspector roughly unchanged. (sourced)
- 2014-08-20 (97ced926) pitfall: StackVisitor must initialize from the VM's top call frame and advance to a requested start frame, not treat that requested frame as the top. (sourced)
- 2016-08-02 (c73b93ba) rationale: a failed JS call above a VM entry frame is represented by topCallFrame == topVMEntryFrame, and StackVisitor maps that sentinel to the previous top JS frame. (sourced)
- 2018-09-03 (3778972c) pitfall: CallFrame::unsafeCallee needs an ASAN-suppressed pointer load because trap probing can inspect frames before ASAN-visible setup is complete. (sourced)
- 2023-11-21 (70ed411a) measurement: removing the VMEntryRecord callee store measured cpp-to-js-cached-call at 12.5615 versus 12.8405, a 1.0222x speedup. (sourced)
- 2025-10-28 (cce33018) pitfall: async stack trace collection must inspect the final previousEntryFrame after traversal because top-level await can leave generator context on the last entry frame. (code)
- 2026-04-15 (d5d2f197) statement: Wasm frames use the argumentCountIncludingThis tag half for call-site index, leave its payload half unused for Wasm-to-Wasm, store actual argument count there for Wasm-to-JS, and use arg0 for the first stack result. (code)

## Moves

- 2008-01-22 (f3e70746) replaced [[exec-state-currentexec-savedexec]]: The per-global-object currentExec/savedExec linked-list mechanism caused crashes (including Amazon.com regression) because it failed to correctly track ExecState across multiple JSGlobalObjects and reentrancy; an explicit process-wide Vector<ExecState*,16> stack fixes ownership and enables correct GC marking of all active frames. (sourced)
- 2008-01-26 (d2ae8b8b) replaced [[single-class-execstate]]: Single ExecState class with multiple constructor overloads required a runtime branch in the destructor to determine whether to manipulate activeExecStates; splitting into GlobalExecState/InterpreterExecState/EvalExecState/FunctionExecState encodes execution context kind in the C++ type and pushes lifecycle code to each derived destructor, eliminating the branch. (sourced)
- 2008-07-23 (77e10c97) replaced [[register-jsvalue-no-execstate]]: The old Register::jsValue() signature cannot support on-the-fly JSValue* creation when a register stores a raw double; requiring an ExecState* allows a future implementation to box the double into a heap-allocated JSValue* on demand, which the callee-side had no way to do before. (sourced)
- 2008-10-03 (ef3cde6f) replaced [[execstate-with-global-data-fields]]: ExecState stored m_globalObject and m_globalData redundantly when the same data is reachable through the call frame's scope chain; removing them makes ExecState a thin call-frame-pointer wrapper, enabling further optimization passes and reducing construction cost. (sourced)
- 2012-04-27 (4bbf6b45) replaced [[register-file-explicit-used-end]]: Register-file users only needed committed capacity plus the active frame extent, so deriving reusable/marked range from topCallFrame makes GC mark only the used portion and prevents VM re-entry from exhausting the register file as quickly. (sourced)
- 2014-01-29 (a3ac51de) replaced [[jsc-private-register-stack]]: The old private JSStack representation could not make non-LLInt-C-loop execution use the native thread stack while still estimating stack usage, sanitizing stack memory, and checking VM stack limits from the thread stack origin. (code)
- 2015-01-20 (f2bf0a76) replaced [[closure-stub-code-origin-field]]: CodeOrigin for call frames is now determined from encoded code-origin bits inside the argument-count tag, and callers that find a ClosureCallStubRoutine through CallLinkInfo can use CallLinkInfo's CodeOrigin instead of a duplicate field on the stub. (sourced)
- 2015-09-18 (29d273d9) replaced [[slow-tail-call-frame-copy]]: CallFrameShuffler carries per-argument ValueRecovery and frame-shuffle metadata so fixed-arity tail calls and polymorphic call stubs can rewrite the current frame instead of bulk-copying a prepared frame. (code)
- 2018-02-14 (6be642a9) replaced [[vmtrap-topcallframe-fallback]]: Trap installation may malloc, so it now refuses to run unless the sampled PC proves the thread is in JIT or LLInt code rather than falling back to topCallFrame while the thread may be in C code holding the malloc lock. (code)
- 2025-10-20 (463c854b) replaced [[ipint-absolute-saved-sp-slot]]: Storing SP as an FP-relative offset lets JSPI save frames off-stack and reinstall them at a different stack address without maintaining and relocating a list of absolute SP slots. (sourced)
