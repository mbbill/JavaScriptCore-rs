- B3::generate optimized, lowered to a locally-created Air::Code, ran all Air phases, and emitted machine code in one call.
- Air::generate both prepared Air and emitted machine code, including callback execution.
- generateToAir accepted externally-owned Air::Code with Procedure-owned pointer dependencies and external lifetime requirements.

## Moves

- 2015-11-18 (257cf285) replaced by [[b3]]: The B3 pipeline was split so expensive B3 and Air preparation can run inside the FTL graph safepoint while final machine-code generation and stackmap callbacks run after leaving it. (sourced)
