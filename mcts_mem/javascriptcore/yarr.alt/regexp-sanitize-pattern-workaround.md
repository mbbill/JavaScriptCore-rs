- UString sanitizePattern(const UString& p) — scan for \uXXXX, decode to UChar, escape metacharacters, return rewritten pattern
- two-pass pcre_compile: first attempt without sanitize, second attempt with sanitizePattern if first fails
- escaping of |+*()[]{}?\\ after \uXXXX decode

## Moves

- 2007-07-19 (2cd3a93e) replaced by [[yarr]]: sanitizePattern pre-processed \uXXXX escapes at the KJS level by converting them to UTF-16 characters and inserting backslashes before special regex metacharacters, but this was fragile (the switch of characters to escape was hand-enumerated) and did not fix the underlying PCRE length-preflighting bugs caused by unsupported advanced features; adding a JAVASCRIPT-mode escape table inside PCRE natively handles \u and \v and also allows systematically disabling all non-JS features (named recursion, POSIX classes, etc.) that caused incorrect preflight lengths. (sourced)
