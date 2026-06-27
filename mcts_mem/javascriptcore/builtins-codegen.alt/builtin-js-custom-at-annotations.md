- Builtin JS files used custom @linkTimeConstant and @alwaysInline annotations.
- Named constants were expressed through bytecode-intrinsic plumbing rather than preprocessing.

## Moves

- 2026-05-31 (17e27ee7) replaced by [[builtins-codegen]]: The custom builtin annotation and BytecodeIntrinsic constant mechanism could not express compile-time flags or simple named constants, so builtin JS sources were routed through a C preprocessor with JSC_BUILTIN_* macros. (sourced)
