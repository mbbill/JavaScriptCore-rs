- Case conversion maps each character independently.
- UChar and UCharReference expose per-character `toLower` and `toUpper` operations.
- Multi-character Unicode special casing cannot expand the output string.

## Moves

- 2006-04-08 (d97e5da8) replaced by [[unicode]]: Character-by-character case mapping cannot produce multi-character results required by Unicode special casings (e.g. German ß → SS), so toLowerCase/toUpperCase failed to honor these mappings; string-level ICU u_strToLower/u_strToUpper operates on the whole string and can expand characters. (sourced)
