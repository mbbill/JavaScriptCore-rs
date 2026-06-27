- The O1 path ran linear register allocation and later graph-coloring stack allocation as separate phases.
- Stack allocation recomputed liveness and interference after register allocation.
- The fast register path still paid the full stack-slot coloring pipeline.

## Moves

- 2017-04-12 (72e83448) replaced by [[register-allocation]]: For B3 -O1, doing stack allocation inside linear scan reuses the liveness already computed for register allocation and skips the graph stack allocator's liveness/interference/coalescing/coloring pipeline, yielding a reported 21% wasm -O1 compile-time speed-up while accepting less optimal frames. (sourced)
