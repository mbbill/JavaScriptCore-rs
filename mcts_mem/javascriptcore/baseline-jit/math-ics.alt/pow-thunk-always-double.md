- Math.pow thunk always returns a double-backed JSValue after pow computation.

## Moves

- 2010-08-19 (2e397785) replaced by [[math-ics]]: Math.pow() thunk unconditionally returned a double-backed JSValue; when the result fits in Int32 (e.g. 2^3=8), a double-backed value was extremely slow as an array subscript because it required unboxing; the fix attempts conversion to Int32 first and falls through to double only when needed. (sourced)
