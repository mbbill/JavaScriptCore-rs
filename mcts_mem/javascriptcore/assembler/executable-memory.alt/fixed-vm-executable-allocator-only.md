- Executable memory allocation used fixed CPU-sized pools and filtered JIT stub ranges through fixed-pool boundaries.
- The removed on-demand allocator path left range filtering assuming all executable code lived in a fixed pool.

## Moves

- 2016-03-02 (f7dfea1b) replaced by [[executable-memory]]: The on-demand executable allocator removal was rolled back because it caused crashes on Mac 32-bit and on ARM. (sourced)
