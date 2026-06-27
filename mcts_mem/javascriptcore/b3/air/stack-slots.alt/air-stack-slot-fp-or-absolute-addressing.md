- Stack-slot addressing first tried an FP-relative address.
- If that offset was invalid, it computed an absolute address in a scratch register.
- Stack slot loads and stores consumed only concrete MacroAssembler address results.

## Moves

- 2023-02-22 (6d88e268) replaced by [[stack-slots]]: Air stack load/store address selection switched from frame-pointer-or-absolute fallback to a tiered FP, SP, FP+index, then absolute strategy because avoiding absolute-address computation significantly reduces generated code size on ARM. (sourced)
