- Windows x86_64 JIT entrypoints and operations use MS ABI shadow-space and hidden-return-pointer shims.

## Moves

- 2024-06-20 (0b1e4218) replaced by [[call-linking-stubs]]: JSC marks JIT entrypoints and operations SYSV_ABI on Windows so Baseline JIT calls can use the same argument and multi-register return convention as other x86_64 ports instead of Windows shadow-space and hidden-return-pointer shims. (code)
