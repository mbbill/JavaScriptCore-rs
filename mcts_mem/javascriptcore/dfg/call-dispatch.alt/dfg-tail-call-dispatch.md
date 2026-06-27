- Tail calls were parsed through the ordinary call and inlining path.
- The inline stack entry kept an entry block but did not use it as a recursive loop target.

## Moves

- 2017-11-08 (3894ac30) replaced by [[call-dispatch]]: Recursive tail calls are converted in DFGByteCodeParser into jumps after op_enter so the resulting loop can be optimized, while limiting entry-block splitting to functions with tail calls because unconditional splitting hurt Octane/raytrace. (sourced)
