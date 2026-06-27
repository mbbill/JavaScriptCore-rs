- Promise reactions, internal jobs, and async-module operations are enqueued as VM-managed microtasks and run at host or API microtask checkpoints.
- A VM can have multiple linked microtask queues, allowing JavaScriptCore framework and WebCore users to share VM state without sharing one hard-coded queue.
- Microtask checkpoint dispatch accepts caller-supplied inline dispatch behavior rather than forcing every task through a virtual dispatcher.
- Async diagnostics and async-function/module lowering keep extra state only where it does not impose permanent costs on the hot promise path.

## Facts

- 2019-09-09 (504d6499) measurement: eager rare-data allocation for anonymous builtin functions in Promise resolve/reject closure creation caused about a 1.7x slowdown; deferring the rare-data allocation removed the fast-path heap allocation. (sourced)
- 2025-09-03 (94283c56) measurement: async stack traces regressed JetStream3/async-fs by 2-3% and JetStream3/doxbee-async by 4%, so the flagged implementation was removed. (sourced)
- 2026-06-10 (5c64352c) pitfall: recursive async-module parent walks can hard-overflow the native stack because the operations are infallible microtasks and cannot throw RangeError; explicit worklists avoid the recursion. (sourced)

## Moves

- 2025-03-04 (43c25c97) replaced [[vm-owned-single-microtask-queue]]: MicrotaskQueue needed to support multiple instances associated with one VM so it could later cover WebCore use cases instead of only the JavaScriptCore framework default queue. (sourced)
- 2025-03-05 (a70ba5ac) replaced [[microtask-queue-virtual-dispatch-checkpoint]]: The checkpoint now accepts an inline caller-supplied dispatch functor so WebCore can bypass a virtual MicrotaskDispatcher::run call for the frequent JavaScript microtask case. (sourced)
- 2025-09-03 (94283c56) removed: async stack trace promise reaction chain — The flagged async stack trace implementation was removed because it regressed JetStream3/async-fs by 2-3% and JetStream3/doxbee-async by 4%. (sourced)
- 2025-12-29 (f42c3630) replaced [[async-function-generator-wrapper-for-all-bodies]]: Async function bodies with no lexical await no longer need a separate body function or generator because there is no suspend/resume point, so the wrapper can inline the body and directly settle the promise. (sourced)
- 2026-06-10 (5c64352c) replaced [[recursive-async-module-parent-walk]]: GatherAvailableAncestors and AsyncModuleExecutionRejected are infallible microtask operations over potentially deep async module graphs, so native recursion is replaced with explicit worklists to avoid hard stack overflows without throwing RangeError. (sourced)
