- Windows x86-64 YARR JIT uses a Windows-specific register map.
- MatchResult is returned through a hidden return pointer.
- RegExp JIT is disabled on Windows rather than sharing the generic x86-64 entry contract.

## Moves

- 2024-06-26 (778df0f6) replaced by [[jit-codegen]]: Windows Yarr JIT now uses SYSV_ABI and the generic x86_64 register contract, allowing RegExp JIT to remain enabled on Windows instead of carrying a separate hidden-return-pointer ABI path. (code)
