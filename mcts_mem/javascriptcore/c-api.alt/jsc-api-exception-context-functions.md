- Exception state is read and written with per-context get, set, and clear functions.
- Callbacks signal errors by mutating context exception state rather than by an explicit exception out-parameter.

## Moves

- 2006-07-11 (c0949c09) replaced by [[c-api]]: Per-context exception get/set/clear functions were replaced by JSValue** exception out-parameters on every callback signature, using exec->exceptionSlot() to provide the slot; this makes exception flow explicit at each call site rather than requiring callers to poll/set a context-global exception state. (sourced)
