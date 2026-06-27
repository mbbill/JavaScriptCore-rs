- JIT stub routines were indexed by a hash entry for every 16-byte step in each routine's address range.
- A parallel vector owned routines while marking queried the address-to-routine map.
- Removing a routine deleted all step entries from the hash table.

## Moves

- 2019-04-29 (9498e00f) replaced by [[cpu-backends]]: HashMap<uintptr_t,StubRoutine*> registered every 16-byte step of each routine, creating O(size/step) entries per routine and ~2MB table on Gmail; sorted Vector<{startAddress,StubRoutine*}> with binary search shrinks memory to O(count) entries at the cost of a sort before each conservative scan. (sourced)
