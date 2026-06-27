- Pending deferred-work tickets are tracked in a global-object HashMap keyed by ticket.
- Ticket data owns vectors of write barriers and unregisters itself from the global object at destruction.
- The global object visits each barrier vector to keep deferred-work dependencies alive.

## Moves

- 2024-06-19 (2aa59a0d) replaced by [[global-object]]: WeakHashSet lets JSGlobalObject track DeferredWorkTimer tickets through weak ownership instead of TicketData destructors manually unregistering a HashMap of write barriers. (code)
