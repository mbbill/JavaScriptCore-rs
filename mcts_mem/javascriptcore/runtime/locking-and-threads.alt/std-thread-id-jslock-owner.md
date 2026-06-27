- JSLock records ownership as `std::thread::id`.
- VM owner-thread queries expose standard-library thread identifiers.
- Thread ownership comparisons cannot directly locate the platform thread object used by stack marking and trap signaling.

## Moves

- 2017-03-01 (119091b3) replaced by [[locking-and-threads]]: PlatformThread was chosen because std::thread::id cannot find the corresponding MachineThreads::Thread, suspend or resume threads, or signal a thread for non-polling VM traps. (sourced)
