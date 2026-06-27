- Message queues store task values directly in a deque.
- Shared ownership is achieved by making queued data a RefPtr-like value type.
- Dequeue writes the copied value into an out-parameter.

## Moves

- 2009-11-02 (a043c143) replaced by [[ref-counted-ownership]]: The old design stored tasks by value in the deque (or required DataType to be a RefPtr for shared ownership), imposing cross-thread refcount churn; the new design gives the queue exclusive ownership via OwnPtr, transferring it to the receiver as PassOwnPtr, eliminating all threadsafe refcounting overhead on every enqueue and dequeue. (sourced)
