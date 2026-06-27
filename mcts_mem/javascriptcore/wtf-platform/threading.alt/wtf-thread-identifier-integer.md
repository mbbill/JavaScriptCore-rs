- Thread identifiers are represented as uint32_t values.
- Native pthreads or Windows handles are stored in per-platform maps keyed by the integer identifier.
- Looking up a platform thread requires acquiring the thread-map lock.

## Moves

- 2009-05-11 (feae6bb7) replaced by [[threading]]: The old uint32_t ThreadIdentifier could not hold a native pthread_t (a pointer on 64-bit) or a Windows HANDLE without an indirection through a per-platform ThreadMap of integer-to-native-id; replacing it with a class wrapping PlatformThreadIdentifier eliminates the ThreadMap entirely and allows direct use of native thread ids. (sourced)
