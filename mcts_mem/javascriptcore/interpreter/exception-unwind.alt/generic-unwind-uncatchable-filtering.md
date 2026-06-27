- Catchable and uncatchable exception filtering is performed in genericUnwind.
- Catchable exception paths are expected to pass through genericUnwind before reaching handlers.
- In-frame handler jumps from optimizing tiers must rendezvous at the generic unwind path.

## Moves

- 2015-09-17 (96333e76) replaced by [[exception-unwind]]: Moving uncatchable-exception filtering to op_catch removes the requirement that every catchable exception path pass through genericUnwind, which enables DFG exception checks to jump directly to in-frame handlers. (sourced)
