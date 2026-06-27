- API entry state and JS lock ownership are represented by separate shim classes.
- Callers can set VM identifier-table state and register machine threads without taking the JS lock.
- API callback and lock-dropping paths restore VM entry state separately from lock acquisition.

## Moves

- 2014-03-04 (3c18fd59) replaced by [[locking-and-threads]]: JSLock is now taking on all of APIEntryShim's responsibilities since there is never a reason to take just the JSLock. (sourced)
