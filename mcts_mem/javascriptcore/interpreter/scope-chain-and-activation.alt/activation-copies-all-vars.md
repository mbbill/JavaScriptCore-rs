- Activation objects copy every local variable, regardless of whether the variable is captured.
- Marking scans the full function variable count past the call-frame header.
- Captured and non-captured locals are not partitioned in register layout.

## Moves

- 2010-09-23 (2c8a9583) replaced by [[scope-chain-and-activation]]: The old code copied every local variable into the activation object regardless of capture status; once free-variable tracking was added to the parser, the bytecode generator can partition vars into captured (low indices) and non-captured (high indices) so the activation only needs to copy and mark m_numCapturedVars registers. (sourced)
