- Return bytecode always calls out to a single CTI op_ret helper.
- Activation, profiler, and scope-chain handling are all behind the helper call.

## Moves

- 2008-09-16 (15b7f81b) replaced by [[call-linking-stubs]]: op_ret in CTI was a single call-out stub; replaced with inline generated code that handles the common case (no activation, no profiler, simple scope chain) directly, keeping three small out-of-line C hooks only for the rare activation/profiler/full-scope-chain paths, giving +1.5% SunSpider, +5-6% v8. (sourced)
