- DFG refused to inline bytecodes that create, tear off, index, or query reflective `arguments`.
- Argument operations addressed only the current CodeBlock arguments register.

## Moves

- 2012-05-23 (be575a09) replaced by [[call-dispatch]]: Inlining functions that use arguments reflectively required arguments creation, tear-off, length, and indexed access to be addressed through the relevant InlineCallFrame rather than only the current CodeBlock's arguments register. (code)
