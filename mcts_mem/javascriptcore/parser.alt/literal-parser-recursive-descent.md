- JSON literal parsing used mutually-recursive statement, expression, array, and object routines.
- A depth counter aborted parsing after a small recursion limit instead of avoiding recursion.

## Moves

- 2009-06-13 (c69b558f) replaced by [[parser]]: Recursive descent with a depth-limit StackGuard (depth<10) can stack-overflow on deeply nested JSON; replacing it with a hand-rolled PDA (explicit stateStack/objectStack vectors) eliminates native call-stack growth entirely. (sourced)
