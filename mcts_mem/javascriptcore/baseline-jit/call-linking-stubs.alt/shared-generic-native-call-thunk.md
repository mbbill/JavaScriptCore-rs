- Native calls share a generic thunk that loads the NativeExecutable function pointer at the call site.

## Moves

- 2010-05-19 (da99e8cb) replaced by [[call-linking-stubs]]: The shared-thunk approach introduced an extra load of NativeExecutable::m_function at every native call site, regressing i386 performance; per-NativeFunction thunks bake the C function pointer as an immediate into the generated JIT code so no load is needed at the call site. (sourced)
