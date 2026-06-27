- Caged adds were hidden in a PatchpointValue with base and masked pointer arguments.
- The patchpoint generator emitted the add in the backend callback and declared no effects.

## Moves

- 2017-08-12 (753a4af6) replaced by [[b3]]: The patchpoint form hid Add(ptr, largeConstant) from harmful B3 reassociation but also imposed patchpoint costs, backend callbacks, and prevented CSE, while Opaque blocks pre-Air reasoning yet remains pure, idempotent, and CSE-able until LowerToAir treats it as Identity. (code)
