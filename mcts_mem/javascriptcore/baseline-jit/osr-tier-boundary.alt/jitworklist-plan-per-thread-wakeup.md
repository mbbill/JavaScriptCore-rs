- Each enqueued JIT plan wakes a compiler thread without weighting the global queue load.

## Moves

- 2025-04-24 (6d26b6d6) replaced by [[osr-tier-boundary]]: The worklist now scales compiler threads from weighted per-tier queue and in-flight load instead of waking one thread per enqueued plan, because extra compiler threads impose wakeup, synchronization, cache, contention, and scheduler overhead when the queue is too small. (sourced)
