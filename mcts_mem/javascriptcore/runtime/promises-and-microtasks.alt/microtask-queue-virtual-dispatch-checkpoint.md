- Each queued microtask chooses dispatch by calling a virtual dispatcher when one is present.
- The checkpoint loop owns both queue draining and per-task dispatch decisions.
- JavaScript microtasks and custom host dispatch take the same virtual path.

## Moves

- 2025-03-05 (a70ba5ac) replaced by [[promises-and-microtasks]]: The checkpoint now accepts an inline caller-supplied dispatch functor so WebCore can bypass a virtual MicrotaskDispatcher::run call for the frequent JavaScript microtask case. (sourced)
