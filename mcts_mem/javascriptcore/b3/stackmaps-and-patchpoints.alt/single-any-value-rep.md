- ValueRep::Any meant B3 could pick any input representation.
- Any implied a cold use in the register allocator.
- Missing stackmap constraints and append(value) both collapsed to the same Any representation.

## Moves

- 2015-12-04 (af93be95) replaced by [[stackmaps-and-patchpoints]]: A single Any could not express whether an unconstrained stackmap use should be warm, cold, or late-cold, so ValueRep was split to carry register-allocation temperature and lateness directly. (code)
