- Program and eval frames place null in the callee slot.
- Debugger and unwind logic identify program frames by null callee rather than a scope-carrying callee object.
- Arity-check code assumes non-null callees are JSFunction frames.

## Moves

- 2014-09-13 (31e3e943) replaced by [[entry-api]]: Program and eval CallFrames now require a non-null scope-carrying callee slot, while preserving function-only behavior by dynamically distinguishing JSFunction from non-function JSCallee objects. (code)
