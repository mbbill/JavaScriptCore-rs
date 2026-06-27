- VM stored bool isExecutingInRegExpJIT initialized false.
- YarrGenerator held VM* solely to store 1 to VM::isExecutingInRegExpJIT on generated entry and 0 on generated return.
- SamplingProfiler::takeSample checked m_vm.isExecutingInRegExpJIT during thread suspension but only had a flag, not the RegExp identity.

## Moves

- 2018-01-29 (88fdfc6a) replaced by [[jit-codegen]]: A VM boolean toggled inside generated Yarr code could be sampled before it was set or after it was cleared and forced YarrJIT to depend on VM, so the replacement marks the exact RegExp object with a main-thread RAII tracer around JIT execution and lets the profiler emit RegExp frames. (sourced)
