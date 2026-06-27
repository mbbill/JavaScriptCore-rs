- VM owned the executable allocator as part of VM lifetime.
- LinkBuffer stored a VM pointer and allocated executable code through the VM.
- Delayed linking callbacks could recover VM state from LinkBuffer.

## Moves

- 2017-03-29 (4ed0e2b9) replaced by [[executable-memory]]: LinkBuffer and ExecutableAllocator were detached from VM ownership so generated code and executable memory allocation would not carry a VM dependency while moving WebAssembly toward position-independent code. (sourced)
