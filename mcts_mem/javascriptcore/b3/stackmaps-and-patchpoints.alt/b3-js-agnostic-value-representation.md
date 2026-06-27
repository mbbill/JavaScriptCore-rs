- B3::ValueRep represented generic registers, stack locations, constants, and unconstrained locations.
- JS ValueRecovery construction lived outside B3.
- Tail-call frame shuffling could not consume B3 stackmap argument locations directly.

## Moves

- 2015-12-22 (081b5f9a) replaced by [[stackmaps-and-patchpoints]]: B3::ValueRep can now turn itself into a ValueRecovery for a JSValue, making tail-call frame shuffling consume stackmap generation parameters directly instead of translating through a separate representation. (sourced)
