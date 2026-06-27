- Array.prototype.every was implemented as a hand-written C++ host function.
- The implementation used CachedCall and direct array fast paths to mimic the builtin algorithm.

## Moves

- 2014-02-12 (fa5f5a32) replaced by [[builtins-codegen]]: Array.prototype.every was moved from a hand-written C++ host function to a generated JS builtin function so builtins can be authored in JS while still mimicking host-function behavior at the API boundary. (code)
