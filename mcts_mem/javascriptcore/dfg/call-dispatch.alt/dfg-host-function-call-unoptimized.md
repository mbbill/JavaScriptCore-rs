- Host functions reached DFG as ordinary Call nodes.
- NativeExecutable and generated hash entries carried no intrinsic identity for DFG parsing.

## Moves

- 2011-09-16 (6e15bf2a) replaced by [[call-dispatch]]: DFG could not inline host (native) functions because it had no mechanism to identify which native function a Call node targeted; adding intrinsic annotations to NativeExecutable and hash table entries lets DFG detect calls to e.g. Math.abs at parse time and substitute ArithAbs nodes, enabling full DFG optimization on those paths. (sourced)
