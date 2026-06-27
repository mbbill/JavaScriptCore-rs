- Wasm memory owns a VM-local weak set of JavaScript WebAssembly instances. (`Wasm::Memory`)
- Shared-memory growth iterates the remembered instances on the current thread to update cached memory state.

## Moves

- 2025-12-09 (597e0085) replaced by [[memory-model]]: Tracking instances in Wasm::Memory only updated instances remembered by the current thread, so growth of shared memory moved cache invalidation to the BufferMemoryHandle's ThreadSafeWeakHashSet of InstanceAnchor objects shared by all threads. (sourced)
