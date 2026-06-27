- Parallel jobs are capped by a compile-time maximum of two worker slots.
- The main thread participates as a worker and at most one helper is normally created.
- Processor count does not influence the default helper limit.

## Moves

- 2011-10-18 (a8bd6a8c) replaced by [[threading]]: The fixed two-thread cap was replaced by a lazily computed processor-count cap, with 2 retained only as the fallback for platforms where processor count is unavailable. (code)
