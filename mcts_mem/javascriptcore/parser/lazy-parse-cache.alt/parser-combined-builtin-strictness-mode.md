- Parser strictness had one combined enum value space for normal, builtin, and strict parsing.
- Cache keys could not independently represent builtin parsing and strict-mode parsing.

## Moves

- 2015-03-17 (225f3e93) replaced by [[lazy-parse-cache]]: Builtin-ness and strictness had to be represented independently because builtin functions use strict mode while still needing builtin lexing and cache separation. (sourced)
