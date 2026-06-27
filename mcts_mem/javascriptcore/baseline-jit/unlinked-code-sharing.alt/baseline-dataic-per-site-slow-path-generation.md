- Baseline DataIC emits a distinct slow-path code sequence for each IC site.

## Moves

- 2023-09-19 (b577e3e9) replaced by [[unlinked-code-sharing]]: Baseline DataIC slow paths were consolidated into shared thunks once register usage was aligned, so both generated IC stubs and baseline IC sites could jump to one slow path and return via StructureStubInfo::doneLocation. (sourced)
