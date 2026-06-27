- Ref-counted and unique ownership helpers make ownership transfer explicit at API boundaries, using adopt/leak semantics for transfer and raw access only as a non-owning view.
- Cross-thread queues transfer exclusive ownership of tasks instead of copying ref-counted values through queue storage.
- Platform object smart pointers separate the generic pointer template from platform-specific ref/deref hooks.
- Bound function wrappers choose storage traits for owning parameters, letting call-time access avoid unnecessary retain/release churn.

## Moves

- 2009-11-02 (a043c143) replaced [[message-queue-refcounted-tasks]]: The old design stored tasks by value in the deque (or required DataType to be a RefPtr for shared ownership), imposing cross-thread refcount churn; the new design gives the queue exclusive ownership via OwnPtr, transferring it to the receiver as PassOwnPtr, eliminating all threadsafe refcounting overhead on every enqueue and dequeue. (sourced)
- 2010-08-25 (60d2a851) replaced [[glib-coupled-refptr]]: GRefPtr<T> hard-wired ref/deref to GLib object system, blocking use on non-GLib platforms (EFL, Cairo); PlatformRefPtr<T> separates the smart-pointer template from platform-specific refPlatformPtr/derefPlatformPtr hooks so Cairo and EFL ports can adopt it without a GLib dependency. (sourced)
- 2011-06-16 (9ffcb423) dropped: loose OwnPtr compatibility shim — Apple-internal clients that required LOOSE_OWN_PTR (raw-pointer construction and assignment of OwnPtr/PassOwnPtr without adoptPtr) had migrated away, so the escape hatch was removed to enforce strict ownership discipline site-wide. (sourced)
- 2011-12-27 (c8ca0d41) replaced [[wtf-bound-function-parameter-stores-declared-type]]: Bound parameters are stored through ParamStorageTraits so RefPtr and PassRefPtr can keep owning RefPtr storage while calls peek as raw pointers to avoid reference-count churn. (code)
