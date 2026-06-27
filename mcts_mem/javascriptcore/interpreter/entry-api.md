- Host entry to JavaScript is through explicit Interpreter/VM calls that carry a global object, source buffer, this value, and ProtoCallFrame instead of relying on a process-global current interpreter.
- Program, eval, module, call, construct, and microtask entry paths all prepare executable code and frame state before handing control to LLInt or JIT entrypoints.
- Native and shell entry users configure VM options, source ownership, and stack limits before VM creation or code entry; entry setup keeps CodeBlock and JITCode paired across GC-capable work.

## Facts

- 2004-06-07 (4a8bf746) rationale: adding sourceURL and startingLineNumber to Interpreter::evaluate broke JavaScriptGlue at source and binary level, so a backward-compatible overload preserved the old signature. (sourced)
- 2014-09-13 (31e3e943) pitfall: C_LOOP previously needed Interpreter-level LLINT_C_LOOP branches that built frames directly and called CLoop::execute; dispatching through Executable JITCode entries removed that entry split. (code)
- 2014-09-15 (0e6ddf4e) rationale: program and eval CallFrames use non-null JSCallee scope carriers so debugger and unwind logic distinguish frame kind by callee type rather than nullness. (code)
- 2020-03-13 (845ec392) pitfall: after a CodeBlock is fetched for ProtoCallFrame setup, any intervening GC-capable work can replace the executable's CodeBlock, so entry setup must refetch or hold DisallowGC until frame and JITCode are paired. (code)
- 2020-03-14 (78640c0d) pitfall: the JITCode handle that pairs CodeBlock and executable code must be loaded only for JS call/construct paths because native calls initialize ProtoCallFrame with a null CodeBlock. (code)
- 2021-03-10 (76ff1da1) pitfall: ProtoCallFrame::init stores the module argument array by reference for later JIT execution, so executeModuleProgram must keep that stack array live until jitCode->execute returns. (code)
- 2023-11-21 (70ed411a) rationale: VMEntryRecord no longer stores ProtoCallFrame::calleeValue; stack-overflow frame construction uses throwOriginFrame->jsCallee()->globalObject() when available and vm.entryScope->globalObject() otherwise. (code)
- 2025-10-06 (7598b7e8) rationale: async stack recovery stores a JSCell context in VMEntryRecord for microtask calls so unwinding can find generator context at entry-frame boundaries without recognizing promiseReactionJob argument positions. (code)

## Moves

- 2002-03-22 (9491afaa) replaced [[kjscript-public-api]]: The KJScript class (global facade with static current() context) was replaced by the Interpreter class that takes an explicit global Object in its constructor, enabling multiple independent interpreter instances without relying on a global singleton. (code)
- 2005-12-16 (84892347) replaced [[evaluate-ustring-api]]: Interpreter::evaluate() parameter changed from UString to UChar*+int to avoid constructing a UString object solely for parsing, which caused unnecessary allocation and copying of the source text. (sourced)
- 2007-10-26 (f10a4fbc) replaced [[context-execstate-split]]: Context and ExecState were always created and destroyed together and carried redundant interpreter pointers; ExecState held only a Context* plus exception state while Context held all execution-context data (scope chain, activation, this value, code type, calling context); merging eliminates one allocation, one pointer indirection, and one pointer field per call frame. (sourced)
- 2007-12-06 (e2f9a746) replaced [[interpreter-owned-global-state]]: The Interpreter class held all runtime data (builtins, prototypes, constructors, currentExec, debugger, timeout state) with JSGlobalObject as a thin client; moving these into JSGlobalObject's JSGlobalObjectData struct eliminated a separate Interpreter indirection and fixed a bootstrapping bug where globalExec was used before Interpreter initialised the global object. (sourced)
- 2013-12-05 (b2ea0fe7) replaced [[c-loop-llint-special-casing-at-interpreter-entry]]: C Loop LLINT was made to dispatch through Executable JITCode entries so it shared the same call/construct entry mechanism as the ASM LLINT and no longer needed Interpreter-level LLINT_C_LOOP branches. (code)
- 2014-09-13 (31e3e943) replaced [[nullable-callee-program-eval-frames]]: Program and eval CallFrames now require a non-null scope-carrying callee slot, while preserving function-only behavior by dynamically distinguishing JSFunction from non-function JSCallee objects. (code)
- 2016-07-13 (5a03dc32) replaced [[single-soft-stack-recursion-limit]]: JSC split recursion checks into normal and soft stack limits because host/VM code with known stack usage can use the smaller guaranteed reserve while JS entry points and code that may call arbitrary JS need the more conservative soft reserve. (sourced)
