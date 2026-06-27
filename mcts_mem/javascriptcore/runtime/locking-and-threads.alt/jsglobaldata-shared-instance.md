- Contexts created through the embedding API share one global-data singleton by default.
- All contexts in the process share one heap and must rely on JSLock for serialized access.
- The shared-instance flag controls implicit locking behavior at API entry.

## Moves

- 2008-07-30 (aebe5fac) replaced by [[locking-and-threads]]: The single shared JSGlobalData instance made independent concurrent execution of separate JSGlobalContexts impossible because all contexts shared one heap and required JSLock for every operation; replacing it with per-group JSGlobalData (each JSGlobalContextCreate gets its own group by default) removes the implicit locking requirement and allows truly independent contexts. (sourced)
