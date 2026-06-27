- Wasm atomic wait and notify operations park directly on the memory address. (`ParkingLot`)
- Wait results are derived from the parking-lot result and validation state.

## Moves

- 2022-12-24 (aa3a6446) replaced by [[memory-model]]: Wasm atomics moved onto WaiterListManager so JS Atomics and Wasm atomics share the same waiter lists for interoperable wait/notify on shared memory. (sourced)
