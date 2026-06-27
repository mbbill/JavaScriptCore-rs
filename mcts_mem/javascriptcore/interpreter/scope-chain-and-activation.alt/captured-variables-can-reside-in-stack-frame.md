- Captured variables can initially reside in stack frame registers and later be torn off into heap storage.
- Argument and activation metadata track whether a variable has moved between stack and activation storage.
- Variable storage kind is discovered from capture ranges and tear-off state.

## Moves

- 2015-03-26 (329af672) replaced by [[scope-chain-and-activation]]: Declared variables now have an explicit, stable storage kind so a variable no longer moves between heap and stack during its lifetime. (sourced)
