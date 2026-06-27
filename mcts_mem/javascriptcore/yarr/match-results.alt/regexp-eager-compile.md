- RegExp construction compiles JIT or bytecode immediately.
- RegExpRepresentation is allocated at construction time.
- match() uses already-compiled code and does not receive the VM/global data needed for lazy compilation.

## Moves

- 2011-05-25 (28ba6e56) replaced by [[match-results]]: RegExp construction now only validates the pattern and extracts numSubpatterns; JIT/bytecode codegen is deferred to the first match() call, reducing construction cost for regexps that are created but never executed. (sourced)
