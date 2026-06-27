- The debugger is a VM-level execution observer attached to JavaScript globals and execution threads (`Debugger`).
- Breakpoint state is owned by the core debugger as ref-counted breakpoint objects with link-before-resolve position binding, actions, ignore counts, conditions, and auto-continue behavior.
- Special pauses use the same breakpoint-object model as user breakpoints; exception, debugger-statement, assertion, and microtask pauses are not separate boolean modes.
- Stepping is represented as pause opportunities at statement, expression, await, return, and end-of-program points.
- Paused call-frame inspection uses fresh frames invalidated after the pause callback and can expose lexical scopes, this-values, tail-deleted frames, and host-installed scope extensions.
- Blackboxing is stored per source range with independent skip and defer flags.
- Inspector runtime access is mediated by environment-owned agents, typed dispatchers, and builtin-backed injected scripts evaluated inside the inspected global.
- Remote inspection maintains thread-safe target registration, target-owned automatic-inspection pauses, and frontend connections through local or remote channels.

## Facts

- 2013-10-05 (8b7b2eb9) rationale: debugger call frames track validity themselves, giving clients a fresh frame around each pause callback that is invalidated when the callback returns. (sourced)
- 2013-11-08 (d5f0e222) rationale: user-defined breakpoints moved into core debugger breakpoint records, leaving the inspector layer to track only breakpoint actions by identifier. (sourced)
- 2014-07-01 (3af5ff5e) pitfall: per-line breakpoint storage must preserve stable breakpoint addresses because the breakpoint-ID map stores pointers that vector compaction or reallocation can invalidate. (code)
- 2015-09-05 (b721e4b4) rationale: agent construction uses an environment-supplied context so every domain agent owns its dispatchers while sharing environment, injected-script, frontend-router, and backend-dispatcher objects with lifetimes longer than the agents. (sourced)
- 2016-03-23 (01e6e308) rationale: debugger-enabled DFG compilation stopped rejecting inlining and instead jettisons optimized code when any inlinee receives a newly set breakpoint. (code)
- 2016-05-10 (f3d09848) rationale: call-frame evaluation moved command-line API injection from a generated closure that replaced global eval into the host call-frame path, where a scope-extension object can be installed during evaluation. (code)
- 2016-05-16 (87b90026) rationale: tail-deleted frames became debugger-visible virtual frames carrying enough data for display, source location, and evaluation. (code)
- 2016-06-29 (2f208b14) rationale: scope metadata moved from scalar scope-type queries into per-scope descriptions backed by symbol-table, code-block, and executable links. (code)
- 2016-09-30 (6a4691b2) rationale: statement and call-frame hooks could not express pauses before individual call expressions or multiple expressions in one statement, so stepping moved to explicit pause opportunities and expression hooks. (sourced)
- 2016-09-30 (4f8dbdba) rationale: binding breakpoints directly to already-generated debug bytecodes could not resolve blank lines, comments, or function-signature requests before execution, so parsing records pause positions for pre-execution breakpoint resolution. (sourced)
- 2016-11-10 (e60d6d8b) rationale: step commands produce exactly one next debugger event, paused or resumed, instead of a resumed event followed by a possible pause. (sourced)
- 2017-01-29 (dfe2cdca) rationale: async stack tracking uses shared truncatable stack-trace nodes because identifier-parent chains could grow indefinitely. (sourced)
- 2019-09-04 (ac0a8fb1) rationale: blackboxing moved from boolean skip-all semantics to typed skip-and-defer behavior that can save a pause reason until execution leaves blackboxed code. (code)
- 2019-10-12 (e2b901dc) rationale: URL-pattern blackboxing replaced exact-URL sets so minified library bundles can be blackboxed by pattern. (sourced)
- 2022-05-12 (1d75ec6c) rationale: injected-script source is parsed as a builtin so inspector helper code uses guaranteed engine-provided builtins instead of user-overridden global properties. (sourced)
- 2022-07-16 (6545a6f6) rationale: microtask async stacks require queue-time/run-time correlation by microtask identity, and nested async dispatch requires a stack rather than a single current async-call identifier. (code)
- 2023-08-30 (eba01be5) rationale: private fields are exposed to injected-script inspection as private symbols because a string-keyed descriptor object cannot represent duplicate private names. (code)
- 2024-07-30 (cc3ce348) rationale: source-wide blackbox state could not encode multiple ranges with independent skip and defer flags inside one script. (code)
- 2025-03-18 (e02e121a) rationale: automatic-inspection pause state moved onto each target because service-worker targets may need to block or unblock a thread different from the process singleton's thread. (sourced)
