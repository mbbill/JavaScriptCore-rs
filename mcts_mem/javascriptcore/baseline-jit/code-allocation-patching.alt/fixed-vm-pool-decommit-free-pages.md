- Free JIT pool pages are returned through PageReservation::decommit.
- Needed pages are committed again with PageReservation::commit.

## Moves

- 2012-05-10 (fef52580) replaced by [[code-allocation-patching]]: Work around the problem by using a different madvise() flag, but only for the JIT memory allocator. (sourced)
