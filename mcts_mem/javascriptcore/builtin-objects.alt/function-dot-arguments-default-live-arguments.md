- Stack frame reification for function.arguments created ordinary live Arguments objects by default.
- The default path exposed real argument count and register-backed values rather than a compatibility-gated fake object.

## Moves

- 2014-09-27 (b59a1014) replaced by [[builtin-objects]]: The support stayed behind a test-enabled option while default execution returned zero arguments because removing the compiler/runtime support outright was considered too risky until compatibility was known. (sourced)
