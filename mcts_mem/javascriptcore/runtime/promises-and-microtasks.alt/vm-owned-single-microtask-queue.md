- The VM owns one built-in microtask queue.
- Marking and queue draining visit that single queue directly from VM hooks.
- JavaScriptCore framework use is the only queue owner represented in the VM.

## Moves

- 2025-03-04 (43c25c97) replaced by [[promises-and-microtasks]]: MicrotaskQueue needed to support multiple instances associated with one VM so it could later cover WebCore use cases instead of only the JavaScriptCore framework default queue. (sourced)
