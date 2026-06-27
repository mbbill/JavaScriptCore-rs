- prepareOSREntry converted values and reshuffled slots directly in the caller's stack frame.
- The slow path returned a machine-code target after modifying the live ExecState in place.

## Moves

- 2014-02-17 (e9207932) replaced by [[osr]]: prepareOSREntry stopped directly editing the caller's stack frame and instead builds the target frame in a scratch buffer for an assembly thunk to copy into place, avoiding stack clobbering concerns and ASan crashes. (sourced)
