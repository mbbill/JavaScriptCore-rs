- YARR and YARR_JIT enablement is enumerated by specific CPU, OS, compiler, and port combinations.
- Each port carries its own regexp-JIT enable block.
- YARR availability is not expressed as one consequence of assembler/JIT availability.

## Moves

- 2010-05-09 (48eeb64a) replaced by [[yarr]]: Per-port YARR enable rules (enumerating specific CPU/OS/platform combos) were replaced with a single rule enabling YARR and YARR_JIT whenever JIT is enabled, since YARR requires JIT and the redundant guards were scattered per-port. (code)
