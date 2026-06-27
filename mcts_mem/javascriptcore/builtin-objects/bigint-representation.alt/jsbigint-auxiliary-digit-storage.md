- JSBigInt digit words were stored in a separate primitive-Gigacage auxiliary allocation.
- The cell carried a caged pointer to digit storage and GC size accounting for that separate allocation.

## Moves

- 2026-02-07 (dbc50284) replaced by [[bigint-representation]]: JSBigInt digits became immutable, length-known payloads, making trailing cell storage sufficient and cheaper for direct access than a separately allocated caged digit buffer. (sourced)
