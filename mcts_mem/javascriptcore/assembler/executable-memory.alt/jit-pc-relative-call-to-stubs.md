- JIT calls and jumps shared one x86 link path that patched relative offsets.
- Call records could be treated as relative branches to interpreter stubs.

## Moves

- 2009-02-19 (77607cda) replaced by [[executable-memory]]: On x86-64 the JSC text segment can lie >2GB from the JIT heap, making 32-bit pc-relative calls to Interpreter stub functions unreachable; x86-64 calls to out-of-range targets must go through an indirect mov-r11/call-r11 sequence instead. (sourced)
