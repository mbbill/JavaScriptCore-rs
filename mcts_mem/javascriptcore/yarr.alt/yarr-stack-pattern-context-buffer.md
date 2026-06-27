- RegExp matching reserves a fixed parenthesis context buffer on the C++ stack when all-parentheses JIT support is enabled.
- YARR JIT execute signatures receive the buffer pointer and size as extra parameters.
- Generated code does not carry a per-code flag saying whether it uses the pattern context buffer.

## Moves

- 2018-02-14 (dff8b0d5) replaced by [[yarr]]: A stack-local Yarr parenthesis context buffer could overflow the stack, so the buffer moved to a lazily allocated VM-owned buffer acquired only for JIT code that declares it uses pattern context. (code)
