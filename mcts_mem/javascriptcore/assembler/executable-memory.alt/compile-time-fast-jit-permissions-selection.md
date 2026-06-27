- ARM64E builds were treated as having the fast JIT-permission path at compile time.
- Non-ARM64 builds exposed constexpr false permission helpers.

## Moves

- 2021-03-18 (756de704) replaced by [[executable-memory]]: Fast JIT permission support is selected once during JIT page reservation using runtime API availability checks and stored in g_jscConfig, instead of treating every ARM64E build as supporting the fast permissions path. (code)
