- ExecutablePool allocation was assumed to succeed or crash.
- LinkBuffer and JIT callers proceeded without a recoverable allocation-success check.
- YARR JIT compilation had no allocation-failure fallback path.

## Moves

- 2010-08-04 (ae752df2) replaced by [[executable-memory]]: JIT code allocation exhaustion previously hit ASSERT/CRASH; changed so allocators return null, LinkBuffer exposes allocationSuccessful(), JIT throws a JS out-of-memory exception, and YARR falls back to PCRE, enabling recovery instead of process abort. (code)
