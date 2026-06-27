- Register-file extent is tracked as a size_t count from the register-file base.
- Growth and restoration APIs save old sizes and recompute end addresses as base plus size.

## Moves

- 2008-10-02 (6d4e2a5a) replaced by [[interpreter]]: Tracking RegisterFile extent as a Register* end pointer instead of a size_t integer eliminates per-call pointer arithmetic (base + size), yielding 2–3% speedup on V8 DeltaBlue and Raytrace. (sourced)
