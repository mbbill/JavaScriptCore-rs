- PCRE compiles UTF-8 buffers produced from JSC's UTF-16 strings.
- Matching converts UTF-16 offsets to UTF-8 byte offsets before pcre_exec.
- Result offsets are translated back from UTF-8 byte offsets to UTF-16 indexes.

## Moves

- 2004-08-10 (2394c5c0) replaced by [[yarr]]: The old mechanism converted UString (UTF-16) to UTF-8 before passing to pcre_compile/pcre_exec, then converted PCRE's byte offsets back to UTF-16 offsets after matching; eliminating the round-trip conversion by extending PCRE to operate natively on uint16_t removes the allocation and offset-translation overhead on every regex match. (sourced)
