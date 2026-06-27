- Wasm OMG admission used function byte size as the proxy for allocator feasibility.
- Tier-up checks and optimization-level selection rejected large functions before graph coloring saw temporary counts.

## Moves

- 2020-10-23 (061db521) replaced by [[register-allocation]]: Graph-coloring register allocation memory grows with the square of the number of temporaries, while Wasm byte size was only an indirect proxy that rejected large functions that did not have many temporaries. (sourced)
