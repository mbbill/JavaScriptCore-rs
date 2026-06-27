- Parser operations and collector operations each own their own recursive lock state.
- Collection, allocation, parser entry, and interpreter list mutation coordinate through subsystem-specific mutexes and conditions.

## Moves

- 2002-12-21 replaced by [[locking-and-threads]]: Per-operation locks on the GC (collectorLock) and parser (parserLock) were too fine-grained and caused excessive contention; replacing both with a single PTHREAD_MUTEX_RECURSIVE interpreter-level lock eliminated the overhead. (sourced)
