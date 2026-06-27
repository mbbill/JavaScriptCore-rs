- ARMv7 linkWithOffset encoded relative offsets directly into branch instructions during linkJump.
- Relinking recomputed relative offsets from runtime pointers and flushed the patched instruction words.
- There was no deferred jump list that could choose an absolute jump after final code placement was known.

## Moves

- 2009-10-29 (e27b33be) replaced by [[patching-relocation]]: The old linkWithOffset computed a relative offset at link time and encoded it directly into the branch instruction immediately; this failed when the offset exceeded the 16 MB range of Thumb-2 B.W, so the new scheme pre-plans all jumps as MOV/MOVT+BX sequences (absolute), and defers final encoding to executableCopy() where the actual target address is known, falling back to a relative B.W only if the distance fits. (sourced)
