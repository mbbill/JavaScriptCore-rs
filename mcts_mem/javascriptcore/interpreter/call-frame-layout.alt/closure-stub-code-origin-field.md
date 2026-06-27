- Closure call stub routines store a duplicate CodeOrigin field.
- Return-PC lookup searches closure call stubs to recover the original bytecode call site.

## Moves

- 2015-01-20 (f2bf0a76) replaced by [[call-frame-layout]]: CodeOrigin for call frames is now determined from encoded code-origin bits inside the argument-count tag, and callers that find a ClosureCallStubRoutine through CallLinkInfo can use CallLinkInfo's CodeOrigin instead of a duplicate field on the stub. (sourced)
