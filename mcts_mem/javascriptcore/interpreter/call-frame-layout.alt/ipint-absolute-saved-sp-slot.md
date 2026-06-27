- Wasm IPInt stores the saved stack pointer as an absolute address in the overloaded CallFrame this slot.
- Restoring an off-stack saved frame requires the original absolute stack address to remain valid.

## Moves

- 2025-10-20 (463c854b) replaced by [[call-frame-layout]]: Storing SP as an FP-relative offset lets JSPI save frames off-stack and reinstall them at a different stack address without maintaining and relocating a list of absolute SP slots. (sourced)
