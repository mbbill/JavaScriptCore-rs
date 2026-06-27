- YarrCodeBlock stores only whether fallback is needed.
- Unsupported constructs and allocation failures collapse to a boolean fallback state.
- Pattern dumps cannot report which feature caused fallback except for ad hoc immediate diagnostics.

## Moves

- 2018-01-24 (99b1f7d2) replaced by [[jit-fallback]]: Replacing the boolean fallback with JITFailureReason lets YarrJIT preserve which unsupported construct or allocation failure caused interpreter fallback and dump it under Options::dumpCompiledRegExpPatterns. (sourced)
