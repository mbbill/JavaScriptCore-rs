- Any nested closure sets a ClosureFeature flag on the containing function.
- The flag forces activation even when no variable from that function is captured.
- Activation need is decided at function granularity, not per referenced variable.

## Moves

- 2010-09-16 (24dfdf84) replaced by [[scope-chain-and-activation]]: The ClosureFeature flag was set whenever any closure was present, forcing activation on functions that did not actually capture any variables from an enclosing scope; per-variable capture analysis lets functions skip activation when no variables from them are closed over. (sourced)
