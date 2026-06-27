- GC scheduling uses allocation pressure, collection scope, and pause budget.
- Collector and mutator coordination uses a connection state machine with begin, fixpoint, concurrent, reloop, and end phases.
- Eden and Full collection scopes share scheduling infrastructure with scope-specific limit and remembered-set state.
- Space-time scheduling charges mutator allocation against elapsed collection time.
- Incremental sweeping and deferred helper work have schedules separate from root marking.

## Facts

- 2016-01-19 rationale: concurrent collection splits mutator and collector execution with an explicit connection protocol so the mutator can resume between collector phases (code).
- 2017-08-29 rationale: space-time scheduling ties pause duration to allocation volume since the cycle began, limiting worst-case pauses without abandoning concurrent progress (sourced).

## Moves

