- ARM complex-immediate materialization tried one-instruction modified-immediate encodings first.
- Non-encodable values fell back to two-instruction OR/MVN synthesis or a PC-relative literal-pool load.

## Moves

- 2009-11-05 (15b6e9ee) replaced by [[cpu-backends]]: ARMv7 (ARM_ARCH_VERSION >= 7) supports MOVW and MOVT instructions that can load a 32-bit immediate in two 16-bit-immediate instructions without a PC-relative literal pool load, eliminating the need for genInt's two-instruction OR/MVN sequence or the ldr_imm pool fallback. (code)
