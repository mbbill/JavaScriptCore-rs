- regcomp/regexec compile and execute regexp patterns through the POSIX regex library.
- Pattern and subject strings are converted through ASCII-facing UString accessors.
- Capture count reporting falls back to zero subpatterns.

## Moves

- 2002-12-04 (2b173bf5) replaced by [[yarr]]: POSIX regex (regcomp/regexec) did not support Multiline semantics and had correctness gaps causing multiple real-site form-validation failures; PCRE 3.9 was vendored to fix these. (sourced)
