- Math.pow delegated to system libm pow after ECMAScript NaN edge-case guards.
- iOS ARM_THUMB2 denormal behavior was not separated from ordinary libm calls.

## Moves

- 2012-06-08 (a97e3a24) replaced by [[builtin-objects]]: On iOS ARM_THUMB2, system pow is used only when neither input is denormal and the result is nonzero or an edge case; otherwise fdlibmPow handles cases where denormal support may be required. (code)
