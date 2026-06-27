- Air generation assigned temporaries by spilling every Tmp to a stack slot.
- Uses and defs were rewritten through loads and stores around each instruction.
- The implementation described spill-everything as a placeholder for testing once a real allocator existed.

## Moves

- 2015-11-11 (762819f8) replaced by [[register-allocation]]: Air generation now uses a direct implementation of Appel's Iterated Register Coalescing allocator instead of spilling every tmp to a stack slot for non-testing code. (code)
