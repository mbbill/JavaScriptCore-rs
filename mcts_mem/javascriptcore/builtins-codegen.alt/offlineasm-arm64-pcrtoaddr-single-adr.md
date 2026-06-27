- ARM64 pcrtoaddr lowering always emitted a single adr instruction for a label reference.
- Extern and global labels used the same lowering as local labels.

## Moves

- 2026-02-24 (586f364e) replaced by [[builtins-codegen]]: A single arm64 adr lowering is only reliable for local labels; extern or global labels need an adrp/add pair with platform-specific relocation syntax. (sourced)
