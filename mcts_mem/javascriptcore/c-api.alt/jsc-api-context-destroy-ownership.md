- Context lifetime is controlled by a manual destroy function.
- The same context handle acts as the owning public context reference.

## Moves

- 2006-07-14 (723987b9) replaced by [[c-api]]: JSContextDestroy (manual delete) was replaced by retain/release ref-counting because embedding environments need shared ownership of contexts; also splits JSContextRef (non-owning, const) from JSGlobalContextRef (owning) to enforce correct usage at the type level. (sourced)
