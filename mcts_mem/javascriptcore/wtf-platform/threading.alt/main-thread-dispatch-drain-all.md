- Main-thread dispatch swaps the entire pending-function queue into a local vector and runs every item in one drain.
- Worker floods can monopolize the main thread until the queue is empty.
- Rescheduling happens only after a full drain.

## Moves

- 2009-02-12 (6ae8e0c4) replaced by [[threading]]: Draining the entire queue in one shot caused UI freezes when workers flooded the queue; the new algorithm dispatches one item at a time, checks elapsed time against a 50ms threshold, and reschedules if exceeded so user input can be processed between batches. (sourced)
