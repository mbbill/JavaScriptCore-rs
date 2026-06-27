- MatchOnly mode rejects patterns with backreferences in the JIT.
- Subpattern recording helpers read and write only through the external output vector.
- ParenContext sizing and capture save/restore are enabled only for capture-producing mode.

## Moves

- 2026-02-05 (98d1969d) replaced by [[jit-fallback]]: MatchOnly backreferences changed from JIT fallback to internal frame subpattern storage because MatchOnly has no external output vector but backreferences need capture start/end data during matching. (code)
