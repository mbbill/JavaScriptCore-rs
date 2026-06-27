- Executable-byte patching copies code bytes with memcpy.
- Patch sites do not distinguish atomic and tearing-allowed writes.

## Moves

- 2024-11-07 (169e231f) replaced by [[code-allocation-patching]]: JIT code repatching sometimes needs atomic writes, so copying executable bytes now uses relaxed atomic stores for 1-, 2-, 4-, and 8-byte writes on architectures that do not need aligned access and falls back to memcpy otherwise. (sourced)
