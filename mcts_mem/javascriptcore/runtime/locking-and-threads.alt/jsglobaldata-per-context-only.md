- All global data is created explicitly per context group.
- There is no shared singleton and no compatibility path for API clients relying on implicit shared global-data locking.
- API entry does not reacquire a shared-instance lock for legacy contexts.

## Moves

- 2008-08-20 (98042fa9) replaced by [[locking-and-threads]]: A shared JSGlobalData singleton with implicit JSLock was removed in a prior commit but reinstated because too many existing API clients relied on the single-shared-instance and implicit-locking semantics, making backward compatibility the deciding constraint. (sourced)
