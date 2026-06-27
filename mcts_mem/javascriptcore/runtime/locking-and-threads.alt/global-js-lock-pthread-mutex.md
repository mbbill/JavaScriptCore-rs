- The global JS lock is backed directly by pthread mutex state on Darwin and pthread ports.
- Non-pthread ports compile lock operations as empty stubs.
- Portability of the runtime lock is expressed with preprocessor branches inside the lock implementation.

## Moves

- 2012-09-16 (041a7b02) replaced by [[locking-and-threads]]: Using a raw pthread_mutex_t (guarded by OS(DARWIN)||USE(PTHREADS)) left non-pthread platforms with no-op stubs causing real synchronization failures on Windows; WTF::Mutex abstracts over platform threading primitives, enabling correct locking on all ports. (sourced)
