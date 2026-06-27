- Inline arity fixup filled shifted arguments and undefined padding with immediate caller-origin SetLocals.
- The argument movement ran before the callee InlineStackEntry existed.

## Moves

- 2017-09-15 (20af43a9) replaced by [[call-dispatch]]: Inline arity fixup changed to a two-phase commit because exiting from caller-origin SetLocals after argument memcpy could expose a clobbered caller frame, while delayed callee-origin SetLocals exit with the callee frame already set up. (code)
