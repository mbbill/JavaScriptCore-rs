- Compiled JavaScript entrypoints distinguish ordinary and arity-check entries but not register-argument callers.

## Moves

- 2016-12-12 (32765c8c) replaced by [[platform-calling-convention]]: Register-argument JS calls need distinct compiled entries and thunks from stack-argument calls because callers may arrive with callee, argument count, and leading JS arguments in platform argument registers instead of the call frame. (code)
