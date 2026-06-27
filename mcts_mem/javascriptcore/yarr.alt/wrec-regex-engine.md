- WREC is enabled on selected x86 and x86-64 Mac/Windows platforms.
- Regex execution has a JIT path but no bytecode interpreter fallback in the same engine.
- The assembler enablement is tied to JIT or WREC platform gates.

## Moves

- 2009-04-28 (463dacc4) replaced by [[yarr]]: WREC (WebKit Regular Expression Compiler) replaced by YARR (Yet Another Regex Runtime) on Mac and Windows x86/x86-64; YARR provides both a JIT tier and a bytecode interpreter fallback, while WREC had only a JIT path. (code)
