- A call loads the callee CodeBlock and ctiCode on every execution.
- The call instruction is not patched directly to the callee after the first execution.

## Moves

- 2008-10-18 (4f1e70bc) replaced by [[call-linking-stubs]]: The indirect dispatch loaded ctiCode from the callee CodeBlock on every call, which required a memory indirection through the callee JSFunction and CodeBlock; the direct-link mechanism patches the JIT call instruction to jump straight to the callee's code the first time the call executes and arity matches, eliminating the indirection on all subsequent calls; ~20% on deltablue/richards, >12% overall v8 reduction. (sourced)
