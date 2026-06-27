- RegExp JIT availability is gated by the same useJIT switch as the main JavaScript JIT.
- Disabling the main JIT unconditionally disables YARR JIT even when executable assembler support exists.
- No separate useYarrJIT or useRegExpJIT option exists.

## Moves

- 2012-09-05 (eed2cdb0) replaced by [[yarr]]: YarrJIT uses the assembler for regex compilation only and does not require the full JIT infrastructure; tying it to useJIT() prevented enabling regex JIT on the LLInt C loop backend (and similar configurations) where the main bytecode JIT must be disabled but the assembler is still accessible, so a separate useYarrJIT() option was introduced to allow independent control. (sourced)
